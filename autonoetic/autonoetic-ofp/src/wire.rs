//! OFP WireMessage — the base envelope for federation and IPC.

use serde::{Deserialize, Serialize};

/// The OFP WireMessage envelope as defined in data_models.md § 7.
///
/// Used for all TCP Gateway-to-Gateway federation, and internal
/// Unix Socket communication where applicable.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WireMessage {
    /// JSON-RPC style incremental or UUID request identifier.
    pub id: String,

    /// Peer ID of the sender.
    pub sender: String,

    /// Per-message HMAC-SHA256 signature (if extension negotiated).
    pub signature: Option<String>,

    /// Sequence number for replay prevention.
    pub seq_num: u64,

    /// The JSON-RPC 2.0 payload.
    pub payload: serde_json::Value,
}
