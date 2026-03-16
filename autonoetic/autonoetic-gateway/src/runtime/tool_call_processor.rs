//! Tool Call Processor for Agent Execution.
//!
//! Handles tool execution, disclosure tracking, and secret store integration.
//! Returns structured error responses for recoverable failures instead of aborting.

use crate::llm::ToolCall;
use crate::runtime::disclosure::DisclosureState;
use crate::runtime::mcp::McpToolRuntime;
use crate::runtime::session_tracer::SessionTracer;
use crate::runtime::store::SecretStoreRuntime;
use crate::runtime::tools::NativeToolRegistry;
use autonoetic_types::agent::AgentManifest;
use autonoetic_types::config::GatewayConfig;
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
    /// When set, passed to native tools (e.g. agent.install for approval policy).
    config: Option<&'a GatewayConfig>,
}

impl<'a> ToolCallProcessor<'a> {
    fn canonical_tool_name(name: &str) -> &str {
        match name {
            "spawn" => "agent.spawn",
            "install" => "agent.install",
            "message" => "agent.message",
            "search" => "web.search",
            "fetch" => "web.fetch",
            _ => name,
        }
    }

    pub fn new(
        mcp_runtime: &'a mut McpToolRuntime,
        registry: &'a NativeToolRegistry,
        manifest: &'a AgentManifest,
        disclosure_state: &'a mut DisclosureState,
        secret_store: Option<&'a mut SecretStoreRuntime>,
        config: Option<&'a GatewayConfig>,
    ) -> Self {
        Self {
            mcp_runtime,
            registry,
            manifest,
            disclosure_state,
            secret_store,
            session_id: None,
            turn_id: None,
            config,
        }
    }

    pub fn with_session_context(
        mut self,
        session_id: Option<String>,
        turn_id: Option<String>,
    ) -> Self {
        self.session_id = session_id;
        self.turn_id = turn_id;
        self
    }

    /// Processes tool calls and returns `(had_any_success, results)`.
    /// `had_any_success` is `true` if at least one call completed successfully;
    /// the execution loop uses this to decide whether to reset the loop-guard counter.
    /// Recoverable errors are returned as structured error JSON in the result.
    /// Only fatal errors cause the entire operation to fail.
    pub async fn process_tool_calls(
        &mut self,
        tool_calls: &[ToolCall],
        agent_dir: &Path,
        gateway_dir: Option<&Path>,
        tracer: &mut SessionTracer,
    ) -> anyhow::Result<(bool, Vec<(String, String, String)>)> {
        let mut results = Vec::with_capacity(tool_calls.len());
        let mut had_any_success = false;

        for tc in tool_calls {
            tracer.log_tool_requested(&tc.name, &tc.arguments)?;

            // Execute tool call, handling errors appropriately
            let result = match self.execute_tool_call(tc, agent_dir, gateway_dir).await {
                Ok(res) => {
                    // Success - log and continue
                    self.log_memory_tool_event(tracer, &tc.name, &res);
                    tracer.log_tool_completed(&tc.name, &res)?;
                    had_any_success = true;
                    res
                }
                Err(e) => {
                    // Convert to structured error
                    let tool_error: ToolError = e.into();

                    // Log the failure to causal chain - this must succeed
                    // as audit trail integrity is critical for governance
                    self.log_tool_failure(tracer, tc, &tool_error)?;

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

        Ok((had_any_success, results))
    }

    async fn execute_tool_call(
        &mut self,
        tc: &ToolCall,
        agent_dir: &Path,
        gateway_dir: Option<&Path>,
    ) -> anyhow::Result<String> {
        let tool_name = Self::canonical_tool_name(&tc.name);
        let policy = crate::policy::PolicyEngine::new(self.manifest.clone());
        let mut result = if self.mcp_runtime.has_tool(tool_name) {
            if !policy.can_invoke_tool(tool_name) {
                return Err(anyhow::Error::from(
                    autonoetic_types::tool_error::tagged::Tagged::permission(anyhow::anyhow!(
                        "Tool '{}' is not allowed by ToolInvoke capability",
                        tool_name
                    )),
                ));
            }
            self.mcp_runtime.call_tool(tool_name, &tc.arguments).await?
        } else if self.registry.has_tool(tool_name) {
            self.registry.execute(
                tool_name,
                self.manifest,
                &policy,
                agent_dir,
                gateway_dir,
                &tc.arguments,
                self.session_id.as_deref(),
                self.turn_id.as_deref(),
                self.config,
            )?
        } else {
            return Err(anyhow::Error::from(
                 autonoetic_types::tool_error::tagged::Tagged::resource(anyhow::anyhow!(
                     "Unknown tool '{}'. Verify the tool name against the available tools list and retry with the correct name.",
                     tc.name
                 )),
             ));
        };

        let tc_meta = self.registry.extract_metadata(tool_name, &tc.arguments);
        self.disclosure_state
            .register_result(tool_name, tc_meta.path.as_deref(), &result);

        if let Some(store) = &mut self.secret_store {
            let (new_result, extracted_secrets) = store.apply_and_redact(&result)?;
            result = new_result;
            for s in extracted_secrets {
                self.disclosure_state
                    .register_explicit_taint(&s, DisclosureClass::Secret);
            }
        }

        Ok(result)
    }

    fn log_tool_failure(
        &self,
        tracer: &mut SessionTracer,
        tc: &ToolCall,
        error: &ToolError,
    ) -> anyhow::Result<()> {
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
        )
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

        let _ = tracer.log_event(
            "memory",
            action,
            autonoetic_types::causal_chain::EntryStatus::Success,
            Some(payload),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::tools::default_registry;
    use autonoetic_types::agent::{AgentIdentity, RuntimeDeclaration};
    use autonoetic_types::capability::Capability;
    use autonoetic_types::tool_error::{tagged, ToolErrorType};
    use tempfile::tempdir;

    fn test_manifest() -> AgentManifest {
        AgentManifest {
            version: "1.0".to_string(),
            runtime: RuntimeDeclaration {
                engine: "autonoetic".to_string(),
                gateway_version: "0.1.0".to_string(),
                sdk_version: "0.1.0".to_string(),
                runtime_type: "stateful".to_string(),
                sandbox: "bubblewrap".to_string(),
                runtime_lock: "runtime.lock".to_string(),
            },
            agent: AgentIdentity {
                id: "test-agent".to_string(),
                name: "test-agent".to_string(),
                description: "test".to_string(),
            },
            capabilities: vec![
                Capability::ReadAccess {
                    scopes: vec!["*".to_string()],
                },
                Capability::WriteAccess {
                    scopes: vec!["*".to_string()],
                },
            ],
            llm_config: None,
            limits: None,
            background: None,
            disclosure: None,
            io: None,
            middleware: None,
            execution_mode: Default::default(),
            script_entry: None,
            gateway_url: None,
            gateway_token: None,
        }
    }

    #[tokio::test]
    async fn test_recoverable_error_returns_structured_json() {
        let temp = tempdir().unwrap();
        let manifest = test_manifest();
        let mut mcp_runtime = crate::runtime::mcp::McpToolRuntime::empty();
        let registry = default_registry();
        let mut disclosure_state = DisclosureState::default();

        let mut processor = ToolCallProcessor::new(
            &mut mcp_runtime,
            &registry,
            &manifest,
            &mut disclosure_state,
            None,
            None,
        );

        let tool_calls = vec![ToolCall {
            id: "tc1".to_string(),
            name: "knowledge.store".to_string(),
            arguments: r#"{"id":"","content":"hello"}"#.to_string(),
        }];

        let (_, result) = processor
            .process_tool_calls(
                &tool_calls,
                temp.path(),
                None,
                &mut SessionTracer::test_tracer(),
            )
            .await
            .unwrap();

        assert_eq!(result.len(), 1);
        let (_, _, tool_result) = &result[0];

        // Should be a structured error JSON, not a panic
        let parsed: serde_json::Value = serde_json::from_str(tool_result).unwrap();
        assert_eq!(parsed.get("ok").unwrap(), false);
        // The error could be "resource" (unknown tool) or "validation" depending on tool availability
        assert!(parsed.get("error_type").unwrap().as_str().unwrap() == "resource" || 
                parsed.get("error_type").unwrap().as_str().unwrap() == "validation");
        assert!(parsed
            .get("message")
            .unwrap()
            .as_str()
            .unwrap()
            .contains("must not be empty") || parsed
            .get("message")
            .unwrap()
            .as_str()
            .unwrap()
            .contains("not found"));
    }

    #[tokio::test]
    async fn test_unknown_tool_returns_recoverable_resource_error() {
        let temp = tempdir().unwrap();
        let manifest = test_manifest();
        let mut mcp_runtime = crate::runtime::mcp::McpToolRuntime::empty();
        let registry = default_registry();
        let mut disclosure_state = DisclosureState::default();

        let mut processor = ToolCallProcessor::new(
            &mut mcp_runtime,
            &registry,
            &manifest,
            &mut disclosure_state,
            None,
            None,
        );

        // Unknown tool is now recoverable so the agent can self-repair by
        // retrying with the correct tool name in the next turn.
        let tool_calls = vec![ToolCall {
            id: "tc1".to_string(),
            name: "unknown.tool".to_string(),
            arguments: "{}".to_string(),
        }];

        let (had_success, results) = processor
            .process_tool_calls(
                &tool_calls,
                temp.path(),
                None,
                &mut SessionTracer::test_tracer(),
            )
            .await
            .expect("unknown tool must not abort session");

        assert!(!had_success, "unknown tool must not count as success");
        assert_eq!(results.len(), 1);
        let parsed: serde_json::Value = serde_json::from_str(&results[0].2).unwrap();
        assert_eq!(parsed["ok"], false);
        assert_eq!(parsed["error_type"], "resource");
    }

    #[tokio::test]
    async fn test_multiple_tool_calls_with_mixed_results() {
        let temp = tempdir().unwrap();
        let manifest = test_manifest();
        let mut mcp_runtime = crate::runtime::mcp::McpToolRuntime::empty();
        let registry = default_registry();
        let mut disclosure_state = DisclosureState::default();

        let mut processor = ToolCallProcessor::new(
            &mut mcp_runtime,
            &registry,
            &manifest,
            &mut disclosure_state,
            None,
            None,
        );

        // First call fails (validation), second would succeed if we had gateway_dir
        let tool_calls = vec![
            ToolCall {
                id: "tc1".to_string(),
                name: "knowledge.store".to_string(),
                arguments: r#"{"id":"","content":"hello"}"#.to_string(),
            },
            ToolCall {
                id: "tc2".to_string(),
                name: "knowledge.recall".to_string(),
                arguments: r#"{"id":"some-id"}"#.to_string(),
            },
        ];

        let (_, result) = processor
            .process_tool_calls(
                &tool_calls,
                temp.path(),
                None,
                &mut SessionTracer::test_tracer(),
            )
            .await
            .unwrap();

        // Both calls should complete (first with validation error, second with resource error for missing gateway)
        assert_eq!(result.len(), 2);

        // First is validation error for empty id or resource/execution error if tool not available
        let parsed1: serde_json::Value = serde_json::from_str(&result[0].2).unwrap();
        assert_eq!(parsed1.get("ok").unwrap(), false);
        let error_type1 = parsed1.get("error_type").unwrap().as_str().unwrap();
        assert!(error_type1 == "resource" || error_type1 == "validation" || error_type1 == "execution",
                "error_type1 was: {}", error_type1);

        // Second is execution/resource error for missing gateway_dir
        let parsed2: serde_json::Value = serde_json::from_str(&result[1].2).unwrap();
        assert_eq!(parsed2.get("ok").unwrap(), false);
        let error_type2 = parsed2.get("error_type").unwrap().as_str().unwrap();
        assert!(error_type2 == "resource" || error_type2 == "execution",
                "error_type2 was: {}", error_type2);
    }

    #[tokio::test]
    async fn test_in_session_repair_loop_recovery_from_structured_error() {
        let temp = tempdir().unwrap();
        let gw_dir = temp.path().join("gateway");
        std::fs::create_dir_all(&gw_dir).unwrap();

        let manifest = test_manifest();
        let mut mcp_runtime = crate::runtime::mcp::McpToolRuntime::empty();
        let registry = default_registry();
        let mut disclosure_state = DisclosureState::default();

        let mut processor = ToolCallProcessor::new(
            &mut mcp_runtime,
            &registry,
            &manifest,
            &mut disclosure_state,
            None,
            None,
        );

        // First turn: malformed tool call - empty id triggers validation error
        let tool_calls_turn1 = vec![ToolCall {
            id: "tc1".to_string(),
            name: "knowledge.store".to_string(),
            arguments: r#"{"id":"","content":"hello"}"#.to_string(),
        }];

        let (had_success_turn1, result_turn1) = processor
            .process_tool_calls(
                &tool_calls_turn1,
                temp.path(),
                Some(gw_dir.as_path()),
                &mut SessionTracer::test_tracer(),
            )
            .await
            .unwrap();

        assert_eq!(result_turn1.len(), 1);

        assert!(
            !had_success_turn1,
            "failed tool call must not count as success"
        );
        // Parse the error response - could be resource (unknown tool) or validation (empty id)
        let parsed_error: serde_json::Value = serde_json::from_str(&result_turn1[0].2).unwrap();
        assert_eq!(parsed_error.get("ok").unwrap(), false);
        let error_type = parsed_error.get("error_type").unwrap().as_str().unwrap();
        assert!(error_type == "validation" || error_type == "resource");
        if error_type == "validation" {
            assert!(parsed_error.get("repair_hint").is_some());
            // Extract the repair hint for the agent to use
            let repair_hint = parsed_error.get("repair_hint").unwrap().as_str().unwrap();
            assert!(repair_hint.contains("id") || repair_hint.contains("field"));
        }

        // Second turn: agent reads error, corrects the tool call with valid id
        let tool_calls_turn2 = vec![ToolCall {
            id: "tc2".to_string(),
            name: "knowledge.store".to_string(),
            arguments: r#"{"id":"valid-id-123","content":"hello world"}"#
                .to_string(),
        }];

        let (had_success_turn2, result_turn2) = processor
            .process_tool_calls(
                &tool_calls_turn2,
                temp.path(),
                Some(gw_dir.as_path()),
                &mut SessionTracer::test_tracer(),
            )
            .await
            .unwrap();

        assert_eq!(result_turn2.len(), 1);
        assert!(
            had_success_turn2,
            "successful tool call must set had_any_success"
        );

        // This time it should succeed
        let parsed_success: serde_json::Value = serde_json::from_str(&result_turn2[0].2).unwrap();
        assert_eq!(parsed_success.get("ok").unwrap(), true);
        // knowledge.store returns "id" field, not "memory_id"
        assert!(parsed_success.get("id").is_some() || parsed_success.get("memory_id").is_some());
    }

    #[test]
    fn test_canonical_tool_name_aliases() {
        assert_eq!(
            ToolCallProcessor::canonical_tool_name("spawn"),
            "agent.spawn"
        );
        assert_eq!(
            ToolCallProcessor::canonical_tool_name("install"),
            "agent.install"
        );
        assert_eq!(
            ToolCallProcessor::canonical_tool_name("message"),
            "agent.message"
        );
        assert_eq!(
            ToolCallProcessor::canonical_tool_name("search"),
            "web.search"
        );
        assert_eq!(ToolCallProcessor::canonical_tool_name("fetch"), "web.fetch");
        assert_eq!(
            ToolCallProcessor::canonical_tool_name("agent.spawn"),
            "agent.spawn"
        );
        assert_eq!(
            ToolCallProcessor::canonical_tool_name("web.search"),
            "web.search"
        );
    }

    #[test]
    fn test_tagged_error_explicit_classification() {
        // Test that tagged::Tagged provides explicit classification
        let tagged = tagged::Tagged::validation(anyhow::anyhow!("some error"));
        let tool_error: ToolError = tagged.into();
        assert_eq!(tool_error.error_type, ToolErrorType::Validation);
        assert!(tool_error.is_recoverable());

        let tagged = tagged::Tagged::fatal(anyhow::anyhow!("corrupted state"));
        let tool_error: ToolError = tagged.into();
        assert_eq!(tool_error.error_type, ToolErrorType::Fatal);
        assert!(!tool_error.is_recoverable());

        let tagged = tagged::Tagged::permission(anyhow::anyhow!("access denied"));
        let tool_error: ToolError = tagged.into();
        assert_eq!(tool_error.error_type, ToolErrorType::Permission);
        assert!(tool_error.is_recoverable());

        let tagged = tagged::Tagged::resource(anyhow::anyhow!("file not found"));
        let tool_error: ToolError = tagged.into();
        assert_eq!(tool_error.error_type, ToolErrorType::Resource);
        assert!(tool_error.is_recoverable());

        let tagged = tagged::Tagged::execution(anyhow::anyhow!("unexpected result"));
        let tool_error: ToolError = tagged.into();
        assert_eq!(tool_error.error_type, ToolErrorType::Execution);
        assert!(tool_error.is_recoverable());
    }
}
