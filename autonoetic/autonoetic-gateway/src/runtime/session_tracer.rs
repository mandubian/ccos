//! Session Tracer for Agent Execution.
//!
//! Owns session_id, event sequencing, causal logger access, and shared trace helpers.

use crate::causal_chain::CausalLogger;
use crate::log_redaction::redact_text_for_logs;
use crate::runtime::artifact::Artifact;
use autonoetic_types::causal_chain::EntryStatus;
use sha2::{Digest, Sha256};
use std::path::Path;

const EVIDENCE_MODE_ENV: &str = "AUTONOETIC_EVIDENCE_MODE";

/// Max characters for `result_preview` in causal_chain.jsonl tool_invoke entries.
/// Full tool results are stored in the evidence store when evidence mode is Full (see evidence_ref).
const TOOL_RESULT_PREVIEW_MAX_CHARS: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvidenceMode {
    Off,
    Full,
}

impl EvidenceMode {
    pub fn parse(value: &str) -> anyhow::Result<Self> {
        match value.to_ascii_lowercase().as_str() {
            "" | "off" => Ok(Self::Off),
            "full" => Ok(Self::Full),
            other => anyhow::bail!(
                "Invalid {}='{}'. Expected one of: off, full",
                EVIDENCE_MODE_ENV,
                other
            ),
        }
    }

    fn as_str(&self) -> &'static str {
        match self {
            EvidenceMode::Off => "off",
            EvidenceMode::Full => "full",
        }
    }
}

pub struct EvidenceStore {
    mode: EvidenceMode,
    agent_dir: std::path::PathBuf,
    base_dir: Option<std::path::PathBuf>,
}

impl EvidenceStore {
    pub fn from_env(agent_dir: &Path, session_id: &str) -> anyhow::Result<Self> {
        let raw = std::env::var(EVIDENCE_MODE_ENV).unwrap_or_else(|_| "off".to_string());
        let mode = EvidenceMode::parse(&raw)?;
        let base_dir = if mode == EvidenceMode::Full {
            let dir = agent_dir.join("history").join("evidence").join(session_id);
            std::fs::create_dir_all(&dir)?;
            Some(dir)
        } else {
            None
        };
        Ok(Self {
            mode,
            agent_dir: agent_dir.to_path_buf(),
            base_dir,
        })
    }

    pub fn capture_json(
        &self,
        turn_id: Option<&str>,
        category: &str,
        action: &str,
        payload: &serde_json::Value,
    ) -> anyhow::Result<Option<String>> {
        if self.mode != EvidenceMode::Full {
            return Ok(None);
        }
        let base = self
            .base_dir
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Evidence base directory is not initialized"))?;
        let file_name = format!(
            "{}-{}-{}-{}-{}.json",
            chrono::Utc::now().format("%Y%m%dT%H%M%S%.6fZ"),
            sanitize_token(turn_id.unwrap_or("session")),
            sanitize_token(category),
            sanitize_token(action),
            uuid::Uuid::new_v4()
        );
        let path = base.join(file_name);
        std::fs::write(&path, serde_json::to_string_pretty(payload)?)?;
        let rel = path.strip_prefix(&self.agent_dir).unwrap_or(&path);
        Ok(Some(rel.display().to_string()))
    }
}

pub struct SessionTracer {
    causal_logger: CausalLogger,
    agent_id: String,
    session_id: String,
    turn_id: Option<String>,
    event_seq: u64,
    evidence_store: EvidenceStore,
}

impl SessionTracer {
    pub fn new(agent_dir: &Path, agent_id: &str, session_id: &str) -> anyhow::Result<Self> {
        let causal_logger = init_causal_logger(agent_dir)?;
        let evidence_store = EvidenceStore::from_env(agent_dir, session_id)?;

        Ok(Self {
            causal_logger,
            agent_id: agent_id.to_string(),
            session_id: session_id.to_string(),
            turn_id: None,
            event_seq: 0,
            evidence_store,
        })
    }

    pub fn with_turn_id(mut self, turn_id: impl Into<String>) -> Self {
        self.turn_id = Some(turn_id.into());
        self
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub fn turn_id(&self) -> Option<&str> {
        self.turn_id.as_deref()
    }

    pub fn set_turn_id(&mut self, turn_id: impl Into<String>) {
        self.turn_id = Some(turn_id.into());
    }

    fn next_event_seq(&mut self) -> u64 {
        self.event_seq += 1;
        self.event_seq
    }

    pub fn log_event(
        &mut self,
        category: &str,
        action: &str,
        status: EntryStatus,
        payload: Option<serde_json::Value>,
    ) -> anyhow::Result<()> {
        let event_seq = self.next_event_seq();
        log_causal_event(
            &self.causal_logger,
            &self.agent_id,
            category,
            action,
            status,
            payload,
            &self.session_id,
            self.turn_id.as_deref(),
            event_seq,
        )
    }

    pub fn log_session_start(
        &mut self,
        trigger_type: &str,
        trigger: &str,
        evidence_mode: EvidenceMode,
    ) -> anyhow::Result<()> {
        let mut session_payload = serde_json::json!({
            "trigger_type": trigger_type,
            "trigger_len": trigger.len(),
            "trigger_sha256": sha256_hex(trigger),
            "trigger_preview": redact_text_for_logs(&truncate_for_log(trigger, 256)),
            "evidence_mode": evidence_mode.as_str(),
        });
        let session_evidence = serde_json::json!({
            "trigger": redact_text_for_logs(trigger)
        });
        if let Some(evidence_ref) =
            self.evidence_store
                .capture_json(None, "session", "start", &session_evidence)?
        {
            session_payload["evidence_ref"] = serde_json::json!(evidence_ref);
        }
        self.log_event(
            "session",
            "start",
            EntryStatus::Success,
            Some(session_payload),
        )?;
        Ok(())
    }

    pub fn log_session_end(&mut self, reason: &str) {
        let _ = self.log_event(
            "session",
            "end",
            EntryStatus::Success,
            Some(serde_json::json!({ "reason": reason })),
        );
    }

    pub fn log_wake(&mut self, history_messages: usize, evidence_mode: EvidenceMode) {
        let _ = self.log_event(
            "lifecycle",
            "wake",
            EntryStatus::Success,
            Some(serde_json::json!({
                "history_messages": history_messages,
                "evidence_mode": evidence_mode.as_str(),
            })),
        );
    }

    pub fn log_llm_completion(
        &mut self,
        model: &str,
        stop_reason: &str,
        text: &str,
        tool_calls: usize,
        input_tokens: u64,
        output_tokens: u64,
        tool_call_details: &[serde_json::Value],
    ) -> anyhow::Result<()> {
        let mut llm_payload = serde_json::json!({
            "model": model,
            "stop_reason": stop_reason,
            "text": redact_text_for_logs(&truncate_for_log(text, 256)),
            "text_sha256": sha256_hex(text),
            "tool_calls": tool_calls,
            "usage": {
                "input_tokens": input_tokens,
                "output_tokens": output_tokens
            }
        });
        let llm_evidence = serde_json::json!({
            "model": model,
            "stop_reason": stop_reason,
            "text": redact_text_for_logs(text),
            "tool_calls": tool_call_details,
            "usage": {
                "input_tokens": input_tokens,
                "output_tokens": output_tokens
            }
        });
        if let Some(evidence_ref) = self.evidence_store.capture_json(
            self.turn_id.as_deref(),
            "llm",
            "completion",
            &llm_evidence,
        )? {
            llm_payload["evidence_ref"] = serde_json::json!(evidence_ref);
        }
        self.log_event("llm", "completion", EntryStatus::Success, Some(llm_payload))?;
        Ok(())
    }

    pub fn log_tool_requested(&mut self, tool_name: &str, arguments: &str) -> anyhow::Result<()> {
        let redacted_args = redact_text_for_logs(arguments);
        let mut requested_payload = serde_json::json!({
            "tool_name": tool_name,
            "arguments": redacted_args,
            "arguments_sha256": sha256_hex(arguments)
        });
        let requested_evidence = serde_json::json!({
            "tool_name": tool_name,
            "arguments": redacted_args
        });
        if let Some(evidence_ref) = self.evidence_store.capture_json(
            self.turn_id.as_deref(),
            "tool_invoke",
            "requested",
            &requested_evidence,
        )? {
            requested_payload["evidence_ref"] = serde_json::json!(evidence_ref);
        }
        self.log_event(
            "tool_invoke",
            "requested",
            EntryStatus::Success,
            Some(requested_payload),
        )?;
        Ok(())
    }

    pub fn log_tool_completed(&mut self, tool_name: &str, result: &str) -> anyhow::Result<()> {
        let mut completed_payload = serde_json::json!({
            "tool_name": tool_name,
            "result_len": result.len(),
            "result_sha256": sha256_hex(result),
            "result_preview": redact_text_for_logs(&truncate_for_log(result, TOOL_RESULT_PREVIEW_MAX_CHARS))
        });
        let completed_evidence = serde_json::json!({
            "tool_name": tool_name,
            "result": redact_text_for_logs(result)
        });
        if let Some(evidence_ref) = self.evidence_store.capture_json(
            self.turn_id.as_deref(),
            "tool_invoke",
            "completed",
            &completed_evidence,
        )? {
            completed_payload["evidence_ref"] = serde_json::json!(evidence_ref);
        }
        self.log_event(
            "tool_invoke",
            "completed",
            EntryStatus::Success,
            Some(completed_payload),
        )?;
        Ok(())
    }

    pub fn log_artifact_detected(&mut self, artifact: &Artifact) -> anyhow::Result<()> {
        self.log_event(
            "artifact",
            "detected",
            EntryStatus::Success,
            Some(serde_json::to_value(artifact).unwrap_or(serde_json::json!({
                "type": artifact.artifact_type,
                "name": artifact.name
            }))),
        )
    }

    pub fn log_hibernate(&mut self, stop_reason: &str) {
        let _ = self.log_event(
            "lifecycle",
            "hibernate",
            EntryStatus::Success,
            Some(serde_json::json!({ "stop_reason": stop_reason })),
        );
    }

    pub fn log_stopped(&mut self, stop_reason: &str) {
        let _ = self.log_event(
            "lifecycle",
            "stopped",
            EntryStatus::Error,
            Some(serde_json::json!({ "stop_reason": stop_reason })),
        );
    }

    pub fn log_history_persisted(&mut self, message_count: usize, content_handle: &str) {
        let _ = self.log_event(
            "session",
            "history.persisted",
            EntryStatus::Success,
            Some(serde_json::json!({
                "message_count": message_count,
                "content_handle": content_handle
            })),
        );
    }

    pub fn log_session_forked(
        &mut self,
        source_session_id: &str,
        fork_turn: u64,
        history_handle: &str,
        branch_message: Option<&str>,
    ) {
        let payload = serde_json::json!({
            "source_session_id": source_session_id,
            "fork_turn": fork_turn,
            "history_handle": history_handle,
            "branch_message_sha256": branch_message.map(|m| {
                use sha2::{Sha256, Digest};
                let mut hasher = Sha256::new();
                hasher.update(m.as_bytes());
                format!("{:x}", hasher.finalize())
            })
        });
        let _ = self.log_event("session", "forked", EntryStatus::Success, Some(payload));
    }
}

fn init_causal_logger(agent_dir: &Path) -> anyhow::Result<CausalLogger> {
    let history_dir = agent_dir.join("history");
    std::fs::create_dir_all(&history_dir)?;
    CausalLogger::new(history_dir.join("causal_chain.jsonl"))
}

fn log_causal_event(
    logger: &CausalLogger,
    actor_id: &str,
    category: &str,
    action: &str,
    status: EntryStatus,
    payload: Option<serde_json::Value>,
    session_id: &str,
    turn_id: Option<&str>,
    event_seq: u64,
) -> anyhow::Result<()> {
    logger
        .log(
            actor_id, session_id, turn_id, event_seq, category, action, status, payload,
        )
        .map_err(|e| {
            anyhow::anyhow!(
                "Failed to append causal log entry for {}/{} in session {}: {}",
                category,
                action,
                session_id,
                e
            )
        })
}

fn truncate_for_log(value: &str, max_len: usize) -> String {
    if value.chars().count() <= max_len {
        return value.to_string();
    }
    let truncated: String = value.chars().take(max_len).collect();
    format!("{}...", truncated)
}

fn sanitize_token(value: &str) -> String {
    value
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn sha256_hex(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    let digest = hasher.finalize();
    format!("{:x}", digest)
}

#[cfg(test)]
impl SessionTracer {
    /// Creates a test tracer that discards all output.
    pub fn test_tracer() -> Self {
        Self {
            causal_logger: CausalLogger::test_logger("/dev/null"),
            agent_id: "test-agent".to_string(),
            session_id: "test-session".to_string(),
            turn_id: Some("test-turn".to_string()),
            event_seq: 0,
            evidence_store: EvidenceStore {
                mode: EvidenceMode::Off,
                agent_dir: std::path::PathBuf::from("/tmp"),
                base_dir: None,
            },
        }
    }
}
