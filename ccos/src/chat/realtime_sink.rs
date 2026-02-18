//! Real-time event streaming for session-based tracking
//!
//! Bridges Causal Chain events + Session state updates to WebSocket clients.
//! Implements the CausalChainEventSink trait to receive actions and broadcast them.

use crate::causal_chain::CausalChain;
use crate::event_sink::CausalChainEventSink;
use crate::types::Action;
use chrono::Utc;
use rtfs::runtime::values::Value as RtfsValue;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};

/// Events streamed to WebSocket clients
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event_type")]
pub enum SessionEvent {
    /// Historical action from Causal Chain (sent on initial connect)
    #[serde(rename = "historical")]
    Historical { action: ActionView },

    /// Live action from Causal Chain
    #[serde(rename = "action")]
    Action { action: ActionView },

    /// Session state update (from heartbeat)
    #[serde(rename = "state_update")]
    StateUpdate {
        timestamp: u64,
        agent_pid: Option<u32>,
        current_step: Option<u32>,
        memory_mb: Option<u64>,
        is_healthy: bool,
    },

    /// Agent crash detected
    #[serde(rename = "agent_crashed")]
    AgentCrashed {
        pid: u32,
        exit_code: Option<i32>,
        timestamp: u64,
    },

    /// WebSocket ping (keepalive)
    #[serde(rename = "ping")]
    Ping { timestamp: u64 },
}

/// Lightweight view of Action for streaming
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionView {
    pub action_id: String,
    pub action_type: String,
    pub function_name: Option<String>,
    pub timestamp: u64,
    pub run_id: Option<String>,
    pub step_id: Option<String>,
    pub success: Option<bool>,
    pub duration_ms: Option<u64>,
    pub summary: String,
    /// Metadata for agent LLM consultations and other rich data
    #[serde(default)]
    pub metadata: Option<serde_json::Value>,
}

impl From<&Action> for ActionView {
    fn from(action: &Action) -> Self {
        let run_id = action
            .metadata
            .get("run_id")
            .and_then(|v| v.as_string())
            .map(|s| s.to_string());
        let step_id = action
            .metadata
            .get("step_id")
            .and_then(|v| v.as_string())
            .map(|s| s.to_string());
        let success = action
            .result
            .as_ref()
            .map(|r| r.success);

        // Convert metadata to JSON for streaming
        let mut json_map = serde_json::Map::new();
        for (key, value) in &action.metadata {
            // Convert RTFS Value to JSON Value
            let json_val = rtfs_value_to_json(value);
            json_map.insert(key.clone(), json_val);
        }

        let mut result_preview: Option<String> = None;
        
        // Also extract result value fields for code execution details
        if let Some(ref result) = action.result {
            let result_json = rtfs_value_to_json(&result.value);
            json_map.insert("result".to_string(), result_json.clone());

            result_preview = Some(match &result_json {
                serde_json::Value::Null => "null".to_string(),
                serde_json::Value::Bool(b) => format!("{}", b),
                serde_json::Value::Number(n) => n.to_string(),
                serde_json::Value::String(s) => {
                    if s.chars().count() > 140 {
                        format!("{}...", s.chars().take(137).collect::<String>())
                    } else {
                        s.clone()
                    }
                }
                other => {
                    let serialized = serde_json::to_string(other).unwrap_or_else(|_| "<result>".to_string());
                    if serialized.chars().count() > 140 {
                        format!("{}...", serialized.chars().take(137).collect::<String>())
                    } else {
                        serialized
                    }
                }
            });

            if let RtfsValue::Map(ref result_map) = result.value {
                // Extract common execution result fields
                for key in [
                    "stdout",
                    "stderr",
                    "code",
                    "success",
                    "exit_code",
                    "explanation",
                    "error",
                    "capability_id",
                ]
                .iter()
                {
                    // Try both String and Keyword keys
                    let rtfs_key = rtfs::ast::MapKey::String(key.to_string());
                    let kw_key = rtfs::ast::MapKey::Keyword(rtfs::ast::Keyword(key.to_string()));
                    
                    if let Some(val) = result_map.get(&rtfs_key).or_else(|| result_map.get(&kw_key)) {
                        // Only add if not already in metadata
                        if !json_map.contains_key(*key) {
                            let json_val = rtfs_value_to_json(val);
                            json_map.insert(key.to_string(), json_val);
                        }
                    }
                }
            }
        }

        // Create a human-readable summary.
        // The monitor already shows function_name and a status label separately,
        // so the summary only carries the result preview (or a short status word
        // as a fallback when there is no result value to show).
        let summary = if let Some(ref preview) = result_preview {
            format!("result={}", preview)
        } else {
            match success {
                Some(true) => "success".to_string(),
                Some(false) => "failed".to_string(),
                None => String::new(),
            }
        };

        let metadata_json = if json_map.is_empty() {
            None
        } else {
            Some(serde_json::Value::Object(json_map))
        };

        Self {
            action_id: action.action_id.to_string(),
            action_type: format!("{:?}", action.action_type),
            function_name: action.function_name.clone(),
            timestamp: action.timestamp,
            run_id,
            step_id,
            success,
            duration_ms: action.duration_ms,
            summary,
            metadata: metadata_json,
        }
    }
}

/// Helper function to convert RTFS Value to JSON Value
fn rtfs_value_to_json(value: &RtfsValue) -> serde_json::Value {
    match value {
        RtfsValue::String(s) => serde_json::Value::String(s.clone()),
        RtfsValue::Integer(i) => serde_json::Value::Number((*i).into()),
        RtfsValue::Float(f) => serde_json::Number::from_f64(*f)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
        RtfsValue::Boolean(b) => serde_json::Value::Bool(*b),
        RtfsValue::Nil => serde_json::Value::Null,
        RtfsValue::Vector(v) => {
            serde_json::Value::Array(v.iter().map(rtfs_value_to_json).collect())
        }
        RtfsValue::Map(m) => {
            let mut map = serde_json::Map::new();
            for (k, v) in m {
                map.insert(k.to_string(), rtfs_value_to_json(v));
            }
            serde_json::Value::Object(map)
        }
        RtfsValue::List(v) => {
            serde_json::Value::Array(v.iter().map(rtfs_value_to_json).collect())
        }
        _ => serde_json::Value::Null,
    }
}

/// Session state snapshot for broadcasting
#[derive(Debug, Clone)]
pub struct SessionStateSnapshot {
    pub agent_pid: Option<u32>,
    pub current_step: Option<u32>,
    pub memory_mb: Option<u64>,
    pub is_healthy: bool,
}

/// Real-time tracking sink that broadcasts Causal Chain events to WebSocket clients
pub struct RealTimeTrackingSink {
    /// Session ID -> broadcast channel for WebSocket clients
    subscribers: Arc<RwLock<HashMap<String, broadcast::Sender<SessionEvent>>>>,
    /// Maximum number of historical events to replay
    replay_limit: usize,
}

impl RealTimeTrackingSink {
    /// Create a new real-time tracking sink
    pub fn new(replay_limit: usize) -> Self {
        Self {
            subscribers: Arc::new(RwLock::new(HashMap::new())),
            replay_limit,
        }
    }

    /// Subscribe to events for a specific session
    pub async fn subscribe(&self, session_id: &str) -> broadcast::Receiver<SessionEvent> {
        let mut subs = self.subscribers.write().await;
        let sender = subs.entry(session_id.to_string()).or_insert_with(|| {
            broadcast::channel(1024).0
        });
        sender.subscribe()
    }

    /// Get recent historical events for a session from Causal Chain
    pub fn get_session_history(
        &self,
        chain: &CausalChain,
        session_id: &str,
    ) -> Vec<SessionEvent> {
        use crate::causal_chain::CausalQuery;
        
        let query = CausalQuery {
            session_id: Some(session_id.to_string()),
            ..Default::default()
        };
        
        chain.query_actions(&query)
            .into_iter()
            .rev() // Most recent first
            .take(self.replay_limit)
            .rev() // Back to chronological order
            .map(|action| SessionEvent::Historical {
                action: ActionView::from(action)
            })
            .collect()
    }

    /// Broadcast a state update to all subscribers for a session
    pub async fn broadcast_state_update(
        &self,
        session_id: &str,
        state: &SessionStateSnapshot,
    ) {
        let subs = self.subscribers.read().await;
        if let Some(sender) = subs.get(session_id) {
            let event = SessionEvent::StateUpdate {
                timestamp: Utc::now().timestamp() as u64,
                agent_pid: state.agent_pid,
                current_step: state.current_step,
                memory_mb: state.memory_mb,
                is_healthy: state.is_healthy,
            };
            // Ignore send errors (client may have disconnected)
            let _ = sender.send(event);
        }
    }

    /// Broadcast agent crash event to all subscribers
    pub async fn broadcast_crash(&self, session_id: &str, pid: u32, exit_code: Option<i32>) {
        let subs = self.subscribers.read().await;
        if let Some(sender) = subs.get(session_id) {
            let event = SessionEvent::AgentCrashed {
                pid,
                exit_code,
                timestamp: Utc::now().timestamp() as u64,
            };
            let _ = sender.send(event);
        }
    }

    /// Clean up disconnected subscriber channels
    pub async fn cleanup_session(&self, session_id: &str) {
        let mut subs = self.subscribers.write().await;
        if let Some(sender) = subs.get(session_id) {
            // If no receivers, remove the channel
            if sender.receiver_count() == 0 {
                subs.remove(session_id);
            }
        }
    }
}

impl std::fmt::Debug for RealTimeTrackingSink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RealTimeTrackingSink")
            .field("replay_limit", &self.replay_limit)
            .finish()
    }
}

impl CausalChainEventSink for RealTimeTrackingSink {
    fn on_action_appended(&self, action: &Action) {
        let session_id = match &action.session_id {
            Some(id) => id.clone(),
            None => return, // No session = no streaming
        };

        let event = SessionEvent::Action {
            action: ActionView::from(action),
        };

        // Spawn async task to broadcast (non-blocking)
        let subs = self.subscribers.clone();
        let session_id_clone = session_id.clone();

        tokio::spawn(async move {
            let subs = subs.read().await;
            if let Some(sender) = subs.get(&session_id_clone) {
                // Ignore send errors (no subscribers or disconnected)
                let _ = sender.send(event);
            }
        });
    }

    fn is_wm_sink(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_subscribe_creates_channel() {
        let sink = RealTimeTrackingSink::new(100);
        let rx = sink.subscribe("test-session").await;
        assert!(!rx.is_closed());
    }

    #[tokio::test]
    async fn test_broadcast_state_update() {
        let sink = RealTimeTrackingSink::new(100);
        let mut rx = sink.subscribe("test-session").await;

        let state = SessionStateSnapshot {
            agent_pid: Some(1234),
            current_step: Some(5),
            memory_mb: Some(128),
            is_healthy: true,
        };

        sink.broadcast_state_update("test-session", &state).await;

        if let Ok(event) = rx.try_recv() {
            match event {
                SessionEvent::StateUpdate {
                    agent_pid,
                    current_step,
                    memory_mb,
                    is_healthy,
                    ..
                } => {
                    assert_eq!(agent_pid, Some(1234));
                    assert_eq!(current_step, Some(5));
                    assert_eq!(memory_mb, Some(128));
                    assert!(is_healthy);
                }
                _ => panic!("Expected StateUpdate event"),
            }
        } else {
            panic!("Expected to receive event");
        }
    }
}
