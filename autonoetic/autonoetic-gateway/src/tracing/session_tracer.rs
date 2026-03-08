//! Session Tracer - centralized session management and trace emission.
//!
//! This module provides a unified abstraction for managing session lifecycle,
//! event sequencing, and causal trace emission across the gateway.

use crate::causal_chain::CausalLogger;
use autonoetic_types::causal_chain::EntryStatus;
use std::sync::Arc;
use uuid::Uuid;

/// Unique identifier for a trace session.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SessionId(String);

impl SessionId {
    pub fn new() -> Self {
        Self(Uuid::new_v4().to_string())
    }

    pub fn from_string(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn is_valid(&self) -> bool {
        !self.0.is_empty()
    }
}

impl Default for SessionId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for SessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Event sequence counter with proper scoping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventScope {
    Request,
    Session,
}

#[derive(Debug)]
pub struct EventSeq {
    counter: u64,
    scope: EventScope,
}

impl EventSeq {
    pub fn new(scope: EventScope) -> Self {
        Self { counter: 0, scope }
    }

    pub fn request() -> Self {
        Self::new(EventScope::Request)
    }

    pub fn session() -> Self {
        Self::new(EventScope::Session)
    }

    pub fn next(&mut self) -> u64 {
        self.counter += 1;
        self.counter
    }

    pub fn reset(&mut self) {
        self.counter = 0;
    }

    pub fn current(&self) -> u64 {
        self.counter
    }

    pub fn scope(&self) -> EventScope {
        self.scope
    }
}


pub struct TraceSession {
    session_id: SessionId,
    event_seq: EventSeq,
    causal_logger: Arc<CausalLogger>,
    actor_id: String,
}

impl TraceSession {
    pub fn new(
        session_id: impl Into<SessionId>,
        event_seq: EventSeq,
        causal_logger: Arc<CausalLogger>,
        actor_id: impl Into<String>,
    ) -> Self {
        Self {
            session_id: session_id.into(),
            event_seq,
            causal_logger,
            actor_id: actor_id.into(),
        }
    }

    pub fn create(
        causal_logger: Arc<CausalLogger>,
        actor_id: impl Into<String>,
        event_scope: EventScope,
    ) -> Self {
        Self::new(
            SessionId::new(),
            EventSeq::new(event_scope),
            causal_logger,
            actor_id,
        )
    }

    pub fn create_with_session_id(
        session_id: impl Into<SessionId>,
        causal_logger: Arc<CausalLogger>,
        actor_id: impl Into<String>,
        event_scope: EventScope,
    ) -> Self {
        Self::new(
            session_id,
            EventSeq::new(event_scope),
            causal_logger,
            actor_id,
        )
    }

    pub fn session_id(&self) -> &SessionId {
        &self.session_id
    }

    pub fn event_seq(&self) -> &EventSeq {
        &self.event_seq
    }

    pub fn next_event_seq(&mut self) -> u64 {
        self.event_seq.next()
    }

    pub fn reset_event_seq(&mut self) {
        self.event_seq.reset();
    }

    pub fn log_requested(
        &mut self,
        action: &str,
        payload: Option<serde_json::Value>,
    ) -> anyhow::Result<()> {
        let session_id = self.session_id.as_str().to_string();
        let event_seq = self.next_event_seq();
        self.causal_logger.log(
            &self.actor_id,
            &session_id,
            None,
            event_seq,
            "gateway",
            &format!("{}.requested", action),
            EntryStatus::Success,
            payload,
        )
    }

    pub fn log_completed(
        &mut self,
        action: &str,
        _reason: Option<&str>,
        payload: Option<serde_json::Value>,
    ) -> anyhow::Result<()> {
        let session_id = self.session_id.as_str().to_string();
        let event_seq = self.next_event_seq();
        self.causal_logger.log(
            &self.actor_id,
            &session_id,
            None,
            event_seq,
            "gateway",
            &format!("{}.completed", action),
            EntryStatus::Success,
            payload,
        )
    }

    pub fn log_failed(
        &mut self,
        action: &str,
        reason: &str,
        _payload: Option<serde_json::Value>,
    ) -> anyhow::Result<()> {
        let session_id = self.session_id.as_str().to_string();
        let event_seq = self.next_event_seq();
        self.causal_logger.log(
            &self.actor_id,
            &session_id,
            None,
            event_seq,
            "gateway",
            &format!("{}.failed", action),
            EntryStatus::Error,
            Some(serde_json::json!({"reason": reason})),
        )
    }

    pub fn log_skipped(
        &mut self,
        action: &str,
        reason: &str,
        _payload: Option<serde_json::Value>,
    ) -> anyhow::Result<()> {
        let session_id = self.session_id.as_str().to_string();
        let event_seq = self.next_event_seq();
        self.causal_logger.log(
            &self.actor_id,
            &session_id,
            None,
            event_seq,
            "gateway",
            &format!("{}.skipped", action),
            EntryStatus::Success,
            Some(serde_json::json!({"reason": reason})),
        )
    }

    pub fn log_denied(
        &mut self,
        action: &str,
        reason: &str,
        _payload: Option<serde_json::Value>,
    ) -> anyhow::Result<()> {
        let session_id = self.session_id.as_str().to_string();
        let event_seq = self.next_event_seq();
        self.causal_logger.log(
            &self.actor_id,
            &session_id,
            None,
            event_seq,
            "gateway",
            &format!("{}.denied", action),
            EntryStatus::Denied,
            Some(serde_json::json!({"reason": reason})),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use crate::causal_chain::CausalLogger;

    fn create_test_logger() -> CausalLogger {
        let dir = tempdir().expect("tempdir should create");
        let path = dir.path().join("causal_chain.jsonl");
        CausalLogger::new(path).expect("logger should create")
    }

    #[test]
    fn test_session_id_generation() {
        let id1 = SessionId::new();
        let id2 = SessionId::new();
        assert!(id1.is_valid());
        assert!(id2.is_valid());
        assert_ne!(id1.as_str(), id2.as_str());
    }

    #[test]
    fn test_event_seq_request_scope() {
        let mut seq = EventSeq::request();
        assert_eq!(seq.next(), 1);
        assert_eq!(seq.next(), 2);
        assert_eq!(seq.scope(), EventScope::Request);
    }

    #[test]
    fn test_trace_session_creation() {
        let logger = create_test_logger();
        let session = TraceSession::create(Arc::new(logger), "test-actor", EventScope::Request);
        assert!(session.session_id().is_valid());
        assert_eq!(session.event_seq().scope(), EventScope::Request);
    }

    #[tokio::test]
    async fn test_trace_ordering_with_duplicate_event_seqs() {
        // Regression test for trace ordering when event_seq resets across sessions
        // Uses a single logger to validate that events are written in timestamp order,
        // not event_seq order (since event_seq resets per TraceSession)
        let temp = tempfile::tempdir().expect("tempdir should create");
        let path = temp.path().join("causal_chain.jsonl");
        
        // Create logger with explicit path
        let logger = Arc::new(CausalLogger::new(&path).expect("logger should create"));
        
        // Create two TraceSessions with the same logger (shared Arc)
        let mut trace1 = TraceSession::create(logger.clone(), "actor", EventScope::Request);
        let mut trace2 = TraceSession::create(logger.clone(), "actor", EventScope::Request);
        
        // Simulate sequential events from different sessions with same event_seq
        let _ = trace1.log_requested("action.1", None);
        let _ = trace2.log_requested("action.2", None);
        let _ = trace1.log_completed("action.1", None, None);
        let _ = trace2.log_completed("action.2", None, None);
        
        // Verify file was written
        let entries = CausalLogger::read_entries(&path).expect("should read entries");
        assert_eq!(entries.len(), 4, "all entries should be written");
        
        // Verify entries are sorted by timestamp (not event_seq)
        // Since event_seq resets per TraceSession, timestamp is the only stable sort key
        for i in 1..entries.len() {
            let prev_ts = &entries[i - 1].timestamp;
            let curr_ts = &entries[i].timestamp;
            assert!(prev_ts <= curr_ts, 
                "Entries must be ordered by timestamp: {:?} > {:?}", prev_ts, curr_ts);
        }
        
        // Verify event_seq values - they should be [1, 1, 2, 2] (resets per session)
        let event_seqs: Vec<u64> = entries.iter().map(|e| e.event_seq).collect();
        assert_eq!(event_seqs, vec![1, 1, 2, 2], 
            "event_seq should reset per session: {:?}", event_seqs);
        
        // Verify actions are in correct timestamp order (not event_seq order)
        let actions: Vec<&str> = entries.iter().map(|e| e.action.as_str()).collect();
        assert_eq!(actions, vec!["action.1.requested", "action.2.requested", "action.1.completed", "action.2.completed"],
            "Actions should be in timestamp order");
    }
}
