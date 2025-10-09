//! Generic Status Taxonomy for Conversational Synthesis
//!
//! Defines domain-agnostic status constants for managing any multi-turn
//! conversational refinement loop (trip planning, investment, recipes, etc.).
//!
//! Design: names are intentionally generic (e.g., `completed` vs `itinerary_ready`) to
//! encourage reuse across domains. The state machine is domain-agnostic.

/// User interaction phase: asking clarifying questions, not all required params known yet.
pub const STATUS_COLLECTING_INFO: &str = "collecting_info";

/// All required user inputs gathered; ready to invoke processing capability (if available).
pub const STATUS_READY_FOR_EXECUTION: &str = "ready_for_execution";

/// External agent/capability referenced but not found in registry; awaiting resolution or fallback.
pub const STATUS_REQUIRES_AGENT: &str = "requires_agent";

/// Deferred state: waiting for agent registration or scheduled recheck.
pub const STATUS_AGENT_UNAVAILABLE_RETRY: &str = "agent_unavailable_retry";

/// Processing in progress (plan generation, analysis, computation, etc.).
pub const STATUS_PROCESSING: &str = "processing";

/// Final result successfully produced. Terminal state.
pub const STATUS_COMPLETED: &str = "completed";

/// No further refinement possible/needed; final result emitted. Terminal state.
pub const STATUS_REFINEMENT_EXHAUSTED: &str = "refinement_exhausted";

/// Returns true if the given status is a terminal state (interaction should end).
/// Only `completed` and `refinement_exhausted` are terminal.
pub fn is_terminal(status: &str) -> bool {
    matches!(status, STATUS_COMPLETED | STATUS_REFINEMENT_EXHAUSTED)
}

/// Returns true if the interaction loop can continue with this status.
pub fn can_continue(status: &str) -> bool {
    !is_terminal(status)
}

/// Returns a human-readable description of the status for logging/display.
pub fn status_description(status: &str) -> &'static str {
    match status {
        STATUS_COLLECTING_INFO => "Collecting user information",
        STATUS_READY_FOR_EXECUTION => "Ready to execute capability",
        STATUS_REQUIRES_AGENT => "Requires external agent (not found)",
        STATUS_AGENT_UNAVAILABLE_RETRY => "Waiting for agent registration",
        STATUS_PROCESSING => "Processing request",
        STATUS_COMPLETED => "Completed (terminal)",
        STATUS_REFINEMENT_EXHAUSTED => "Refinement exhausted (terminal)",
        _ => "Unknown status",
    }
}

/// Validates that a status string is recognized.
pub fn is_valid_status(status: &str) -> bool {
    matches!(
        status,
        STATUS_COLLECTING_INFO
            | STATUS_READY_FOR_EXECUTION
            | STATUS_REQUIRES_AGENT
            | STATUS_AGENT_UNAVAILABLE_RETRY
            | STATUS_PROCESSING
            | STATUS_COMPLETED
            | STATUS_REFINEMENT_EXHAUSTED
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_terminal_statuses() {
        assert!(is_terminal(STATUS_COMPLETED));
        assert!(is_terminal(STATUS_REFINEMENT_EXHAUSTED));

        assert!(!is_terminal(STATUS_COLLECTING_INFO));
        assert!(!is_terminal(STATUS_READY_FOR_EXECUTION));
        assert!(!is_terminal(STATUS_REQUIRES_AGENT));
        assert!(!is_terminal(STATUS_AGENT_UNAVAILABLE_RETRY));
        assert!(!is_terminal(STATUS_PROCESSING));
    }

    #[test]
    fn test_can_continue() {
        assert!(can_continue(STATUS_COLLECTING_INFO));
        assert!(can_continue(STATUS_READY_FOR_EXECUTION));
        assert!(can_continue(STATUS_REQUIRES_AGENT));

        assert!(!can_continue(STATUS_COMPLETED));
        assert!(!can_continue(STATUS_REFINEMENT_EXHAUSTED));
    }

    #[test]
    fn test_status_validation() {
        assert!(is_valid_status(STATUS_COLLECTING_INFO));
        assert!(is_valid_status(STATUS_COMPLETED));
        assert!(!is_valid_status("invalid_status"));
        assert!(!is_valid_status(""));
    }

    #[test]
    fn test_status_descriptions() {
        assert_eq!(status_description(STATUS_COLLECTING_INFO), "Collecting user information");
        assert_eq!(status_description(STATUS_COMPLETED), "Completed (terminal)");
        assert_eq!(status_description("unknown"), "Unknown status");
    }
}


