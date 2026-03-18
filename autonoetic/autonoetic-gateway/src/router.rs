//! Internal JSON-RPC 2.0 Router.

use crate::execution::{
    gateway_actor_id, gateway_root_dir, init_gateway_causal_logger, sha256_hex,
    GatewayExecutionService, SpawnResult,
};
use crate::scheduler::append_task_board_entry;
use crate::tracing::{EventScope, SessionId, TraceSession};
#[cfg(test)]
use autonoetic_types::causal_chain::EntryStatus;
use autonoetic_types::config::GatewayConfig;
use autonoetic_types::task_board::{TaskBoardEntry, TaskStatus};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
#[cfg(test)]
use std::future::Future;
use std::sync::Arc;

#[derive(Debug)]
enum IngressType {
    Spawn {
        agent_id: String,
        source_agent_id: Option<String>,
        message: String,
        metadata: Option<serde_json::Value>,
    },
    Ingest {
        target_agent_id: String,
        source_agent_id: Option<String>,
        event_type: String,
        message: String,
        metadata: Option<serde_json::Value>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: String,
    pub method: String,
    pub params: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: String,
    // Provide either result or error
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

impl JsonRpcResponse {
    pub fn success(id: String, result: serde_json::Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: Some(result),
            error: None,
        }
    }

    pub fn error(id: String, code: i32, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error: Some(JsonRpcError {
                code,
                message: message.into(),
                data: None,
            }),
        }
    }
}

#[derive(Clone)]
pub struct JsonRpcRouter {
    config: Arc<GatewayConfig>,
    execution: Arc<GatewayExecutionService>,
}

impl JsonRpcRouter {
    pub fn new(config: GatewayConfig) -> Self {
        let execution = Arc::new(GatewayExecutionService::new(config.clone()));
        Self {
            config: Arc::new(config),
            execution,
        }
    }

    pub fn execution_service(&self) -> Arc<GatewayExecutionService> {
        self.execution.clone()
    }

    async fn execute_agent_request(
        &self,
        ingress: IngressType,
        session_id: String,
    ) -> Result<(SpawnResult, Option<TraceSession>), (String, Option<TraceSession>)> {
        let (
            action_name,
            agent_id,
            source_agent_id,
            message,
            event_type_for_inbox,
            event_type_str,
            metadata_for_trace,
        ) = match &ingress {
            IngressType::Spawn {
                agent_id,
                source_agent_id,
                message,
                metadata,
            } => {
                let kickoff = match metadata {
                    Some(value) => format!("{}\n\nDelegation metadata: {}", message, value),
                    None => message.clone(),
                };
                (
                    "agent.spawn",
                    agent_id.clone(),
                    source_agent_id.clone(),
                    kickoff,
                    None,
                    None,
                    metadata.clone(),
                )
            }
            IngressType::Ingest {
                target_agent_id,
                source_agent_id,
                event_type,
                message,
                metadata,
            } => {
                let kickoff = match metadata {
                    Some(metadata) => format!(
                        "Gateway event type: {}\nMessage: {}\nMetadata: {}",
                        event_type, message, metadata
                    ),
                    None => format!("Gateway event type: {}\nMessage: {}", event_type, message),
                };
                (
                    "event.ingest",
                    target_agent_id.clone(),
                    source_agent_id.clone(),
                    kickoff,
                    Some(event_type.clone()),
                    Some(event_type.clone()),
                    metadata.clone(),
                )
            }
        };

        let causal_logger = match init_gateway_causal_logger(self.config.as_ref()) {
            Ok(logger) => logger,
            Err(e) => {
                return Err((
                    format!("{} failed: unable to initialize gateway causal logger: {}", action_name, e),
                    None,
                ));
            }
        };

        let mut trace_session = TraceSession::create_with_session_id(
            SessionId::from_string(session_id.clone()),
            Arc::new(causal_logger),
            gateway_actor_id(),
            EventScope::Request,
        );

        let requested_data = match (&ingress, &metadata_for_trace) {
            (
                IngressType::Spawn {
                    agent_id,
                    source_agent_id,
                    message,
                    metadata,
                },
                _,
            ) => serde_json::json!({
                "agent_id": agent_id,
                "source_agent_id": source_agent_id,
                "session_id": session_id,
                "message_len": message.len(),
                "message_sha256": sha256_hex(message),
                "metadata_sha256": metadata.as_ref().map(|v| sha256_hex(&v.to_string())),
            }),
            (IngressType::Ingest { target_agent_id, source_agent_id, event_type, message, metadata }, _) => serde_json::json!({
                "event_type": event_type,
                "target_agent_id": target_agent_id,
                "source_agent_id": source_agent_id,
                "session_id": session_id,
                "message_len": message.len(),
                "message_sha256": sha256_hex(message),
                "metadata_sha256": metadata.as_ref().and_then(|v| serde_json::to_string(v).ok()).as_ref().map(|v| sha256_hex(v)),
            }),
        };

        let _ = trace_session.log_requested(action_name, Some(requested_data));

        let result = match self
            .spawn_agent_once(
                &agent_id,
                &message,
                &session_id,
                source_agent_id.as_deref(),
                false,
                event_type_for_inbox.as_deref(),
                metadata_for_trace.as_ref(),
            )
            .await
        {
            Ok(r) => r,
            Err(e) => {
                return Err((e.to_string(), Some(trace_session)));
            }
        };

        // Background signal already sent in execution layer if needed (result.should_signal_background)

        let completed_data = match (&ingress, &event_type_str) {
            (IngressType::Spawn { source_agent_id, metadata, .. }, _) => serde_json::json!({
                "agent_id": result.agent_id,
                "source_agent_id": source_agent_id,
                "assistant_reply_len": result.assistant_reply.as_ref().map(|s| s.len()).unwrap_or(0),
                "assistant_reply_sha256": result.assistant_reply.as_ref().map(|s| sha256_hex(s)),
                "metadata_sha256": metadata.as_ref().map(|v| sha256_hex(&v.to_string())),
            }),
            (IngressType::Ingest { target_agent_id, source_agent_id, event_type, .. }, _) => serde_json::json!({
                "event_type": event_type,
                "target_agent_id": target_agent_id,
                "source_agent_id": source_agent_id,
                "assistant_reply_len": result.assistant_reply.as_ref().map(|s| s.len()).unwrap_or(0),
                "assistant_reply_sha256": result.assistant_reply.as_ref().map(|s| sha256_hex(s)),
            }),
        };

        let _ = trace_session.log_completed(action_name, None, Some(completed_data));

        Ok((result, None))
    }

    pub async fn dispatch(&self, req: JsonRpcRequest) -> JsonRpcResponse {
        tracing::debug!("Dispatching JSON-RPC method: {}", req.method);

        match req.method.as_str() {
            "ping" => JsonRpcResponse::success(req.id, serde_json::json!("pong")),
            "agent.spawn" => {
                let params: AgentSpawnParams = match serde_json::from_value(req.params) {
                    Ok(v) => v,
                    Err(e) => {
                        return JsonRpcResponse::error(
                            req.id,
                            -32602,
                            format!("Invalid params for agent.spawn: {}", e),
                        );
                    }
                };
                let session_id = params
                    .session_id
                    .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
                let agent_id = params.agent_id.clone();
                
                let ingress = IngressType::Spawn {
                    agent_id: params.agent_id.clone(),
                    source_agent_id: params.source_agent_id.clone(),
                    message: params.message.clone(),
                    metadata: params.metadata.clone(),
                };
                
                match self.execute_agent_request(ingress, session_id.clone()).await {
                    Ok((result, _trace_session)) => {
                        // Persist binding on successful spawn so future
                        // event.ingest to this session routes to the spawned
                        // agent (instead of default lead fallback).
                        if let Err(e) = persist_session_lead_agent_binding(
                            self.config.as_ref(),
                            &result.session_id,
                            &result.agent_id,
                        ) {
                            tracing::warn!(
                                error = %e,
                                session_id = %result.session_id,
                                agent_id = %result.agent_id,
                                "Failed to persist session lead binding after spawn"
                            );
                        }
                        if let Some(source_agent_id) = params.source_agent_id.as_deref() {
                            let _ = append_delegation_task_entry(
                                self.config.as_ref(),
                                source_agent_id,
                                &agent_id,
                                "agent.spawn",
                                TaskStatus::Completed,
                                Some(serde_json::json!({
                                    "session_id": result.session_id.clone(),
                                    "assistant_reply": result.assistant_reply.clone(),
                                    "artifacts": result.artifacts.clone(),
                                    "shared_knowledge": result.shared_knowledge.clone(),
                                    "delegation_metadata": params.metadata.clone(),
                                })),
                            );
                        }
                        JsonRpcResponse::success(
                            req.id,
                            serde_json::json!({
                                "agent_id": result.agent_id,
                                "session_id": result.session_id,
                                "assistant_reply": result.assistant_reply,
                                "artifacts": result.artifacts,
                                "shared_knowledge": result.shared_knowledge,
                            }),
                        )
                    }
                    Err((e, maybe_trace_session)) => {
                        if let Some(source_agent_id) = params.source_agent_id.as_deref() {
                            let _ = append_delegation_task_entry(
                                self.config.as_ref(),
                                source_agent_id,
                                &agent_id,
                                "agent.spawn",
                                TaskStatus::Failed,
                                Some(serde_json::json!({
                                    "error": e.clone(),
                                })),
                            );
                        }
                        if let Some(mut trace_session) = maybe_trace_session {
                            let _ = trace_session.log_failed(
                                "agent.spawn",
                                &e,
                                Some(serde_json::json!({
                                    "agent_id": agent_id.clone(),
                                    "source_agent_id": params.source_agent_id.clone(),
                                })),
                            );
                        }
                        JsonRpcResponse::error(req.id, -32000, format!("agent.spawn failed: {}", e))
                    }
                }
            }
            "event.ingest" => {
                let params: EventIngestParams = match serde_json::from_value(req.params) {
                    Ok(v) => v,
                    Err(e) => {
                        return JsonRpcResponse::error(
                            req.id,
                            -32602,
                            format!("Invalid params for event.ingest: {}", e),
                        );
                    }
                };
                let session_id = params
                    .session_id
                    .clone()
                    .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
                let event_type = params.event_type.clone();
                let target_agent_id = match self
                    .resolve_ingest_target_agent_id(&session_id, params.target_agent_id.as_deref())
                {
                    Ok(value) => value,
                    Err(e) => {
                        return JsonRpcResponse::error(
                            req.id,
                            -32000,
                            format!("event.ingest routing failed: {}", e),
                        );
                    }
                };
                
                let ingress = IngressType::Ingest {
                    target_agent_id: target_agent_id.clone(),
                    source_agent_id: params.source_agent_id.clone(),
                    event_type: params.event_type.clone(),
                    message: params.message.clone(),
                    metadata: params.metadata.clone(),
                };
                
                match self.execute_agent_request(ingress, session_id.clone()).await {
                    Ok((result, _trace_session)) => {
                        if let Some(source_agent_id) = params.source_agent_id.as_deref() {
                            let _ = append_delegation_task_entry(
                                self.config.as_ref(),
                                source_agent_id,
                                &target_agent_id,
                                "event.ingest",
                                TaskStatus::Completed,
                                Some(serde_json::json!({
                                    "session_id": result.session_id.clone(),
                                    "assistant_reply": result.assistant_reply.clone(),
                                    "artifacts": result.artifacts.clone(),
                                    "shared_knowledge": result.shared_knowledge.clone(),
                                    "event_type": event_type.clone(),
                                })),
                            );
                        }
                        JsonRpcResponse::success(
                            req.id,
                            serde_json::json!({
                                "event_type": event_type,
                                "target_agent_id": target_agent_id,
                                "session_id": result.session_id,
                                "assistant_reply": result.assistant_reply,
                                "artifacts": result.artifacts,
                                "shared_knowledge": result.shared_knowledge,
                            }),
                        )
                    }
                    Err((e, maybe_trace_session)) => {
                        if let Some(source_agent_id) = params.source_agent_id.as_deref() {
                            let _ = append_delegation_task_entry(
                                self.config.as_ref(),
                                source_agent_id,
                                &target_agent_id,
                                "event.ingest",
                                TaskStatus::Failed,
                                Some(serde_json::json!({
                                    "error": e.clone(),
                                    "event_type": event_type.clone(),
                                })),
                            );
                        }
                        if let Some(mut trace_session) = maybe_trace_session {
                            let _ = trace_session.log_failed(
                                "event.ingest",
                                &e,
                                Some(serde_json::json!({
                                    "event_type": event_type.clone(),
                                    "target_agent_id": target_agent_id.clone(),
                                    "source_agent_id": params.source_agent_id.clone(),
                                })),
                            );
                        }
                        JsonRpcResponse::error(
                            req.id,
                            -32000,
                            format!("event.ingest failed: {}", e),
                        )
                    }
                }
            }

            // Session fork - fork a session from a snapshot
            "session.fork" => {
                #[derive(Deserialize)]
                struct ForkParams {
                    source_session_id: String,
                    #[serde(default)]
                    branch_message: Option<String>,
                    #[serde(default)]
                    new_session_id: Option<String>,
                    #[serde(default)]
                    target_agent_id: Option<String>,
                }

                let params: ForkParams = match serde_json::from_value(req.params.clone()) {
                    Ok(p) => p,
                    Err(e) => {
                        return JsonRpcResponse::error(
                            req.id,
                            -32602,
                            format!("Invalid params for session.fork: {}", e),
                        );
                    }
                };

                let gateway_dir = self.config.agents_dir.join(".gateway");

                // Load snapshot from source session
                let snapshot = match crate::runtime::session_snapshot::SessionSnapshot::load_from_session(
                    &params.source_session_id,
                    &gateway_dir,
                ) {
                    Ok(s) => s,
                    Err(e) => {
                        return JsonRpcResponse::error(
                            req.id,
                            -32000,
                            format!("Failed to load snapshot from session '{}': {}", params.source_session_id, e),
                        );
                    }
                };

                // Fork the session
                let fork = match crate::runtime::session_snapshot::SessionFork::fork(
                    &snapshot,
                    params.new_session_id.as_deref(),
                    params.branch_message.as_deref(),
                    &gateway_dir,
                ) {
                    Ok(f) => f,
                    Err(e) => {
                        return JsonRpcResponse::error(
                            req.id,
                            -32000,
                            format!("Failed to fork session: {}", e),
                        );
                    }
                };

                // Determine target agent
                let target_agent_id = params
                    .target_agent_id
                    .unwrap_or_else(|| params.source_session_id.clone());

                // Log fork in causal chain (best effort, don't fail fork on logging error)
                let causal_logger_result =
                    crate::execution::init_gateway_causal_logger(&self.config);
                if let Ok(causal_logger) = causal_logger_result {
                    let branch_message_sha256 = params.branch_message.as_ref().map(|m| {
                        use sha2::{Sha256, Digest};
                        let mut hasher = Sha256::new();
                        hasher.update(m.as_bytes());
                        format!("{:x}", hasher.finalize())
                    });
                    let _ = crate::execution::log_gateway_causal_event(
                        &causal_logger,
                        &target_agent_id,
                        &fork.new_session_id,
                        1,
                        "session.forked",
                        autonoetic_types::causal_chain::EntryStatus::Success,
                        Some(serde_json::json!({
                            "source_session_id": params.source_session_id,
                            "fork_turn": fork.fork_turn,
                            "history_handle": fork.history_handle,
                            "branch_message_sha256": branch_message_sha256,
                        })),
                    );
                }

                JsonRpcResponse::success(
                    req.id,
                    serde_json::json!({
                        "new_session_id": fork.new_session_id,
                        "source_session_id": fork.source_session_id,
                        "fork_turn": fork.fork_turn,
                        "history_handle": fork.history_handle,
                        "message_count": fork.initial_history.len(),
                    }),
                )
            }

            _ => JsonRpcResponse::error(req.id, -32601, "Method not found"),
        }
    }

    pub async fn spawn_agent_once(
        &self,
        agent_id: &str,
        message: &str,
        session_id: &str,
        source_agent_id: Option<&str>,
        is_message: bool,
        ingest_event_type: Option<&str>,
        metadata: Option<&serde_json::Value>,
    ) -> anyhow::Result<SpawnResult> {
        self.execution
            .spawn_agent_once(
                agent_id,
                message,
                session_id,
                source_agent_id,
                is_message,
                ingest_event_type,
                metadata,
            )
            .await
    }

    fn resolve_ingest_target_agent_id(
        &self,
        session_id: &str,
        requested_target_agent_id: Option<&str>,
    ) -> anyhow::Result<String> {
        if let Some(explicit_target) = requested_target_agent_id.map(str::trim) {
            anyhow::ensure!(
                !explicit_target.is_empty(),
                "target_agent_id must not be empty when provided"
            );
            persist_session_lead_agent_binding(self.config.as_ref(), session_id, explicit_target)?;
            return Ok(explicit_target.to_string());
        }

        if let Some(bound_agent_id) = load_session_lead_agent_binding(self.config.as_ref(), session_id)?
        {
            return Ok(bound_agent_id);
        }

        let default_lead = self.config.default_lead_agent_id.trim();
        anyhow::ensure!(
            !default_lead.is_empty(),
            "default_lead_agent_id is empty; configure a lead agent or provide target_agent_id"
        );
        persist_session_lead_agent_binding(self.config.as_ref(), session_id, default_lead)?;
        Ok(default_lead.to_string())
    }

    #[cfg(test)]
    async fn execute_with_reliability_controls<F, Fut, T>(
        &self,
        agent_id: &str,
        operation: F,
    ) -> anyhow::Result<T>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = anyhow::Result<T>>,
    {
        self.execution
            .execute_with_reliability_controls(agent_id, operation)
            .await
    }

    #[cfg(test)]
    async fn agent_admission_semaphore(&self, agent_id: &str) -> Arc<tokio::sync::Semaphore> {
        self.execution.agent_admission_semaphore(agent_id).await
    }

    #[cfg(test)]
    fn execution_semaphore(&self) -> Arc<tokio::sync::Semaphore> {
        self.execution.execution_semaphore()
    }

  }

#[derive(Debug, Deserialize)]
struct AgentSpawnParams {
    agent_id: String,
    message: String,
    #[serde(default)]
    metadata: Option<serde_json::Value>,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    source_agent_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct EventIngestParams {
    event_type: String,
    #[serde(default)]
    target_agent_id: Option<String>,
    message: String,
    #[serde(default)]
    metadata: Option<serde_json::Value>,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    source_agent_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct SessionLeadBinding {
    session_id: String,
    lead_agent_id: String,
    updated_at: String,
}

fn session_lead_bindings_dir(config: &GatewayConfig) -> PathBuf {
    gateway_root_dir(config).join("sessions").join("lead_bindings")
}

fn session_lead_binding_path(config: &GatewayConfig, session_id: &str) -> PathBuf {
    session_lead_bindings_dir(config).join(format!("{}.json", session_filename_token(session_id)))
}

fn session_filename_token(session_id: &str) -> String {
    let sanitized = session_id
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    let prefix = if sanitized.is_empty() {
        "session".to_string()
    } else {
        sanitized.chars().take(48).collect::<String>()
    };
    let mut hasher = Sha256::new();
    hasher.update(session_id.as_bytes());
    let digest = format!("{:x}", hasher.finalize());
    format!("{}-{}", prefix, &digest[..12])
}

fn load_session_lead_agent_binding(
    config: &GatewayConfig,
    session_id: &str,
) -> anyhow::Result<Option<String>> {
    let path = session_lead_binding_path(config, session_id);
    if !path.exists() {
        return Ok(None);
    }
    let body = std::fs::read_to_string(path)?;
    let binding: SessionLeadBinding = serde_json::from_str(&body)?;
    anyhow::ensure!(
        !binding.lead_agent_id.trim().is_empty(),
        "Session lead binding for '{}' contains an empty lead_agent_id",
        session_id
    );
    Ok(Some(binding.lead_agent_id))
}

fn persist_session_lead_agent_binding(
    config: &GatewayConfig,
    session_id: &str,
    lead_agent_id: &str,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        !lead_agent_id.trim().is_empty(),
        "lead_agent_id must not be empty"
    );
    let path = session_lead_binding_path(config, session_id);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let binding = SessionLeadBinding {
        session_id: session_id.to_string(),
        lead_agent_id: lead_agent_id.to_string(),
        updated_at: Utc::now().to_rfc3339(),
    };
    std::fs::write(path, serde_json::to_string_pretty(&binding)?)?;
    Ok(())
}

pub(crate) fn ingress_wake_signal_internal(event_type: &str, session_id: &str) -> String {
    serde_json::json!({
        "kind": "event_ingest",
        "event_type": event_type,
        "session_id": session_id,
    })
    .to_string()
}

fn append_delegation_task_entry(
    config: &GatewayConfig,
    source_agent_id: &str,
    creator_id: &str,
    action: &str,
    status: TaskStatus,
    result: Option<serde_json::Value>,
) -> anyhow::Result<()> {
    append_task_board_entry(
        config,
        &TaskBoardEntry {
            task_id: uuid::Uuid::new_v4().to_string(),
            creator_id: creator_id.to_string(),
            title: format!("{action} result from {creator_id}"),
            description: format!("Delegated {action} for source agent '{source_agent_id}'"),
            status,
            assignee_id: Some(source_agent_id.to_string()),
            created_at: chrono::Utc::now().to_rfc3339(),
            capabilities_required: vec![],
            result,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution::{init_gateway_causal_logger, log_gateway_causal_event};
    use crate::scheduler::{inbox_path, task_board_path, InboxEvent};
    use autonoetic_types::task_board::TaskBoardEntry;
    use tempfile::TempDir;

    fn test_router() -> (TempDir, JsonRpcRouter) {
        let temp = tempfile::tempdir().expect("tempdir should create");
        let router = JsonRpcRouter::new(GatewayConfig {
            agents_dir: temp.path().join("agents"),
            ..GatewayConfig::default()
        });
        (temp, router)
    }

    fn write_minimal_agent(agents_dir: &std::path::Path, agent_id: &str) {
        let agent_dir = agents_dir.join(agent_id);
        std::fs::create_dir_all(&agent_dir).unwrap();
        std::fs::write(
            agent_dir.join("SKILL.md"),
            format!(
                "---\nversion: \"1.0\"\nruntime:\n  engine: \"autonoetic\"\n  gateway_version: \"0.1.0\"\n  sdk_version: \"0.1.0\"\n  type: \"stateful\"\n  sandbox: \"bubblewrap\"\n  runtime_lock: \"runtime.lock\"\nagent:\n  id: \"{agent_id}\"\n  name: \"{agent_id}\"\n  description: \"test\"\n---\nbody\n"
            ),
        )
        .unwrap();
    }

    fn write_background_agent(agents_dir: &std::path::Path, agent_id: &str, new_messages: bool) {
        let agent_dir = agents_dir.join(agent_id);
        std::fs::create_dir_all(&agent_dir).unwrap();
        std::fs::write(
            agent_dir.join("SKILL.md"),
            format!(
                "---\nversion: \"1.0\"\nruntime:\n  engine: \"autonoetic\"\n  gateway_version: \"0.1.0\"\n  sdk_version: \"0.1.0\"\n  type: \"stateful\"\n  sandbox: \"bubblewrap\"\n  runtime_lock: \"runtime.lock\"\nagent:\n  id: \"{agent_id}\"\n  name: \"{agent_id}\"\n  description: \"test\"\nbackground:\n  enabled: true\n  interval_secs: 60\n  mode: deterministic\n  wake_predicates:\n    timer: false\n    new_messages: {new_messages}\n---\nbody\n"
            ),
        )
        .unwrap();
    }

    #[tokio::test]
    async fn test_dispatch_ping() {
        let (_temp, router) = test_router();
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: "1".to_string(),
            method: "ping".to_string(),
            params: serde_json::json!({}),
        };
        let resp = router.dispatch(req).await;
        assert_eq!(resp.result, Some(serde_json::json!("pong")));
        assert!(resp.error.is_none());
    }

    #[tokio::test]
    async fn test_dispatch_event_ingest_invalid_params() {
        let (_temp, router) = test_router();
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: "2".to_string(),
            method: "event.ingest".to_string(),
            params: serde_json::json!({
                "event_type": "webhook",
                "target_agent_id": "agent_a"
            }),
        };
        let resp = router.dispatch(req).await;
        assert_eq!(resp.error.as_ref().map(|e| e.code), Some(-32602));
    }

    #[tokio::test]
    async fn test_dispatch_event_ingest_without_target_routes_to_default_lead_agent() {
        let (_temp, router) = test_router();
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: "2b".to_string(),
            method: "event.ingest".to_string(),
            params: serde_json::json!({
                "event_type": "chat",
                "message": "hello planner",
                "session_id": "sess-default-route"
            }),
        };
        let resp = router.dispatch(req).await;
        let err = resp.error.expect("event.ingest should fail on missing agent");
        assert_eq!(err.code, -32000);
        assert!(err.message.contains("planner.default"));
        assert!(err.message.contains("not found"));
    }

    #[tokio::test]
    async fn test_dispatch_event_ingest_uses_existing_session_lead_binding() {
        let (_temp, router) = test_router();
        let first = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: "2c-1".to_string(),
            method: "event.ingest".to_string(),
            params: serde_json::json!({
                "event_type": "chat",
                "target_agent_id": "researcher.default",
                "message": "hello researcher",
                "session_id": "sess-bound-route"
            }),
        };
        let first_resp = router.dispatch(first).await;
        let first_err = first_resp.error.expect("first event.ingest should fail on missing agent");
        assert_eq!(first_err.code, -32000);
        assert!(first_err.message.contains("researcher.default"));

        let second = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: "2c-2".to_string(),
            method: "event.ingest".to_string(),
            params: serde_json::json!({
                "event_type": "chat",
                "message": "follow-up without explicit target",
                "session_id": "sess-bound-route"
            }),
        };
        let second_resp = router.dispatch(second).await;
        let second_err = second_resp
            .error
            .expect("second event.ingest should also fail on missing agent");
        assert_eq!(second_err.code, -32000);
        assert!(second_err.message.contains("researcher.default"));
        assert!(!second_err.message.contains("planner.default"));
    }

    #[tokio::test]
    async fn test_dispatch_agent_spawn_unauthorized() {
        let (temp, router) = test_router();
        let agents_dir = temp.path().join("agents");

        // Create source agent without AgentSpawn capability
        let source_dir = agents_dir.join("source");
        std::fs::create_dir_all(&source_dir).unwrap();
        std::fs::write(
            source_dir.join("SKILL.md"),
            "---\nname: source\ndescription: test\ncapabilities:\n  - type: ReadAccess\n    scopes: ['*']\n---\nbody\n",
        ).unwrap();

        // Create target agent
        let target_dir = agents_dir.join("target");
        std::fs::create_dir_all(&target_dir).unwrap();
        std::fs::write(
            target_dir.join("SKILL.md"),
            "---\nname: target\ndescription: test\n---\nbody\n",
        )
        .unwrap();

        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: "3".to_string(),
            method: "agent.spawn".to_string(),
            params: serde_json::json!({
                "agent_id": "target",
                "message": "hello",
                "source_agent_id": "source"
            }),
        };

        let resp = router.dispatch(req).await;
        let err = resp.error.expect("Expected an error");
        assert_eq!(err.code, -32000);
        assert!(
            err.message.contains("Permission Denied"),
            "Message was: {}",
            err.message
        );
        assert!(
            err.message.contains("AgentSpawn"),
            "Message was: {}",
            err.message
        );
    }

    #[tokio::test]
    async fn test_dispatch_agent_spawn_unknown_agent() {
        let (temp, router) = test_router();
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: "3".to_string(),
            method: "agent.spawn".to_string(),
            params: serde_json::json!({
                "agent_id": "missing",
                "message": "hello"
            }),
        };
        let resp = router.dispatch(req).await;
        assert_eq!(resp.error.as_ref().map(|e| e.code), Some(-32000));
        assert!(resp
            .error
            .as_ref()
            .expect("error should exist")
            .message
            .contains("not found"));
        let gateway_log_path = temp
            .path()
            .join("agents")
            .join(".gateway")
            .join("history")
            .join("causal_chain.jsonl");
        let body =
            std::fs::read_to_string(gateway_log_path).expect("gateway causal chain should exist");
        assert!(body.contains("\"action\":\"agent.spawn.requested\""));
        assert!(body.contains("\"action\":\"agent.spawn.failed\""));
    }

    #[tokio::test]
    async fn test_dispatch_agent_spawn_accepts_metadata_param() {
        let (_temp, router) = test_router();
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: "3b".to_string(),
            method: "agent.spawn".to_string(),
            params: serde_json::json!({
                "agent_id": "missing",
                "message": "hello",
                "metadata": {
                    "delegated_role": "researcher",
                    "delegation_reason": "need evidence",
                    "expected_outputs": ["summary.md", "sources.json"]
                }
            }),
        };
        let resp = router.dispatch(req).await;
        assert_eq!(resp.error.as_ref().map(|e| e.code), Some(-32000));
        assert!(resp
            .error
            .as_ref()
            .expect("error should exist")
            .message
            .contains("not found"));
    }

    #[tokio::test]
    #[allow(deprecated)]
    async fn test_dispatch_agent_spawn_enforces_max_children_per_session() {
        let (temp, router) = test_router();
        let agents_dir = temp.path().join("agents");

        let source_dir = agents_dir.join("source");
        std::fs::create_dir_all(&source_dir).unwrap();
        std::fs::write(
            source_dir.join("SKILL.md"),
            "---\nversion: \"1.0\"\nruntime:\n  engine: \"autonoetic\"\n  gateway_version: \"0.1.0\"\n  sdk_version: \"0.1.0\"\n  type: \"stateful\"\n  sandbox: \"bubblewrap\"\n  runtime_lock: \"runtime.lock\"\nagent:\n  id: \"source\"\n  name: \"source\"\n  description: \"test\"\ncapabilities:\n  - type: AgentSpawn\n    max_children: 1\n---\nbody\n",
        )
        .unwrap();

        let target_dir = agents_dir.join("target");
        std::fs::create_dir_all(&target_dir).unwrap();
        std::fs::write(
            target_dir.join("SKILL.md"),
            "---\nname: target\ndescription: test\n---\nbody\n",
        )
        .unwrap();

        let causal_logger = init_gateway_causal_logger(&GatewayConfig {
            agents_dir: agents_dir.clone(),
            ..GatewayConfig::default()
        })
        .unwrap();
        log_gateway_causal_event(
            &causal_logger,
            "gateway",
            "session-1",
            1,
            "agent.spawn.completed",
            EntryStatus::Success,
            Some(serde_json::json!({
                "agent_id": "target",
                "source_agent_id": "source",
                "assistant_reply_len": 0,
            })),
        );

        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: "4".to_string(),
            method: "agent.spawn".to_string(),
            params: serde_json::json!({
                "agent_id": "target",
                "message": "hello",
                "session_id": "session-1",
                "source_agent_id": "source"
            }),
        };

        let resp = router.dispatch(req).await;
        let err = resp.error.expect("Expected an error");
        assert_eq!(err.code, -32000);
        assert!(
            err.message.contains("exceeded AgentSpawn limit"),
            "Message was: {}",
            err.message
        );
    }

    #[tokio::test]
    #[allow(deprecated)]
    async fn test_dispatch_event_ingest_enforces_max_children_per_session() {
        let (temp, router) = test_router();
        let agents_dir = temp.path().join("agents");

        let source_dir = agents_dir.join("source");
        std::fs::create_dir_all(&source_dir).unwrap();
        std::fs::write(
            source_dir.join("SKILL.md"),
            "---\nversion: \"1.0\"\nruntime:\n  engine: \"autonoetic\"\n  gateway_version: \"0.1.0\"\n  sdk_version: \"0.1.0\"\n  type: \"stateful\"\n  sandbox: \"bubblewrap\"\n  runtime_lock: \"runtime.lock\"\nagent:\n  id: \"source\"\n  name: \"source\"\n  description: \"test\"\ncapabilities:\n  - type: AgentSpawn\n    max_children: 1\n---\nbody\n",
        )
        .unwrap();

        let target_dir = agents_dir.join("target");
        std::fs::create_dir_all(&target_dir).unwrap();
        std::fs::write(
            target_dir.join("SKILL.md"),
            "---\nname: target\ndescription: test\n---\nbody\n",
        )
        .unwrap();

        let causal_logger = init_gateway_causal_logger(&GatewayConfig {
            agents_dir: agents_dir.clone(),
            ..GatewayConfig::default()
        })
        .unwrap();
        log_gateway_causal_event(
            &causal_logger,
            "gateway",
            "session-2",
            1,
            "event.ingest.completed",
            EntryStatus::Success,
            Some(serde_json::json!({
                "event_type": "webhook",
                "target_agent_id": "target",
                "source_agent_id": "source",
                "assistant_reply_len": 0,
            })),
        );

        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: "5".to_string(),
            method: "event.ingest".to_string(),
            params: serde_json::json!({
                "event_type": "webhook",
                "target_agent_id": "target",
                "message": "hello",
                "session_id": "session-2",
                "source_agent_id": "source"
            }),
        };

        let resp = router.dispatch(req).await;
        let err = resp.error.expect("Expected an error");
        assert_eq!(err.code, -32000);
        assert!(
            err.message.contains("exceeded AgentSpawn limit"),
            "Message was: {}",
            err.message
        );
    }

    #[tokio::test]
    async fn test_dispatch_event_ingest_writes_scheduler_inbox_signal_for_background_agents() {
        let (temp, router) = test_router();
        let agents_dir = temp.path().join("agents");
        write_background_agent(&agents_dir, "target", true);

        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: "6".to_string(),
            method: "event.ingest".to_string(),
            params: serde_json::json!({
                "event_type": "webhook",
                "target_agent_id": "target",
                "message": "deploy",
                "session_id": "session-inbox"
            }),
        };

        let resp = router.dispatch(req).await;
        assert_eq!(resp.error.as_ref().map(|e| e.code), Some(-32000));

        let body = std::fs::read_to_string(inbox_path(router.config.as_ref(), "target"))
            .expect("inbox should exist");
        let event: InboxEvent =
            serde_json::from_str(body.trim()).expect("inbox event should parse");
        assert!(event.message.contains("\"kind\":\"event_ingest\""));
        assert!(event.message.contains("\"event_type\":\"webhook\""));
        assert!(event.message.contains("\"session_id\":\"session-inbox\""));
        assert!(!event.message.contains("deploy"));
        assert!(!event.message.contains("Gateway event type"));
    }

    #[tokio::test]
    async fn test_dispatch_event_ingest_skips_scheduler_inbox_without_new_message_opt_in() {
        let (temp, router) = test_router();
        let agents_dir = temp.path().join("agents");
        write_minimal_agent(&agents_dir, "target-no-bg");
        write_background_agent(&agents_dir, "target-no-new-messages", false);

        for agent_id in ["target-no-bg", "target-no-new-messages"] {
            let req = JsonRpcRequest {
                jsonrpc: "2.0".to_string(),
                id: agent_id.to_string(),
                method: "event.ingest".to_string(),
                params: serde_json::json!({
                    "event_type": "webhook",
                    "target_agent_id": agent_id,
                    "message": "deploy",
                    "session_id": "session-inbox"
                }),
            };

            let resp = router.dispatch(req).await;
            assert_eq!(resp.error.as_ref().map(|e| e.code), Some(-32000));
            assert!(
                !inbox_path(router.config.as_ref(), agent_id).exists(),
                "unexpected inbox signal for {agent_id}"
            );
        }
    }

    #[tokio::test]
    async fn test_dispatch_agent_spawn_failure_writes_task_board_entry_for_source() {
        let (temp, router) = test_router();
        let agents_dir = temp.path().join("agents");

        let source_dir = agents_dir.join("source");
        std::fs::create_dir_all(&source_dir).unwrap();
        std::fs::write(
            source_dir.join("SKILL.md"),
            "---\nversion: \"1.0\"\nruntime:\n  engine: \"autonoetic\"\n  gateway_version: \"0.1.0\"\n  sdk_version: \"0.1.0\"\n  type: \"stateful\"\n  sandbox: \"bubblewrap\"\n  runtime_lock: \"runtime.lock\"\nagent:\n  id: \"source\"\n  name: \"source\"\n  description: \"test\"\ncapabilities:\n  - type: AgentSpawn\n    max_children: 3\n---\nbody\n",
        )
        .unwrap();
        write_minimal_agent(&agents_dir, "target");

        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: "7".to_string(),
            method: "agent.spawn".to_string(),
            params: serde_json::json!({
                "agent_id": "target",
                "message": "hello",
                "session_id": "session-task",
                "source_agent_id": "source"
            }),
        };

        let resp = router.dispatch(req).await;
        assert_eq!(resp.error.as_ref().map(|e| e.code), Some(-32000));

        let body = std::fs::read_to_string(task_board_path(router.config.as_ref()))
            .expect("task board should exist");
        let entry: TaskBoardEntry =
            serde_json::from_str(body.lines().next().expect("task board entry should exist"))
                .expect("task board entry should decode");
        assert_eq!(entry.assignee_id.as_deref(), Some("source"));
        assert!(matches!(entry.status, TaskStatus::Failed));
        assert_eq!(entry.creator_id, "target");
    }

    #[tokio::test]
    async fn test_reliability_controls_reject_agent_queue_overflow() {
        let temp = tempfile::tempdir().expect("tempdir should create");
        let router = JsonRpcRouter::new(GatewayConfig {
            agents_dir: temp.path().join("agents"),
            max_concurrent_spawns: 2,
            max_pending_spawns_per_agent: 2,
            ..GatewayConfig::default()
        });

        let admission = router.agent_admission_semaphore("agent-a").await;
        let _permit1 = admission
            .clone()
            .acquire_owned()
            .await
            .expect("first permit should be acquired");
        let _permit2 = admission
            .clone()
            .acquire_owned()
            .await
            .expect("second permit should be acquired");

        let third = router
            .execute_with_reliability_controls("agent-a", || async { Ok::<_, anyhow::Error>(()) })
            .await;
        assert!(third.is_err());
        assert!(third
            .err()
            .expect("queue overflow should error")
            .to_string()
            .contains("pending execution queue is full"));
    }

    #[tokio::test]
    async fn test_reliability_controls_reject_global_execution_overflow() {
        let temp = tempfile::tempdir().expect("tempdir should create");
        let router = JsonRpcRouter::new(GatewayConfig {
            agents_dir: temp.path().join("agents"),
            max_concurrent_spawns: 1,
            max_pending_spawns_per_agent: 2,
            ..GatewayConfig::default()
        });

        let _permit = router
            .execution_semaphore()
            .acquire_owned()
            .await
            .expect("global execution permit should be acquired");

        let second = router
            .execute_with_reliability_controls("agent-b", || async { Ok::<_, anyhow::Error>(()) })
            .await;
        assert!(second.is_err());
        assert!(second
            .err()
            .expect("global overflow should error")
            .to_string()
            .contains("max concurrent executions reached"));
    }
}
