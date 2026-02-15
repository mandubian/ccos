//! Session Management for Gateway "Sheriff"
//!
//! Manages active sessions, authentication tokens, and session lifecycle.
//! The SessionRegistry is the authoritative source for session state.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
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
#[derive(Debug, Clone)]
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
}

impl SessionRegistry {
    /// Create a new empty registry
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create a new session and return its state
    pub async fn create_session(&self, explicit_id: Option<String>) -> SessionState {
        let session_id = explicit_id
            .unwrap_or_else(|| format!("session_{}", Uuid::new_v4().to_string().replace("-", "")));
        let session = SessionState::new(session_id.clone());

        let mut sessions = self.sessions.write().await;
        sessions.insert(session_id.clone(), session.clone());

        log::debug!("Created new session: {}", session_id);
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
        let mut sessions = self.sessions.write().await;
        if let Some(session) = sessions.get_mut(session_id) {
            let status_str = format!("{:?}", status);
            session.status = status;
            log::debug!("Updated session {} status to {}", session_id, status_str);
        }
    }

    /// Update session state
    pub async fn update_session(&self, session: SessionState) {
        let mut sessions = self.sessions.write().await;
        sessions.insert(session.session_id.clone(), session);
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
        let mut sessions = self.sessions.write().await;

        if let Some(session) = sessions.get_mut(session_id) {
            session.status = SessionStatus::Terminated;
            log::debug!("Terminated session {}", session_id);
            Ok(())
        } else {
            Err(format!("Session {} not found", session_id))
        }
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

    /// Drain inbox for a specific session (atomically)
    pub async fn drain_session_inbox(&self, session_id: &str) -> Vec<ChatMessage> {
        let mut sessions = self.sessions.write().await;
        if let Some(session) = sessions.get_mut(session_id) {
            session.drain_inbox()
        } else {
            Vec::new()
        }
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
