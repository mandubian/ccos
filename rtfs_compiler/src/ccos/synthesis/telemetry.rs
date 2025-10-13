//! Telemetry
//!
//! Emits JSON events for observability (spec section 25).
//! All events have format: {"type":"...", "timestamp":"ISO8601", ...}

use chrono::{DateTime, Utc};
use std::time::SystemTime;

/// Synthesis events emitted during conversation lifecycle.
#[derive(Debug, Clone)]
pub enum TelemetryEvent {
    /// Conversation status transitioned
    StatusTransition {
        from: String,
        to: String,
        conversation_id: String,
    },
    /// Parameter extracted from conversation turn
    ParameterExtracted {
        key: String,
        value: String,
        source_turn: usize,
        conversation_id: String,
    },
    /// Artifact generated (collector/planner/stub)
    ArtifactGenerated {
        artifact_type: String, // "collector" | "planner" | "stub"
        capability_id: String,
        conversation_id: String,
    },
    /// Missing capability detected
    MissingCapability {
        capability_id: String,
        conversation_id: String,
    },
    /// Pending execution created
    PendingExecutionCreated {
        execution_id: String,
        capability_id: String,
        conversation_id: String,
    },
}

impl TelemetryEvent {
    /// Serialize event to JSON string.
    ///
    /// Format (from spec 25.1):
    /// ```json
    /// {
    ///   "type": "status_transition",
    ///   "timestamp": "2025-02-01T12:34:56Z",
    ///   "from": "collecting_info",
    ///   "to": "ready_for_planning",
    ///   "conversation_id": "conv-123"
    /// }
    /// ```
    pub fn to_json(&self) -> String {
        let now: DateTime<Utc> = SystemTime::now().into();
        let timestamp = now.to_rfc3339();

        match self {
            TelemetryEvent::StatusTransition {
                from,
                to,
                conversation_id,
            } => {
                format!(
                    r#"{{"type":"status_transition","timestamp":"{}","from":"{}","to":"{}","conversation_id":"{}"}}"#,
                    timestamp, from, to, conversation_id
                )
            }
            TelemetryEvent::ParameterExtracted {
                key,
                value,
                source_turn,
                conversation_id,
            } => {
                format!(
                    r#"{{"type":"parameter_extracted","timestamp":"{}","key":"{}","value":"{}","source_turn":{},"conversation_id":"{}"}}"#,
                    timestamp, key, value, source_turn, conversation_id
                )
            }
            TelemetryEvent::ArtifactGenerated {
                artifact_type,
                capability_id,
                conversation_id,
            } => {
                format!(
                    r#"{{"type":"artifact_generated","timestamp":"{}","artifact_type":"{}","capability_id":"{}","conversation_id":"{}"}}"#,
                    timestamp, artifact_type, capability_id, conversation_id
                )
            }
            TelemetryEvent::MissingCapability {
                capability_id,
                conversation_id,
            } => {
                format!(
                    r#"{{"type":"missing_capability","timestamp":"{}","capability_id":"{}","conversation_id":"{}"}}"#,
                    timestamp, capability_id, conversation_id
                )
            }
            TelemetryEvent::PendingExecutionCreated {
                execution_id,
                capability_id,
                conversation_id,
            } => {
                format!(
                    r#"{{"type":"pending_execution_created","timestamp":"{}","execution_id":"{}","capability_id":"{}","conversation_id":"{}"}}"#,
                    timestamp, execution_id, capability_id, conversation_id
                )
            }
        }
    }
}

/// Emit telemetry event to stdout (Phase 7 implementation can route to structured logger).
///
/// For now, prints JSON to stdout with `[TELEMETRY]` prefix for easy filtering.
pub fn emit(event: TelemetryEvent) {
    println!("[TELEMETRY] {}", event.to_json());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_status_transition_json() {
        let event = TelemetryEvent::StatusTransition {
            from: "collecting_info".to_string(),
            to: "ready_for_planning".to_string(),
            conversation_id: "conv-123".to_string(),
        };
        let json = event.to_json();
        assert!(json.contains(r#""type":"status_transition"#));
        assert!(json.contains(r#""from":"collecting_info"#));
        assert!(json.contains(r#""to":"ready_for_planning"#));
        assert!(json.contains(r#""conversation_id":"conv-123"#));
    }

    #[test]
    fn test_parameter_extracted_json() {
        let event = TelemetryEvent::ParameterExtracted {
            key: "trip/destination".to_string(),
            value: "Paris".to_string(),
            source_turn: 2,
            conversation_id: "conv-456".to_string(),
        };
        let json = event.to_json();
        assert!(json.contains(r#""type":"parameter_extracted"#));
        assert!(json.contains(r#""key":"trip/destination"#));
        assert!(json.contains(r#""source_turn":2"#));
    }
}
