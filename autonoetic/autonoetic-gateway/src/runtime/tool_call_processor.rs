//! Tool Call Processor for Agent Execution.
//!
//! Handles tool execution, disclosure tracking, and secret store integration.

use crate::runtime::disclosure::DisclosureState;
use crate::runtime::mcp::McpToolRuntime;
use crate::runtime::session_tracer::SessionTracer;
use crate::runtime::store::SecretStoreRuntime;
use crate::runtime::tools::NativeToolRegistry;
use crate::llm::ToolCall;
use autonoetic_types::agent::AgentManifest;
use autonoetic_types::disclosure::DisclosureClass;
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

            let result = self.execute_tool_call(tc, agent_dir, gateway_dir).await?;

            tracer.log_tool_completed(&tc.name, &result)?;

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
}
