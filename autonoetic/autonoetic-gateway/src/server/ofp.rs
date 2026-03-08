//! OpenFang Protocol (OFP) Gateway Server.
//!
//! Handles TCP listener for incoming federated peers, length-prefixed framing,
//! and HMAC-SHA256 authenticated handshakes with optional extensions (msg_hmac).

use crate::server::registry::{PeerEntry, PeerRegistry, PeerState};
use autonoetic_ofp::wire::{
    decode_length, decode_message, encode_message, WireMessage, WireMessageKind, WireRequest,
    WireResponse, PROTOCOL_VERSION,
};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::net::SocketAddr;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tracing::{debug, error, info, warn};

type HmacSha256 = Hmac<Sha256>;

const MAX_MESSAGE_SIZE: u32 = 16 * 1024 * 1024; // 16 MB

/// Generate HMAC-SHA256 signature for message authentication.
pub fn hmac_sign(secret: &str, data: &[u8]) -> String {
    let mut mac =
        HmacSha256::new_from_slice(secret.as_bytes()).expect("HMAC can take key of any size");
    mac.update(data);
    hex::encode(mac.finalize().into_bytes())
}

/// Verify HMAC-SHA256 signature using constant-time comparison.
pub fn hmac_verify(secret: &str, data: &[u8], signature: impl AsRef<str>) -> bool {
    let expected = hmac_sign(secret, data);
    subtle::ConstantTimeEq::ct_eq(expected.as_bytes(), signature.as_ref().as_bytes()).into()
}

fn unsigned_wire_payload(msg: &WireMessage) -> anyhow::Result<Vec<u8>> {
    let mut unsigned = msg.clone();
    // signature is excluded from the signed payload to avoid self-reference.
    unsigned.signature = None;
    Ok(serde_json::to_vec(&unsigned)?)
}

/// Sign an OFP wire message envelope for `msg_hmac`.
pub fn sign_wire_message(secret: &str, msg: &WireMessage) -> anyhow::Result<String> {
    let payload = unsigned_wire_payload(msg)?;
    Ok(hmac_sign(secret, &payload))
}

/// Verify sequence and signature constraints for `msg_hmac`.
pub fn verify_wire_message(
    secret: &str,
    msg: &WireMessage,
    expected_seq: u64,
) -> anyhow::Result<()> {
    let actual_seq = msg
        .seq_num
        .ok_or_else(|| anyhow::anyhow!("Missing seq_num for msg_hmac-protected message"))?;
    if actual_seq != expected_seq {
        anyhow::bail!(
            "Invalid sequence number: expected {}, got {}",
            expected_seq,
            actual_seq
        );
    }

    let signature = msg
        .signature
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("Missing signature for msg_hmac-protected message"))?;
    let payload = unsigned_wire_payload(msg)?;
    if !hmac_verify(secret, &payload, signature) {
        anyhow::bail!("Invalid message signature");
    }

    Ok(())
}

/// Start the OFP TCP listener
pub async fn start_ofp_server(
    listen_addr: SocketAddr,
    node_id: String,
    node_name: String,
    shared_secret: String,
    registry: PeerRegistry,
    router: std::sync::Arc<crate::router::JsonRpcRouter>,
) -> anyhow::Result<()> {
    let listener = TcpListener::bind(listen_addr).await?;
    info!(
        "OFP Server listening on {} (node_id={})",
        listener.local_addr()?,
        node_id
    );

    loop {
        match listener.accept().await {
            Ok((stream, peer_addr)) => {
                debug!("OFP: accepted connection from {}", peer_addr);
                let node_id_clone = node_id.clone();
                let node_name_clone = node_name.clone();
                let secret_clone = shared_secret.clone();
                let registry_clone = registry.clone();
                let router_clone = router.clone();

                tokio::spawn(async move {
                    if let Err(e) = handle_inbound_connection(
                        stream,
                        peer_addr,
                        node_id_clone,
                        node_name_clone,
                        secret_clone,
                        registry_clone,
                        router_clone,
                    )
                    .await
                    {
                        warn!("OFP connection from {} closed: {}", peer_addr, e);
                    }
                });
            }
            Err(e) => {
                error!("OFP accept error: {}", e);
                tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
            }
        }
    }
}

async fn handle_inbound_connection(
    stream: TcpStream,
    peer_addr: SocketAddr,
    local_node_id: String,
    local_node_name: String,
    shared_secret: String,
    registry: PeerRegistry,
    router: std::sync::Arc<crate::router::JsonRpcRouter>,
) -> anyhow::Result<()> {
    let (mut reader, mut writer) = stream.into_split();

    // 1. Read the handshake request
    let msg = parse_ofp_response(&mut reader).await?;

    let (peer_node_id, peer_node_name, peer_protocol_version, peer_agents, peer_extensions) =
        match msg.kind {
            WireMessageKind::Request(WireRequest::Handshake {
                node_id,
                node_name,
                protocol_version,
                agents,
                nonce,
                auth_hmac,
                extensions,
            }) => {
                if protocol_version != PROTOCOL_VERSION {
                    let err = WireMessage {
                        id: msg.id.clone(),
                        signature: None,
                        seq_num: None,
                        kind: WireMessageKind::Response(WireResponse::Error {
                            code: 1,
                            message: format!("Version mismatch. Expected {}", PROTOCOL_VERSION),
                        }),
                    };
                    write_framed_message(&mut writer, &err).await?;
                    anyhow::bail!("Protocol version mismatch from {}", peer_addr);
                }

                // Verify HMAC
                let expected_data = format!("{}{}", nonce, node_id);
                if !hmac_verify(&shared_secret, expected_data.as_bytes(), &auth_hmac) {
                    let err = WireMessage {
                        id: msg.id.clone(),
                        signature: None,
                        seq_num: None,
                        kind: WireMessageKind::Response(WireResponse::Error {
                            code: 403,
                            message: "HMAC authentication failed".into(),
                        }),
                    };
                    write_framed_message(&mut writer, &err).await?;
                    anyhow::bail!("HMAC auth failed for {}", peer_addr);
                }

                info!(
                    "OFP: authenticated handshake via {} ({}) from {} [{} agents]",
                    node_name,
                    node_id,
                    peer_addr,
                    agents.len()
                );
                (
                    node_id,
                    node_name,
                    protocol_version,
                    agents,
                    extensions.unwrap_or_default(),
                )
            }
            _ => {
                let err = WireMessage {
                    id: msg.id.clone(),
                    signature: None,
                    seq_num: None,
                    kind: WireMessageKind::Response(WireResponse::Error {
                        code: 401,
                        message: "First message must be Handshake".into(),
                    }),
                };
                write_framed_message(&mut writer, &err).await?;
                anyhow::bail!("Unauthenticated connection attempt from {}", peer_addr);
            }
        };

    // 2. Compute agreed extensions
    let mut agreed_extensions = Vec::new();
    if peer_extensions.contains(&"msg_hmac".to_string()) {
        agreed_extensions.push("msg_hmac".to_string());
    }
    if peer_extensions.contains(&"resilience".to_string()) {
        agreed_extensions.push("resilience".to_string());
    }
    registry
        .add_peer(PeerEntry {
            node_id: peer_node_id.clone(),
            node_name: peer_node_name,
            address: peer_addr,
            agents: peer_agents,
            state: PeerState::Connected,
            connected_at: chrono::Utc::now(),
            protocol_version: peer_protocol_version,
            negotiated_extensions: agreed_extensions.clone(),
        })
        .await;

    // 3. Send HandshakeAck
    let ack_nonce = uuid::Uuid::new_v4().to_string();
    let ack_auth_data = format!("{}{}", ack_nonce, local_node_id);
    let ack_hmac = hmac_sign(&shared_secret, ack_auth_data.as_bytes());

    let ack = WireMessage {
        id: msg.id,
        signature: None,
        seq_num: None,
        kind: WireMessageKind::Response(WireResponse::HandshakeAck {
            node_id: local_node_id,
            node_name: local_node_name,
            protocol_version: PROTOCOL_VERSION,
            agents: vec![], // TODO: populate from Gateway state
            nonce: ack_nonce,
            auth_hmac: ack_hmac,
            extensions: if agreed_extensions.is_empty() {
                None
            } else {
                Some(agreed_extensions.clone())
            },
        }),
    };
    write_framed_message(&mut writer, &ack).await?;

    // 4. Enter connection loop
    let use_msg_hmac = agreed_extensions.contains(&"msg_hmac".to_string());
    if use_msg_hmac {
        info!("OFP: msg_hmac extension enabled for {}", peer_node_id);
    }
    let mut expected_inbound_seq: u64 = 1;
    let mut outbound_seq: u64 = 1;

    loop {
        let req = match parse_ofp_response(&mut reader).await {
            Ok(m) => m,
            Err(e) => {
                debug!("OFP peer {} disconnected: {}", peer_node_id, e);
                break;
            }
        };

        if use_msg_hmac {
            if let Err(e) = verify_wire_message(&shared_secret, &req, expected_inbound_seq) {
                let err = WireMessage {
                    id: req.id.clone(),
                    signature: None,
                    seq_num: None,
                    kind: WireMessageKind::Response(WireResponse::Error {
                        code: 403,
                        message: format!("Invalid msg_hmac envelope: {}", e),
                    }),
                };
                write_framed_message(&mut writer, &err).await?;
                registry.mark_disconnected(&peer_node_id).await;
                anyhow::bail!("Invalid msg_hmac message from {}: {}", peer_node_id, e);
            }
            expected_inbound_seq += 1;
        }

        // Handle Ping, Discover, AgentMessage...
        match &req.kind {
            WireMessageKind::Request(WireRequest::Ping) => {
                let mut resp = WireMessage {
                    id: req.id.clone(),
                    signature: None,
                    seq_num: None,
                    kind: WireMessageKind::Response(WireResponse::Pong { uptime_secs: 1 }), // TODO: real uptime
                };
                if use_msg_hmac {
                    resp.seq_num = Some(outbound_seq);
                    resp.signature = Some(sign_wire_message(&shared_secret, &resp)?);
                    outbound_seq += 1;
                }
                write_framed_message(&mut writer, &resp).await?;
            }
            WireMessageKind::Request(WireRequest::AgentMessage {
                agent,
                message,
                sender,
            }) => {
                let session_id = uuid::Uuid::new_v4().to_string();
                let mut resp = match sender.as_deref() {
                    Some(sender_agent)
                        if registry.peer_hosts_agent(&peer_node_id, sender_agent).await =>
                    {
                        match router
                            .spawn_agent_once(&agent, &message, &session_id, None, true, None)
                            .await
                        {
                            Ok(result) => {
                                let text = result.assistant_reply.unwrap_or_default();
                                WireMessage {
                                    id: req.id.clone(),
                                    signature: None,
                                    seq_num: None,
                                    kind: WireMessageKind::Response(WireResponse::AgentResponse {
                                        text,
                                    }),
                                }
                            }
                            Err(e) => WireMessage {
                                id: req.id.clone(),
                                signature: None,
                                seq_num: None,
                                kind: WireMessageKind::Response(WireResponse::Error {
                                    code: 500,
                                    message: format!("Agent spawn failed: {}", e),
                                }),
                            },
                        }
                    }
                    Some(sender_agent) => WireMessage {
                        id: req.id.clone(),
                        signature: None,
                        seq_num: None,
                        kind: WireMessageKind::Response(WireResponse::Error {
                            code: 403,
                            message: format!(
                                "Sender '{}' is not advertised by authenticated peer '{}'",
                                sender_agent, peer_node_id
                            ),
                        }),
                    },
                    None => WireMessage {
                        id: req.id.clone(),
                        signature: None,
                        seq_num: None,
                        kind: WireMessageKind::Response(WireResponse::Error {
                            code: 400,
                            message: "AgentMessage sender is required for federated delivery"
                                .into(),
                        }),
                    },
                };

                if use_msg_hmac {
                    resp.seq_num = Some(outbound_seq);
                    resp.signature = Some(sign_wire_message(&shared_secret, &resp)?);
                    outbound_seq += 1;
                }
                write_framed_message(&mut writer, &resp).await?;
            }
            // For now, return Error on everything else
            WireMessageKind::Request(_) => {
                let mut resp = WireMessage {
                    id: req.id.clone(),
                    signature: None,
                    seq_num: None,
                    kind: WireMessageKind::Response(WireResponse::Error {
                        code: 501,
                        message: "Not Implemented".into(),
                    }),
                };
                if use_msg_hmac {
                    resp.seq_num = Some(outbound_seq);
                    resp.signature = Some(sign_wire_message(&shared_secret, &resp)?);
                    outbound_seq += 1;
                }
                write_framed_message(&mut writer, &resp).await?;
            }
            _ => {}
        }
    }
    registry.mark_disconnected(&peer_node_id).await;

    Ok(())
}

/// Read exactly 4 bytes length, then that many bytes of JSON payload.
pub async fn read_framed_message(
    reader: &mut tokio::net::tcp::OwnedReadHalf,
) -> anyhow::Result<WireMessage> {
    let mut header = [0u8; 4];
    reader.read_exact(&mut header).await?;

    let len = decode_length(&header);
    if len > MAX_MESSAGE_SIZE {
        anyhow::bail!(
            "Message too large: {} exceeds limit {}",
            len,
            MAX_MESSAGE_SIZE
        );
    }

    let mut body = vec![0u8; len as usize];
    reader.read_exact(&mut body).await?;

    Ok(decode_message(&body)?)
}

/// Backward-compatible alias for older call sites.
pub async fn parse_ofp_response(
    reader: &mut tokio::net::tcp::OwnedReadHalf,
) -> anyhow::Result<WireMessage> {
    read_framed_message(reader).await
}

/// Encode JSON payload, prepend 4-byte length, and write to socket.
pub async fn write_framed_message(
    writer: &mut tokio::net::tcp::OwnedWriteHalf,
    msg: &WireMessage,
) -> anyhow::Result<()> {
    let bytes = encode_message(msg)?;
    writer.write_all(&bytes).await?;
    writer.flush().await?;
    Ok(())
}
