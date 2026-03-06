//! OFP WireMessage — the base envelope for federation and IPC.
//!
//! All communication between Autonoetic/OpenFang peers uses JSON-framed messages
//! over TCP. Each message is prefixed with a 4-byte big-endian length header.
//!
//! Autonoetic is 100% wire-compatible with OpenFang.
//! Autonoetic extensions (`signature`, `seq_num`, `extensions`) are ignored by OpenFang
//! because OpenFang uses default serde parsing (which drops unknown fields).

use serde::{Deserialize, Serialize};

/// A wire protocol message (envelope).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WireMessage {
    /// Unique message ID.
    pub id: String,

    /// Autonoetic extension: Per-message HMAC-SHA256 signature (if negotiated).
    /// Prevents session hijack and replay attacks.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,

    /// Autonoetic extension: Sequence number for replay prevention.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seq_num: Option<u64>,

    /// Message variant (flattened directly into the JSON object).
    #[serde(flatten)]
    pub kind: WireMessageKind,
}

/// The different kinds of wire messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum WireMessageKind {
    /// Request from one peer to another.
    #[serde(rename = "request")]
    Request(WireRequest),
    /// Response to a request.
    #[serde(rename = "response")]
    Response(WireResponse),
    /// One-way notification (no response expected).
    #[serde(rename = "notification")]
    Notification(WireNotification),
}

/// Request messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method")]
pub enum WireRequest {
    /// Handshake: exchange peer identity.
    #[serde(rename = "handshake")]
    Handshake {
        /// The peer's unique node ID.
        node_id: String,
        /// Human-readable node name.
        node_name: String,
        /// Protocol version.
        protocol_version: u32,
        /// List of agents available on this peer.
        agents: Vec<RemoteAgentInfo>,
        /// Random nonce for HMAC authentication.
        #[serde(default)]
        nonce: String,
        /// HMAC-SHA256(shared_secret, nonce + node_id).
        #[serde(default)]
        auth_hmac: String,
        /// Autonoetic extension: list of supported protocol extensions (e.g., ["msg_hmac", "resilience"]).
        #[serde(skip_serializing_if = "Option::is_none")]
        extensions: Option<Vec<String>>,
    },
    /// Discover agents matching a query on the remote peer.
    #[serde(rename = "discover")]
    Discover {
        /// Search query (matches name, tags, description).
        query: String,
    },
    /// Send a message to a specific agent on the remote peer.
    #[serde(rename = "agent_message")]
    AgentMessage {
        /// Target agent ID or name on the remote peer.
        agent: String,
        /// The message text.
        message: String,
        /// Optional sender identity.
        sender: Option<String>,
    },
    /// Ping to check if the peer is alive.
    #[serde(rename = "ping")]
    Ping,
}

/// Response messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method")]
pub enum WireResponse {
    /// Handshake acknowledgement.
    #[serde(rename = "handshake_ack")]
    HandshakeAck {
        node_id: String,
        node_name: String,
        protocol_version: u32,
        agents: Vec<RemoteAgentInfo>,
        /// Random nonce for HMAC authentication.
        #[serde(default)]
        nonce: String,
        /// HMAC-SHA256(shared_secret, nonce + node_id).
        #[serde(default)]
        auth_hmac: String,
        /// Autonoetic extension: the extensions this peer agreed to enable.
        #[serde(skip_serializing_if = "Option::is_none")]
        extensions: Option<Vec<String>>,
    },
    /// Discovery results.
    #[serde(rename = "discover_result")]
    DiscoverResult { agents: Vec<RemoteAgentInfo> },
    /// Agent message response.
    #[serde(rename = "agent_response")]
    AgentResponse {
        /// The agent's response text.
        text: String,
    },
    /// Pong response.
    #[serde(rename = "pong")]
    Pong {
        /// Uptime in seconds.
        uptime_secs: u64,
    },
    /// Error response.
    #[serde(rename = "error")]
    Error {
        /// Error code.
        code: i32,
        /// Error message.
        message: String,
    },
}

/// Notification messages (one-way, no response).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event")]
pub enum WireNotification {
    /// An agent was spawned on the peer.
    #[serde(rename = "agent_spawned")]
    AgentSpawned { agent: RemoteAgentInfo },
    /// An agent was terminated on the peer.
    #[serde(rename = "agent_terminated")]
    AgentTerminated { agent_id: String },
    /// Peer is shutting down.
    #[serde(rename = "shutting_down")]
    ShuttingDown,
}

/// Information about a remote agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteAgentInfo {
    /// Agent ID (UUID string).
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// Description of what the agent does.
    pub description: String,
    /// Tags for categorization/discovery.
    pub tags: Vec<String>,
    /// Available tools.
    pub tools: Vec<String>,
    /// Current state.
    pub state: String,
}

/// Current protocol version. OpenFang expects 1.
pub const PROTOCOL_VERSION: u32 = 1;

/// Encode a wire message to bytes (4-byte big-endian length + JSON).
pub fn encode_message(msg: &WireMessage) -> Result<Vec<u8>, serde_json::Error> {
    let json = serde_json::to_vec(msg)?;
    let len = json.len() as u32;
    let mut bytes = Vec::with_capacity(4 + json.len());
    bytes.extend_from_slice(&len.to_be_bytes());
    bytes.extend_from_slice(&json);
    Ok(bytes)
}

/// Decode the length prefix from a 4-byte header.
pub fn decode_length(header: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*header)
}

/// Parse a JSON body into a WireMessage.
pub fn decode_message(body: &[u8]) -> Result<WireMessage, serde_json::Error> {
    serde_json::from_slice(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_decode_roundtrip() {
        let msg = WireMessage {
            id: "msg-1".to_string(),
            signature: None,
            seq_num: None,
            kind: WireMessageKind::Request(WireRequest::Ping),
        };
        let bytes = encode_message(&msg).unwrap();
        // First 4 bytes are length
        let len = decode_length(&[bytes[0], bytes[1], bytes[2], bytes[3]]);
        assert_eq!(len as usize, bytes.len() - 4);
        let decoded = decode_message(&bytes[4..]).unwrap();
        assert_eq!(decoded.id, "msg-1");
    }

    #[test]
    fn test_handshake_serialization_with_extensions() {
        let msg = WireMessage {
            id: "hs-1".to_string(),
            signature: None,
            seq_num: None,
            kind: WireMessageKind::Request(WireRequest::Handshake {
                node_id: "node-abc".to_string(),
                node_name: "autonoetic-kernel".to_string(),
                protocol_version: PROTOCOL_VERSION,
                agents: vec![RemoteAgentInfo {
                    id: "agent-1".to_string(),
                    name: "coder".to_string(),
                    description: "A coding agent".to_string(),
                    tags: vec!["code".to_string()],
                    tools: vec!["file_read".to_string()],
                    state: "running".to_string(),
                }],
                nonce: "test-nonce".to_string(),
                auth_hmac: "test-hmac".to_string(),
                extensions: Some(vec!["msg_hmac".to_string(), "resilience".to_string()]),
            }),
        };
        let json = serde_json::to_string_pretty(&msg).unwrap();
        assert!(json.contains("handshake"));
        assert!(json.contains("coder"));
        assert!(json.contains("msg_hmac")); // Extension is serialized

        let decoded: WireMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.id, "hs-1");

        if let WireMessageKind::Request(WireRequest::Handshake { extensions, .. }) = decoded.kind {
            assert!(extensions.is_some());
            assert_eq!(extensions.unwrap().len(), 2);
        } else {
            panic!("Wrong message kind");
        }
    }
}
