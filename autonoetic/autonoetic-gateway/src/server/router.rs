//! Gateway Message Router.
//!
//! Handles JSON-RPC messages from agents. Routes `ecosystem.send_message`
//! locally or transparently over OFP federation.

use crate::server::ofp::{
    hmac_sign, hmac_verify, parse_ofp_response, sign_wire_message, verify_wire_message,
    write_framed_message,
};
use crate::server::registry::PeerRegistry;
use autonoetic_ofp::wire::{
    WireMessage, WireMessageKind, WireRequest, WireResponse, PROTOCOL_VERSION,
};
use tokio::net::TcpStream;
use tracing::{debug, info};

pub struct MessageRouter {
    registry: PeerRegistry,
    node_id: String,
    shared_secret: String,
}

impl MessageRouter {
    pub fn new(registry: PeerRegistry, node_id: String, shared_secret: String) -> Self {
        Self {
            registry,
            node_id,
            shared_secret,
        }
    }

    /// Route an `ecosystem.send_message` outgoing call from a local agent.
    pub async fn route_send_message(
        &self,
        sender_agent_id: &str,
        target_agent_id: &str,
        message: &str,
    ) -> anyhow::Result<String> {
        // 1. Check if the target is remote via PeerRegistry
        if let Some(peer_node_id) = self.registry.resolve_agent_node(target_agent_id).await {
            info!(
                "Routing message from {} to remote agent {} (on node {})",
                sender_agent_id, target_agent_id, peer_node_id
            );
            return self
                .send_via_ofp(&peer_node_id, target_agent_id, message, sender_agent_id)
                .await;
        }

        // 2. Fallback to local routing
        info!(
            "Routing message from {} to local agent {}",
            sender_agent_id, target_agent_id
        );

        // TODO: Actually deliver to local agent's session inbox
        Ok("Delivered locally (stub)".to_string())
    }

    /// Forward a message to a remote peer via OFP TCP connection.
    async fn send_via_ofp(
        &self,
        peer_node_id: &str,
        target_agent: &str,
        message: &str,
        sender_agent: &str,
    ) -> anyhow::Result<String> {
        let peer = self
            .registry
            .get_peer(peer_node_id)
            .await
            .ok_or_else(|| anyhow::anyhow!("Peer {} dropped from registry", peer_node_id))?;

        debug!(
            "Connecting to OFP peer {} at {}",
            peer_node_id, peer.address
        );
        let stream = TcpStream::connect(peer.address).await?;
        let (mut reader, mut writer) = stream.into_split();

        // 1. Send Handshake
        let nonce = uuid::Uuid::new_v4().to_string();
        let auth_data = format!("{}{}", nonce, self.node_id);
        let auth_hmac = hmac_sign(&self.shared_secret, auth_data.as_bytes());

        let handshake = WireMessage {
            id: uuid::Uuid::new_v4().to_string(),
            signature: None,
            seq_num: None,
            kind: WireMessageKind::Request(WireRequest::Handshake {
                node_id: self.node_id.clone(),
                node_name: "autonoetic-router".into(), // TODO: use actual node name
                protocol_version: PROTOCOL_VERSION,
                agents: vec![], // Router connection doesn't strictly need to advertise here
                nonce,
                auth_hmac,
                extensions: Some(vec!["msg_hmac".into()]),
            }),
        };
        write_framed_message(&mut writer, &handshake).await?;

        // Wait for HandshakeAck
        let ack = parse_ofp_response(&mut reader).await?;
        let (ack_node_id, ack_protocol_version, ack_nonce, ack_auth_hmac, ack_extensions) =
            match ack.kind {
                WireMessageKind::Response(WireResponse::HandshakeAck {
                    node_id,
                    protocol_version,
                    nonce,
                    auth_hmac,
                    extensions,
                    ..
                }) => (node_id, protocol_version, nonce, auth_hmac, extensions),
                WireMessageKind::Response(WireResponse::Error { code, message }) => {
                    anyhow::bail!("Handshake failed: [{}]: {}", code, message);
                }
                _ => anyhow::bail!("Expected HandshakeAck"),
            };

        if ack_protocol_version != PROTOCOL_VERSION {
            anyhow::bail!(
                "Peer protocol mismatch: expected {}, got {}",
                PROTOCOL_VERSION,
                ack_protocol_version
            );
        }
        let ack_expected_data = format!("{}{}", ack_nonce, ack_node_id);
        if !hmac_verify(
            &self.shared_secret,
            ack_expected_data.as_bytes(),
            &ack_auth_hmac,
        ) {
            anyhow::bail!(
                "HandshakeAck HMAC verification failed for peer {}",
                peer_node_id
            );
        }
        let negotiated_extensions = ack_extensions.unwrap_or_default();
        let use_msg_hmac = negotiated_extensions
            .iter()
            .any(|ext| ext.eq_ignore_ascii_case("msg_hmac"));

        // 2. Send AgentMessage
        let mut agent_msg = WireMessage {
            id: uuid::Uuid::new_v4().to_string(),
            signature: None,
            seq_num: None,
            kind: WireMessageKind::Request(WireRequest::AgentMessage {
                agent: target_agent.to_string(),
                message: message.to_string(),
                sender: Some(sender_agent.to_string()),
            }),
        };
        if use_msg_hmac {
            agent_msg.seq_num = Some(1);
            agent_msg.signature = Some(sign_wire_message(&self.shared_secret, &agent_msg)?);
        }
        write_framed_message(&mut writer, &agent_msg).await?;

        // 3. Wait for AgentResponse
        let resp = parse_ofp_response(&mut reader).await?;
        if use_msg_hmac {
            verify_wire_message(&self.shared_secret, &resp, 1)?;
        }
        match resp.kind {
            WireMessageKind::Response(WireResponse::AgentResponse { text }) => Ok(text),
            WireMessageKind::Response(WireResponse::Error { code, message }) => {
                anyhow::bail!("Agent error [{}]: {}", code, message);
            }
            _ => anyhow::bail!("Expected AgentResponse"),
        }
    }
}
