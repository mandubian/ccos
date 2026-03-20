//! Best-effort mirror of orchestration transitions into the gateway causal chain.
//!
//! See `plan_workflow_orchestration.md` § "Relationship To Causal Chain": workflow JSONL is
//! mutable coordination state; the causal chain remains the append-only audit log. These entries
//! link the two (`workflow_id`, `task_id` in payload) without replacing causal semantics.

use autonoetic_types::causal_chain::EntryStatus;
use autonoetic_types::config::GatewayConfig;
use serde_json::Value;
use std::time::{SystemTime, UNIX_EPOCH};

/// `event_seq` for mirrored rows: wall-clock millis so entries stay ordered without a per-session counter.
fn orchestration_event_seq() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Append a gateway causal entry (category `gateway`) for an orchestration transition.
///
/// `session_id_for_index` is the session whose timeline/index should receive the row (use root
/// session for operator-visible delegation lines).
pub fn mirror_orchestration_event(
    config: &GatewayConfig,
    session_id_for_index: &str,
    action: &str,
    status: EntryStatus,
    payload: Value,
) {
    let Ok(logger) = crate::execution::init_gateway_causal_logger(config) else {
        return;
    };
    let seq = orchestration_event_seq();
    crate::execution::log_gateway_causal_event(
        &logger,
        &crate::execution::gateway_actor_id(),
        session_id_for_index,
        seq,
        action,
        status,
        Some(payload),
    );
}
