//! Session Management for Gateway "Sheriff"
//!
//! Manages active sessions, authentication tokens, and session lifecycle.
//! The SessionRegistry is the authoritative source for session state.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio::sync::RwLock;
use uuid::Uuid;

// Note: In production, use secrecy::SecretString for tokens
// For simplicity, using plain String here
type SecretString = String;

/// Current state of a session
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionStatus {
    /// Session is active with agent running
    Active,
    /// Session exists but agent is not running (crashed or never spawned)
    AgentNotRunning,
    /// Session is idle (no recent activity)
    Idle,
    /// Session has been terminated
    Terminated,
}

/// State of an individual session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionState {
    /// Unique session identifier
    pub session_id: String,
    /// Authentication token (secret)
    pub auth_token: SecretString,
    /// Current session status
    pub status: SessionStatus,
    /// Process ID of the agent runtime (if local)
    pub agent_pid: Option<u32>,
    /// Current step the agent is executing (if any)
    pub current_step: Option<u32>,
    /// Memory usage in MB (last reported by agent)
    pub memory_mb: Option<u64>,
    /// When the session was created
    pub created_at: DateTime<Utc>,
    /// Last activity timestamp (updated on heartbeat)
    pub last_activity: DateTime<Utc>,
    /// Inbox for messages to be processed by agent
    pub inbox: Vec<ChatMessage>,
}

/// A chat message in the session inbox
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub id: String,
    pub channel_id: String,
    pub content: String,
    pub sender: String,
    pub timestamp: DateTime<Utc>,
    pub run_id: Option<String>,
}

impl SessionState {
    /// Create a new session with generated token
    pub fn new(session_id: String) -> Self {
        let token = format!("sess_{}", Uuid::new_v4().to_string().replace("-", ""));
        let now = Utc::now();

        Self {
            session_id,
            auth_token: token,
            status: SessionStatus::Active,
            agent_pid: None,
            current_step: None,
            memory_mb: None,
            created_at: now,
            last_activity: now,
            inbox: Vec::new(),
        }
    }

    /// Update last activity timestamp
    pub fn touch(&mut self) {
        self.last_activity = Utc::now();
    }

    /// Add a message to the inbox
    pub fn push_message(
        &mut self,
        channel_id: String,
        content: String,
        sender: String,
        run_id: Option<String>,
    ) {
        let msg = ChatMessage {
            id: format!(
                "msg_{}",
                Uuid::new_v4().to_string().split('-').next().unwrap()
            ),
            channel_id,
            content,
            sender,
            timestamp: Utc::now(),
            run_id,
        };
        self.inbox.push(msg);
        self.touch();
    }

    /// Get and clear the inbox (atomically retrieve messages)
    pub fn drain_inbox(&mut self) -> Vec<ChatMessage> {
        self.touch();
        std::mem::take(&mut self.inbox)
    }

    /// Check if session has an agent running (based on status and heartbeat)
    pub fn is_agent_running(&self) -> bool {
        matches!(self.status, SessionStatus::Active) && self.agent_pid.is_some()
    }

    /// Get a human-readable status description with icon
    pub fn status_with_icon(&self) -> &'static str {
        match self.status {
            SessionStatus::Active => "🟢 Active",
            SessionStatus::AgentNotRunning => "🔴 Agent Not Running",
            SessionStatus::Idle => "🟡 Idle",
            SessionStatus::Terminated => "⚫ Terminated",
        }
    }
}

/// Registry managing all active sessions
#[derive(Debug, Clone)]
pub struct SessionRegistry {
    sessions: Arc<RwLock<HashMap<String, SessionState>>>,
    /// Optional path for persisting session state to disk
    persist_path: Option<PathBuf>,
}

impl SessionRegistry {
    /// Create a new empty registry
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            persist_path: None,
        }
    }

    /// Derive the archive file path from the main persist path.
    /// e.g. `.ccos/sessions.json` → `.ccos/sessions_archive.jsonl`
    fn archive_path(persist_path: &Path) -> PathBuf {
        let stem = persist_path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy();
        let parent = persist_path.parent().unwrap_or(Path::new("."));
        parent.join(format!("{}_archive.jsonl", stem))
    }

    /// Create a registry that persists sessions to the given path.
    /// The path may be given as `sessions.json` (legacy) or `sessions.jsonl`.
    /// Internally, the live store always uses `.jsonl`; the `.json` path is only
    /// consulted on first load for backwards-compatibility migration.
    pub fn new_with_persistence(path: impl Into<PathBuf>) -> Self {
        let raw: PathBuf = path.into();
        // Normalise: always store as .jsonl
        let jsonl_path = if raw.extension().and_then(|e| e.to_str()) == Some("json") {
            raw.with_extension("jsonl")
        } else {
            raw
        };
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            persist_path: Some(jsonl_path),
        }
    }

    /// Append the current state of a single session as one JSONL line.
    /// This is the hot path — O(1) write regardless of total session count.
    async fn append_session_to_disk(&self, session_id: &str) {
        let Some(path) = &self.persist_path else {
            return;
        };
        let session = {
            let sessions = self.sessions.read().await;
            sessions.get(session_id).cloned()
        };
        let Some(session) = session else { return };
        let line = match serde_json::to_string(&session) {
            Ok(s) => s,
            Err(e) => {
                log::error!("[SessionRegistry] Failed to serialize session {}: {}", session_id, e);
                return;
            }
        };
        if let Some(parent) = path.parent() {
            let _ = tokio::fs::create_dir_all(parent).await;
        }
        match tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .await
        {
            Ok(mut f) => {
                let mut buf = line;
                buf.push('\n');
                if let Err(e) = f.write_all(buf.as_bytes()).await {
                    log::error!("[SessionRegistry] Failed to append session to {:?}: {}", path, e);
                }
            }
            Err(e) => log::error!("[SessionRegistry] Failed to open {:?} for append: {}", path, e),
        }
    }

    /// Compact-rewrite the JSONL: one line per live session (latest state).
    /// Called after archiving so the file never accumulates stale entries.
    pub async fn save_to_disk(&self) {
        let Some(path) = &self.persist_path else {
            return;
        };
        let sessions = self.sessions.read().await;
        let mut lines = String::new();
        for session in sessions.values() {
            match serde_json::to_string(session) {
                Ok(line) => {
                    lines.push_str(&line);
                    lines.push('\n');
                }
                Err(e) => log::error!("[SessionRegistry] Serialize error for {}: {}", session.session_id, e),
            }
        }
        if let Some(parent) = path.parent() {
            let _ = tokio::fs::create_dir_all(parent).await;
        }
        if let Err(e) = tokio::fs::write(path, lines.as_bytes()).await {
            log::error!("[SessionRegistry] Failed to compact-write {:?}: {}", path, e);
        } else {
            log::debug!("[SessionRegistry] Compacted {} sessions to {:?}", sessions.len(), path);
        }
    }

    /// Load sessions from disk (if persistence is configured).
    /// Reads JSONL (last line per session_id wins — handles un-compacted append logs).
    /// Falls back to legacy `sessions.json` array format for migration.
    /// Loaded sessions have transient process state cleared (PIDs are gone after restart).
    pub async fn load_from_disk(&self) {
        let Some(path) = &self.persist_path else {
            return;
        };

        // Try JSONL first; fall back to legacy .json array for one-time migration.
        let raw = match tokio::fs::read(path).await {
            Ok(b) => b,
            Err(_) => {
                // Check for legacy .json file
                let legacy = path.with_extension("json");
                match tokio::fs::read(&legacy).await {
                    Ok(b) => {
                        log::info!("[SessionRegistry] Migrating from legacy {:?} → {:?}", legacy, path);
                        b
                    }
                    Err(_) => {
                        log::info!("[SessionRegistry] No persisted sessions found at {:?}", path);
                        return;
                    }
                }
            }
        };

        // Try JSONL (one JSON object per line, last-write-wins).
        let text = String::from_utf8_lossy(&raw);
        let mut map: HashMap<String, SessionState> = HashMap::new();
        let mut jsonl_ok = false;
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() { continue; }
            match serde_json::from_str::<SessionState>(line) {
                Ok(s) => { map.insert(s.session_id.clone(), s); jsonl_ok = true; }
                Err(_) => { jsonl_ok = false; break; }
            }
        }

        // Fall back: try legacy JSON array format.
        if !jsonl_ok {
            match serde_json::from_slice::<Vec<SessionState>>(&raw) {
                Ok(loaded) => {
                    for s in loaded { map.insert(s.session_id.clone(), s); }
                }
                Err(e) => {
                    log::error!("[SessionRegistry] Failed to deserialize sessions from {:?}: {}", path, e);
                    return;
                }
            }
        }

        let count = map.len();
        {
            let mut sessions = self.sessions.write().await;
            for mut session in map.into_values() {
                // Reset transient process state — PIDs are meaningless after restart
                session.agent_pid = None;
                session.status = SessionStatus::AgentNotRunning;
                session.current_step = None;
                sessions.insert(session.session_id.clone(), session);
            }
        }
        log::info!("[SessionRegistry] Loaded {} sessions from {:?}", count, path);
        // Compact immediately so we start with a clean JSONL (no duplicates from pre-migration)
        self.save_to_disk().await;
    }

    /// Create a new session and return its state
    pub async fn create_session(&self, explicit_id: Option<String>) -> SessionState {
        let session_id = explicit_id
            .unwrap_or_else(|| format!("session_{}", Uuid::new_v4().to_string().replace("-", "")));
        let session = SessionState::new(session_id.clone());

        {
            let mut sessions = self.sessions.write().await;
            sessions.insert(session_id.clone(), session.clone());
        }

        log::debug!("Created new session: {}", session_id);
        self.append_session_to_disk(&session_id).await;
        session
    }

    /// Get session state by ID
    pub async fn get_session(&self, session_id: &str) -> Option<SessionState> {
        let sessions = self.sessions.read().await;
        sessions.get(session_id).cloned()
    }

    /// Validate token and return session if valid
    pub async fn validate_token(&self, session_id: &str, token: &str) -> Option<SessionState> {
        let sessions = self.sessions.read().await;

        if let Some(session) = sessions.get(session_id) {
            if session.auth_token == token {
                return Some(session.clone());
            }
        }

        None
    }

    /// Get the auth token for a session (for testing/logging)
    pub async fn get_token(&self, session_id: &str) -> Option<SecretString> {
        let sessions = self.sessions.read().await;
        sessions.get(session_id).map(|s| s.auth_token.clone())
    }

    /// Get or create a session with the given ID
    /// Returns the session and a flag indicating if it's a new session
    /// This enables the "hybrid" approach for persistent sessions
    pub async fn get_or_create_session(&self, session_id: &str) -> (SessionState, bool) {
        // Try to get existing session
        if let Some(session) = self.get_session(session_id).await {
            log::debug!("Reconnected to existing session: {}", session_id);
            return (session, false);
        }

        // Create new session
        let session = self.create_session(Some(session_id.to_string())).await;
        log::info!("Created new session: {}", session_id);
        (session, true)
    }

    /// Update session status (e.g., when agent crashes)
    pub async fn update_session_status(&self, session_id: &str, status: SessionStatus) {
        {
            let mut sessions = self.sessions.write().await;
            if let Some(session) = sessions.get_mut(session_id) {
                let status_str = format!("{:?}", status);
                session.status = status;
                log::debug!("Updated session {} status to {}", session_id, status_str);
            }
        }
        self.append_session_to_disk(session_id).await;
    }

    /// Update session state
    pub async fn update_session(&self, session: SessionState) {
        let session_id = session.session_id.clone();
        {
            let mut sessions = self.sessions.write().await;
            sessions.insert(session_id.clone(), session);
        }
        self.append_session_to_disk(&session_id).await;
    }

    /// Set agent PID for a session
    pub async fn set_agent_pid(&self, session_id: &str, pid: u32) -> Result<(), String> {
        let mut sessions = self.sessions.write().await;

        if let Some(session) = sessions.get_mut(session_id) {
            session.agent_pid = Some(pid);
            log::debug!("Set agent PID {} for session {}", pid, session_id);
            Ok(())
        } else {
            Err(format!("Session {} not found", session_id))
        }
    }

    /// Push a message to a session's inbox (in-place update)
    pub async fn push_message_to_session(
        &self,
        session_id: &str,
        channel_id: String,
        content: String,
        sender: String,
        run_id: Option<String>,
    ) -> Result<(), String> {
        let mut sessions = self.sessions.write().await;
        if let Some(session) = sessions.get_mut(session_id) {
            session.push_message(channel_id, content, sender, run_id);
            Ok(())
        } else {
            Err(format!("Session {} not found", session_id))
        }
    }

    /// Mark session as terminated
    pub async fn terminate_session(&self, session_id: &str) -> Result<(), String> {
        let result = {
            let mut sessions = self.sessions.write().await;
            if let Some(session) = sessions.get_mut(session_id) {
                session.status = SessionStatus::Terminated;
                log::debug!("Terminated session {}", session_id);
                Ok(())
            } else {
                Err(format!("Session {} not found", session_id))
            }
        };
        self.append_session_to_disk(session_id).await;
        result
    }

    /// List all active sessions
    pub async fn list_active_sessions(&self) -> Vec<SessionState> {
        let sessions = self.sessions.read().await;
        sessions
            .values()
            .filter(|s| s.status == SessionStatus::Active)
            .cloned()
            .collect()
    }

    /// Return all sessions regardless of status (used by admin endpoints and monitor).
    pub async fn list_all_sessions(&self) -> Vec<SessionState> {
        let sessions = self.sessions.read().await;
        sessions.values().cloned().collect()
    }

    /// Drain inbox for a specific session (atomically)
    pub async fn drain_session_inbox(&self, session_id: &str) -> Vec<ChatMessage> {
        let mut sessions = self.sessions.write().await;
        if let Some(session) = sessions.get_mut(session_id) {
            session.drain_inbox()
        } else {
            Vec::new()
        }
    }

    /// Archive sessions that are `Terminated` or `AgentNotRunning` and whose
    /// `last_activity` is older than `older_than`.  Archived sessions are
    /// appended (one JSON object per line) to `<persist_stem>_archive.jsonl`
    /// and then removed from the live in-memory map so the main `sessions.json`
    /// stays small and loads fast.
    ///
    /// Returns the number of sessions that were archived.
    pub async fn archive_old_sessions(&self, older_than: chrono::Duration) -> usize {
        let Some(path) = &self.persist_path else {
            return 0;
        };
        let archive_path = Self::archive_path(path);
        let now = Utc::now();

        let to_archive: Vec<SessionState> = {
            let sessions = self.sessions.read().await;
            sessions
                .values()
                .filter(|s| {
                    matches!(
                        s.status,
                        SessionStatus::Terminated | SessionStatus::AgentNotRunning
                    ) && now.signed_duration_since(s.last_activity) > older_than
                })
                .cloned()
                .collect()
        };

        if to_archive.is_empty() {
            return 0;
        }

        // Build JSONL lines to append
        let mut lines = String::new();
        for session in &to_archive {
            match serde_json::to_string(session) {
                Ok(line) => {
                    lines.push_str(&line);
                    lines.push('\n');
                }
                Err(e) => {
                    log::error!(
                        "[SessionRegistry] Failed to serialize session {} for archive: {}",
                        session.session_id,
                        e
                    );
                }
            }
        }

        if let Some(parent) = archive_path.parent() {
            let _ = tokio::fs::create_dir_all(parent).await;
        }

        match tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&archive_path)
            .await
        {
            Ok(mut f) => {
                if let Err(e) = f.write_all(lines.as_bytes()).await {
                    log::error!("[SessionRegistry] Failed to write to archive {:?}: {}", archive_path, e);
                    return 0;
                }
            }
            Err(e) => {
                log::error!("[SessionRegistry] Failed to open archive file {:?}: {}", archive_path, e);
                return 0;
            }
        }

        let count = to_archive.len();

        // Remove archived sessions from the live map
        {
            let mut sessions = self.sessions.write().await;
            for s in &to_archive {
                sessions.remove(&s.session_id);
            }
        }

        log::info!(
            "[SessionRegistry] Archived {} old session(s) (older than {}h) to {:?}",
            count,
            older_than.num_hours(),
            archive_path
        );

        // Persist the trimmed live set
        self.save_to_disk().await;
        count
    }

    /// Clean up terminated sessions older than threshold
    pub async fn cleanup_terminated(&self, older_than: chrono::Duration) -> usize {
        let mut sessions = self.sessions.write().await;
        let now = Utc::now();

        let to_remove: Vec<String> = sessions
            .iter()
            .filter(|(_, s)| {
                s.status == SessionStatus::Terminated
                    && now.signed_duration_since(s.last_activity) > older_than
            })
            .map(|(id, _)| id.clone())
            .collect();

        let count = to_remove.len();
        for id in to_remove {
            sessions.remove(&id);
            log::debug!("Cleaned up terminated session: {}", id);
        }

        count
    }
}

impl Default for SessionRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_create_session() {
        let registry = SessionRegistry::new();
        let session = registry.create_session(None).await;

        assert!(!session.session_id.is_empty());
        assert_eq!(session.status, SessionStatus::Active);
        assert!(!session.auth_token.is_empty());
    }

    #[tokio::test]
    async fn test_validate_token() {
        let registry = SessionRegistry::new();
        let session = registry.create_session(None).await;
        let token = session.auth_token.to_string();

        // Valid token
        let validated = registry.validate_token(&session.session_id, &token).await;
        assert!(validated.is_some());

        // Invalid token
        let validated = registry
            .validate_token(&session.session_id, "wrong_token")
            .await;
        assert!(validated.is_none());

        // Wrong session
        let validated = registry.validate_token("wrong_session", &token).await;
        assert!(validated.is_none());
    }

    #[tokio::test]
    async fn test_inbox_operations() {
        let mut session = SessionState::new("test".to_string());

        session.push_message(
            "chan1".to_string(),
            "Hello".to_string(),
            "user".to_string(),
            None,
        );
        session.push_message(
            "chan1".to_string(),
            "World".to_string(),
            "user".to_string(),
            None,
        );

        assert_eq!(session.inbox.len(), 2);

        let messages = session.drain_inbox();
        assert_eq!(messages.len(), 2);
        assert!(session.inbox.is_empty());
    }

    #[tokio::test]
    async fn test_terminate_session() {
        let registry = SessionRegistry::new();
        let session = registry.create_session(None).await;

        registry
            .terminate_session(&session.session_id)
            .await
            .unwrap();

        let updated = registry.get_session(&session.session_id).await.unwrap();
        assert_eq!(updated.status, SessionStatus::Terminated);
    }
}
