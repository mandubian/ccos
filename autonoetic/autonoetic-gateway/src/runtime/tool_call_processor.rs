//! Tool Call Processor for Agent Execution.
//!
//! Handles tool execution, disclosure tracking, and secret store integration.
//! Returns structured error responses for recoverable failures instead of aborting.

use crate::runtime::disclosure::DisclosureState;
use crate::runtime::mcp::McpToolRuntime;
use crate::runtime::session_tracer::SessionTracer;
use crate::runtime::store::SecretStoreRuntime;
use crate::runtime::tools::NativeToolRegistry;
use crate::llm::ToolCall;
use autonoetic_types::agent::AgentManifest;
use autonoetic_types::disclosure::DisclosureClass;
use autonoetic_types::tool_error::ToolError;
use std::path::Path;

pub struct ToolCallProcessor<'a> {
    mcp_runtime: &'a mut McpToolRuntime,
    registry: &'a NativeToolRegistry,
    manifest: &'a AgentManifest,
    disclosure_state: &'a mut DisclosureState,
    secret_store: Option<&'a mut SecretStoreRuntime>,
    session_id: Option<String>,
    turn_id: Option<String>,
}

impl<'a> ToolCallProcessor<'a> {
    pub fn new(
        mcp_runtime: &'a mut McpToolRuntime,
        registry: &'a NativeToolRegistry,
        manifest: &'a AgentManifest,
        disclosure_state: &'a mut DisclosureState,
        secret_store: Option<&'a mut SecretStoreRuntime>,
    ) -> Self {
        Self {
            mcp_runtime,
            registry,
            manifest,
            disclosure_state,
            secret_store,
            session_id: None,
            turn_id: None,
        }
    }

    pub fn with_session_context(mut self, session_id: Option<String>, turn_id: Option<String>) -> Self {
        self.session_id = session_id;
        self.turn_id = turn_id;
        self
    }

     /// Processes tool calls and returns results for all calls.
     /// Recoverable errors are returned as structured error JSON in the result.
     /// Only fatal errors cause the entire operation to fail.
     pub async fn process_tool_calls(
        &mut self,
        tool_calls: &[ToolCall],
        agent_dir: &Path,
        gateway_dir: Option<&Path>,
        tracer: &mut SessionTracer,
    ) -> anyhow::Result<Vec<(String, String, String)>> {
        let mut results = Vec::with_capacity(tool_calls.len());

        for tc in tool_calls {
            tracer.log_tool_requested(&tc.name, &tc.arguments)?;

            // Execute tool call, handling errors appropriately
            let result = match self.execute_tool_call(tc, agent_dir, gateway_dir).await {
                Ok(res) => {
                    // Success - log and continue
                    self.log_memory_tool_event(tracer, &tc.name, &res);
                    tracer.log_tool_completed(&tc.name, &res)?;
                    res
                }
                Err(e) => {
                    // Convert to structured error
                    let tool_error: ToolError = e.into();
                    
                    // Log the failure to causal chain
                    self.log_tool_failure(tracer, tc, &tool_error);
                    
                    // Fatal errors abort the session
                    if !tool_error.is_recoverable() {
                        return Err(anyhow::anyhow!(
                            "Fatal tool error in {}: {}",
                            tc.name,
                            tool_error.message
                        ));
                    }
                    
                    // Recoverable errors are returned as structured JSON
                    let error_json = tool_error.to_json_string();
                    tracer.log_tool_completed(&tc.name, &error_json)?;
                    error_json
                }
            };

            results.push((tc.id.clone(), tc.name.clone(), result));
        }

        Ok(results)
    }

    async fn execute_tool_call(
        &mut self,
        tc: &ToolCall,
        agent_dir: &Path,
        gateway_dir: Option<&Path>,
    ) -> anyhow::Result<String> {
        let mut result = if self.mcp_runtime.has_tool(&tc.name) {
            self.mcp_runtime.call_tool(&tc.name, &tc.arguments).await?
        } else if self.registry.has_tool(&tc.name) {
            self.registry.execute(
                &tc.name,
                self.manifest,
                &crate::policy::PolicyEngine::new(self.manifest.clone()),
                agent_dir,
                gateway_dir,
                &tc.arguments,
                self.session_id.as_deref(),
                self.turn_id.as_deref(),
            )?
        } else {
            anyhow::bail!("Unknown tool '{}'", tc.name)
        };

        let tc_meta = self.registry.extract_metadata(&tc.name, &tc.arguments);
        self.disclosure_state.register_result(
            &tc.name,
            tc_meta.path.as_deref(),
            &result,
        );

        if let Some(store) = &mut self.secret_store {
            let (new_result, extracted_secrets) = store.apply_and_redact(&result)?;
            result = new_result;
            for s in extracted_secrets {
                self.disclosure_state.register_explicit_taint(
                    &s,
                    DisclosureClass::Secret,
                );
            }
        }

        Ok(result)
    }

    fn log_tool_failure(
        &self,
        tracer: &mut SessionTracer,
        tc: &ToolCall,
        error: &ToolError,
    ) {
        let payload = serde_json::json!({
            "tool_name": tc.name,
            "tool_id": tc.id,
            "error_type": error.error_type,
            "message": error.message,
            "repair_hint": error.repair_hint,
            "recoverable": error.is_recoverable(),
        });

        tracer.log_event(
            "tool",
            "failure",
            autonoetic_types::causal_chain::EntryStatus::Error,
            Some(payload),
        );
    }

    fn log_memory_tool_event(&self, tracer: &mut SessionTracer, tool_name: &str, result: &str) {
        let action = match tool_name {
            "memory.remember" => "remember",
            "memory.recall" => "recall",
            "memory.search" => "search",
            "memory.share" => "share",
            _ => return,
        };

        let parsed = match serde_json::from_str::<serde_json::Value>(result) {
            Ok(value) => value,
            Err(_) => return,
        };

        let payload = serde_json::json!({
            "tool_name": tool_name,
            "memory_id": parsed.get("memory_id").and_then(|v| v.as_str()),
            "scope": parsed.get("scope").and_then(|v| v.as_str()),
            "count": parsed.get("count").and_then(|v| v.as_u64()),
            "source_ref": parsed.get("source_ref").and_then(|v| v.as_str()),
            "visibility": parsed.get("visibility").cloned(),
        });

        tracer.log_event(
            "memory",
            action,
            autonoetic_types::causal_chain::EntryStatus::Success,
            Some(payload),
        );
    }
}
