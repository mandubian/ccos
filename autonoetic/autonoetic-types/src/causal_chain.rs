//! Causal Chain log entry — immutable hash-chain audit trail.

use serde::{Deserialize, Serialize};

/// Status of a causal chain entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EntryStatus {
    Success,
    Denied,
    Error,
}

/// A single entry in the append-only `.jsonl` Causal Chain log.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CausalChainEntry {
    pub timestamp: String,
    pub log_id: String,
    pub actor_id: String,
    pub category: String,
    pub action: String,
    pub target: Option<String>,
    pub status: EntryStatus,
    pub reason: Option<String>,
    pub payload: Option<serde_json::Value>,
    pub prev_hash: String,
}
