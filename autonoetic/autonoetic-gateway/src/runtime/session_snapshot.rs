//! Session Snapshot - persist and fork conversation history.
//!
//! Enables saving session history to content store and forking new sessions
//! from any snapshot, with lineage tracked in the causal chain.

use crate::llm::Message;
use crate::runtime::content_store::ContentStore;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// A snapshot of a session's conversation history and state.
///
/// Stored in the content-addressable storage for cross-session access.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSnapshot {
    /// Source session ID that was snapshotted.
    pub source_session_id: String,
    /// Number of turns at snapshot time.
    pub turn_count: usize,
    /// ISO 8601 timestamp.
    pub created_at: String,
    /// Full conversation history.
    pub history: Vec<Message>,
    /// Session context (known facts, open threads).
    pub session_context: Option<SessionContext>,
    /// Optional SDK checkpoint data.
    pub sdk_checkpoint: Option<serde_json::Value>,
    /// Content handle for the serialized snapshot.
    pub content_handle: Option<String>,
}

/// Session context - compact per-session continuity data.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SessionContext {
    /// Recent exchange summary.
    pub recent_exchange: Vec<ExchangeSummary>,
    /// Compact known facts.
    pub known_facts: Vec<String>,
    /// Open threads to continue.
    pub open_threads: Vec<String>,
    /// Last turn ID.
    pub last_turn_id: Option<String>,
}

/// Summary of a single exchange (user message + assistant response).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExchangeSummary {
    /// First ~200 chars of user message.
    pub user_summary: String,
    /// First ~200 chars of assistant response.
    pub assistant_summary: String,
    /// Turn ID for reference.
    pub turn_id: String,
}

impl SessionSnapshot {
    /// Captures a snapshot of the current session and stores it in content store.
    pub fn capture(
        session_id: &str,
        history: &[Message],
        turn_count: usize,
        session_context: Option<&SessionContext>,
        sdk_checkpoint: Option<&serde_json::Value>,
        gateway_dir: &Path,
    ) -> anyhow::Result<Self> {
        let store = ContentStore::new(gateway_dir)?;

        // Serialize history to JSON
        let history_json = serde_json::to_string_pretty(history)?;

        // Store in content store
        let handle = store.write(history_json.as_bytes())?;

        // Register with session name
        store.register_name(session_id, "session_snapshot", &handle)?;

        // Build snapshot
        let snapshot = SessionSnapshot {
            source_session_id: session_id.to_string(),
            turn_count,
            created_at: chrono::Utc::now().to_rfc3339(),
            history: history.to_vec(),
            session_context: session_context.cloned(),
            sdk_checkpoint: sdk_checkpoint.cloned(),
            content_handle: Some(handle.clone()),
        };

        // Store the full snapshot metadata as well
        let snapshot_json = serde_json::to_string(&snapshot)?;
        let snapshot_handle = store.write(snapshot_json.as_bytes())?;
        store.register_name(session_id, "session_snapshot_metadata", &snapshot_handle)?;

        Ok(snapshot)
    }

    /// Loads a snapshot from content store by handle.
    pub fn load(handle: &str, gateway_dir: &Path) -> anyhow::Result<Self> {
        let store = ContentStore::new(gateway_dir)?;
        let content = store.read(&handle.to_string())?;
        let snapshot: SessionSnapshot = serde_json::from_slice(&content)?;
        Ok(snapshot)
    }

    /// Loads a snapshot from a session's content store.
    pub fn load_from_session(session_id: &str, gateway_dir: &Path) -> anyhow::Result<Self> {
        let store = ContentStore::new(gateway_dir)?;
        let content = store.read_by_name(session_id, "session_snapshot_metadata")?;
        let snapshot: SessionSnapshot = serde_json::from_slice(&content)?;
        Ok(snapshot)
    }

    /// Marks the snapshot's content as persistent (survives cleanup).
    pub fn persist(&self, session_id: &str, gateway_dir: &Path) -> anyhow::Result<()> {
        let store = ContentStore::new(gateway_dir)?;

        if let Some(handle) = &self.content_handle {
            store.persist(session_id, handle)?;
        }

        Ok(())
    }

    /// Returns the content handle for this snapshot.
    pub fn handle(&self) -> Option<&str> {
        self.content_handle.as_deref()
    }

    /// Extracts the history from this snapshot.
    pub fn history(&self) -> &[Message] {
        &self.history
    }

    /// Returns turn count.
    pub fn turn_count(&self) -> usize {
        self.turn_count
    }
}

/// Fork a session from a snapshot.
pub struct SessionFork {
    /// New session ID.
    pub new_session_id: String,
    /// Source session ID.
    pub source_session_id: String,
    /// Fork turn number.
    pub fork_turn: usize,
    /// Content handle of the copied history.
    pub history_handle: String,
    /// Initial history for the forked session (including branch message if any).
    pub initial_history: Vec<Message>,
}

impl SessionFork {
    /// Creates a new session by forking from a snapshot.
    pub fn fork(
        snapshot: &SessionSnapshot,
        new_session_id: Option<&str>,
        branch_message: Option<&str>,
        gateway_dir: &Path,
    ) -> anyhow::Result<Self> {
        let store = ContentStore::new(gateway_dir)?;

        // Generate new session ID if not provided
        let new_session_id = new_session_id
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("fork-{}", &uuid::Uuid::new_v4().to_string()[..8]));

        // Build history from snapshot
        let mut history = snapshot.history.clone();

        // Add branch message if provided
        if let Some(msg_text) = branch_message {
            history.push(Message::user(msg_text));
        }

        // Copy history to new session
        let history_json = serde_json::to_string(&history)?;
        let history_handle = store.write(history_json.as_bytes())?;
        store.register_name(&new_session_id, "session_history", &history_handle)?;

        // Persist the history
        store.persist(&new_session_id, &history_handle)?;

        Ok(SessionFork {
            new_session_id,
            source_session_id: snapshot.source_session_id.clone(),
            fork_turn: snapshot.turn_count,
            history_handle,
            initial_history: history,
        })
    }

    /// Returns the initial history for the forked session.
    pub fn initial_history(&self) -> &[Message] {
        &self.initial_history
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::Message;
    use tempfile::tempdir;

    #[test]
    fn test_snapshot_capture_and_load() {
        let temp = tempdir().unwrap();
        let gateway_dir = temp.path().join(".gateway");
        std::fs::create_dir_all(&gateway_dir).unwrap();

        let history = vec![
            Message::user("Hello"),
            Message::assistant("Hi there!"),
            Message::user("What is 2+2?"),
            Message::assistant("4"),
        ];

        let snapshot =
            SessionSnapshot::capture("test-session", &history, 2, None, None, &gateway_dir)
                .unwrap();

        assert_eq!(snapshot.source_session_id, "test-session");
        assert_eq!(snapshot.turn_count, 2);
        assert_eq!(snapshot.history.len(), 4);
        assert!(snapshot.content_handle.is_some());

        // Load from session
        let loaded = SessionSnapshot::load_from_session("test-session", &gateway_dir).unwrap();
        assert_eq!(loaded.history.len(), 4);
        assert_eq!(loaded.turn_count, 2);
    }

    #[test]
    fn test_snapshot_persist() {
        let temp = tempdir().unwrap();
        let gateway_dir = temp.path().join(".gateway");
        std::fs::create_dir_all(&gateway_dir).unwrap();

        let history = vec![Message::user("Test")];

        let snapshot =
            SessionSnapshot::capture("test-session", &history, 1, None, None, &gateway_dir)
                .unwrap();

        snapshot.persist("test-session", &gateway_dir).unwrap();

        // Verify persisted
        let store = ContentStore::new(&gateway_dir).unwrap();
        let persisted = store.list_persisted("test-session").unwrap();
        assert!(!persisted.is_empty());
    }

    #[test]
    fn test_session_fork() {
        let temp = tempdir().unwrap();
        let gateway_dir = temp.path().join(".gateway");
        std::fs::create_dir_all(&gateway_dir).unwrap();

        let history = vec![Message::user("Hello"), Message::assistant("Hi!")];

        let snapshot =
            SessionSnapshot::capture("original-session", &history, 1, None, None, &gateway_dir)
                .unwrap();

        // Fork with branch message
        let fork = SessionFork::fork(
            &snapshot,
            Some("forked-session"),
            Some("Try a different approach"),
            &gateway_dir,
        )
        .unwrap();

        assert_eq!(fork.new_session_id, "forked-session");
        assert_eq!(fork.source_session_id, "original-session");
        assert_eq!(fork.fork_turn, 1);
        assert_eq!(fork.initial_history.len(), 3); // 2 original + 1 branch message
        assert_eq!(fork.initial_history[2].content, "Try a different approach");
    }
}
