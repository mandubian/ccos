//! Thin per-session continuity stored alongside agent state.

use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

const MAX_MESSAGE_CHARS: usize = 280;
const MAX_FACT_LABEL_CHARS: usize = 80;
const MAX_FACT_VALUE_CHARS: usize = 160;
const MAX_THREAD_CHARS: usize = 160;
const MAX_KNOWN_FACTS: usize = 6;
const MAX_OPEN_THREADS: usize = 6;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionFact {
    pub label: String,
    pub value: String,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionContext {
    pub session_id: String,
    pub updated_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_user_message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_assistant_reply: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub known_facts: Vec<SessionFact>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub open_threads: Vec<String>,
}

impl SessionContext {
    pub fn empty(session_id: impl Into<String>) -> Self {
        Self {
            session_id: session_id.into(),
            updated_at: Utc::now().to_rfc3339(),
            last_user_message: None,
            last_assistant_reply: None,
            known_facts: Vec::new(),
            open_threads: Vec::new(),
        }
    }

    pub fn load(agent_dir: &Path, session_id: &str) -> anyhow::Result<Self> {
        let path = session_context_path(agent_dir, session_id);
        if !path.exists() {
            return Ok(Self::empty(session_id));
        }
        let body = std::fs::read_to_string(path)?;
        let mut context: Self = serde_json::from_str(&body)?;
        context.session_id = session_id.to_string();
        context.prune();
        Ok(context)
    }

    pub fn save(&mut self, agent_dir: &Path) -> anyhow::Result<()> {
        self.prune();
        self.updated_at = Utc::now().to_rfc3339();
        let path = session_context_path(agent_dir, &self.session_id);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, serde_json::to_string_pretty(self)?)?;
        Ok(())
    }

    pub fn record_turn(&mut self, user_message: &str, assistant_reply: Option<&str>) {
        self.last_user_message = normalize_optional(user_message, MAX_MESSAGE_CHARS);
        self.last_assistant_reply =
            assistant_reply.and_then(|value| normalize_optional(value, MAX_MESSAGE_CHARS));
        self.prune();
    }

    pub fn render_prompt(&self) -> Option<String> {
        let mut lines = vec!["Session context from prior turns:".to_string()];

        if let Some(last_user_message) = &self.last_user_message {
            lines.push(format!("- Last user message: {}", last_user_message));
        }
        if let Some(last_assistant_reply) = &self.last_assistant_reply {
            lines.push(format!("- Last assistant reply: {}", last_assistant_reply));
        }
        if !self.known_facts.is_empty() {
            lines.push("- Known facts:".to_string());
            for fact in &self.known_facts {
                lines.push(format!(
                    "  - {}: {} ({})",
                    fact.label, fact.value, fact.source
                ));
            }
        }
        if !self.open_threads.is_empty() {
            lines.push("- Open threads:".to_string());
            for item in &self.open_threads {
                lines.push(format!("  - {}", item));
            }
        }

        if lines.len() == 1 {
            return None;
        }

        lines.push(
            "Use this only as continuity context for the same session; prefer explicit durable memory reads for older or broader facts."
                .to_string(),
        );
        Some(lines.join("\n"))
    }

    fn prune(&mut self) {
        self.last_user_message = self
            .last_user_message
            .as_deref()
            .and_then(|value| normalize_optional(value, MAX_MESSAGE_CHARS));
        self.last_assistant_reply = self
            .last_assistant_reply
            .as_deref()
            .and_then(|value| normalize_optional(value, MAX_MESSAGE_CHARS));

        for fact in &mut self.known_facts {
            fact.label = truncate_chars(&fact.label, MAX_FACT_LABEL_CHARS);
            fact.value = truncate_chars(&fact.value, MAX_FACT_VALUE_CHARS);
            fact.source = truncate_chars(&fact.source, 32);
        }
        self.known_facts.truncate(MAX_KNOWN_FACTS);

        for item in &mut self.open_threads {
            *item = truncate_chars(item, MAX_THREAD_CHARS);
        }
        self.open_threads.truncate(MAX_OPEN_THREADS);
    }
}

pub fn session_context_path(agent_dir: &Path, session_id: &str) -> PathBuf {
    agent_dir
        .join("state")
        .join("sessions")
        .join(format!("{}.json", filename_token(session_id)))
}

fn filename_token(session_id: &str) -> String {
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
        truncate_chars(&sanitized, 48)
    };
    let mut hasher = Sha256::new();
    hasher.update(session_id.as_bytes());
    let digest = format!("{:x}", hasher.finalize());
    format!("{}-{}", prefix, &digest[..12])
}

fn normalize_optional(value: &str, max_chars: usize) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(truncate_chars(trimmed, max_chars))
    }
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let truncated = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_some() && max_chars >= 3 {
        let mut shortened = truncated;
        shortened.truncate(shortened.len().saturating_sub(3));
        shortened.push_str("...");
        shortened
    } else {
        truncated
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_context_round_trip() {
        let temp = tempfile::tempdir().expect("tempdir should create");
        let mut context = SessionContext::empty("terminal/session");
        context.record_turn("hello", Some("hi there"));
        context.known_facts.push(SessionFact {
            label: "project".to_string(),
            value: "Atlas".to_string(),
            source: "user_stated".to_string(),
        });
        context
            .save(temp.path())
            .expect("session context should save");

        let loaded = SessionContext::load(temp.path(), "terminal/session")
            .expect("session context should load");
        assert_eq!(loaded.last_user_message.as_deref(), Some("hello"));
        assert_eq!(loaded.last_assistant_reply.as_deref(), Some("hi there"));
        assert_eq!(loaded.known_facts.len(), 1);
    }

    #[test]
    fn test_session_context_prompt_renders_only_when_populated() {
        let empty = SessionContext::empty("session-1");
        assert!(empty.render_prompt().is_none());

        let mut populated = SessionContext::empty("session-1");
        populated.record_turn("remember this", Some("stored"));
        let prompt = populated
            .render_prompt()
            .expect("prompt should render once context exists");
        assert!(prompt.contains("Last user message: remember this"));
        assert!(prompt.contains("Last assistant reply: stored"));
    }

    #[test]
    fn test_session_context_path_is_stable_and_safe() {
        let path = session_context_path(Path::new("/tmp/agent"), "terminal:you/demo");
        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .expect("file name should be valid utf-8");
        assert!(name.ends_with(".json"));
        assert!(name.contains("terminal_you_demo"));
    }
}
