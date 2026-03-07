//! Integration tests for the OFP (OpenFang Protocol) implementation.
//!
//! Run with:
//!   cargo test -p autonoetic-gateway --test ofp_integration -- --nocapture

use autonoetic_gateway::server::ofp::{
    hmac_sign, hmac_verify, read_framed_message, start_ofp_server, write_framed_message,
};
use autonoetic_ofp::wire::*;
use tokio::net::TcpListener;
use tokio::time::Duration;

// ═══════════════════════════════════════════════════════════════════════
// 1. Wire-level compatibility with OpenFang
// ═══════════════════════════════════════════════════════════════════════

/// Verify our WireMessage JSON is byte-for-byte parseable by OpenFang's serde-based parser.
/// OpenFang expects { "id": "...", "type": "request", "method": "ping" }.
#[test]
fn test_wire_compat_ping_json_shape() {
    let msg = WireMessage {
        id: "test-1".to_string(),
        signature: None,
        seq_num: None,
        kind: WireMessageKind::Request(WireRequest::Ping),
    };
    let json: serde_json::Value = serde_json::to_value(&msg).unwrap();

    // Must have these exact fields
    assert_eq!(json["id"], "test-1");
    assert_eq!(json["type"], "request");
    assert_eq!(json["method"], "ping");

    // Must NOT have signature/seq_num when None (skip_serializing_if)
    assert!(json.get("signature").is_none());
    assert!(json.get("seq_num").is_none());
}

/// Verify handshake JSON matches OpenFang's exact shape.
#[test]
fn test_wire_compat_handshake_json_shape() {
    let msg = WireMessage {
        id: "hs-1".to_string(),
        signature: None,
        seq_num: None,
        kind: WireMessageKind::Request(WireRequest::Handshake {
            node_id: "node-abc".to_string(),
            node_name: "test-kernel".to_string(),
            protocol_version: 1,
            agents: vec![RemoteAgentInfo {
                id: "a-1".to_string(),
                name: "echo".to_string(),
                description: "Echo agent".to_string(),
                tags: vec!["test".to_string()],
                tools: vec![],
                state: "running".to_string(),
            }],
            nonce: "nonce-123".to_string(),
            auth_hmac: "hmac-abc".to_string(),
            extensions: None, // No extensions → vanilla OpenFang compat
        }),
    };
    let json: serde_json::Value = serde_json::to_value(&msg).unwrap();

    assert_eq!(json["type"], "request");
    assert_eq!(json["method"], "handshake");
    assert_eq!(json["node_id"], "node-abc");
    assert_eq!(json["protocol_version"], 1);
    assert_eq!(json["agents"][0]["name"], "echo");
    assert_eq!(json["nonce"], "nonce-123");
    assert_eq!(json["auth_hmac"], "hmac-abc");
    // extensions should be absent when None
    assert!(json.get("extensions").is_none());
}

/// Verify our extensions are present when set, but the message is still
/// parseable by a standard serde parser that ignores unknown fields.
#[test]
fn test_wire_compat_extensions_graceful_degradation() {
    let msg = WireMessage {
        id: "hs-2".to_string(),
        signature: Some("sig-xyz".to_string()),
        seq_num: Some(42),
        kind: WireMessageKind::Request(WireRequest::Handshake {
            node_id: "autonoetic-1".to_string(),
            node_name: "my-gateway".to_string(),
            protocol_version: 1,
            agents: vec![],
            nonce: "nonce".to_string(),
            auth_hmac: "hmac".to_string(),
            extensions: Some(vec!["msg_hmac".to_string()]),
        }),
    };
    let json_str = serde_json::to_string(&msg).unwrap();

    // Extensions are in the JSON
    assert!(json_str.contains("\"signature\":\"sig-xyz\""));
    assert!(json_str.contains("\"seq_num\":42"));
    assert!(json_str.contains("\"extensions\":[\"msg_hmac\"]"));

    // A vanilla OpenFang node would parse this with default serde (no deny_unknown_fields).
    // Simulate by deserializing back — extra fields are silently preserved in our struct.
    let decoded: WireMessage = serde_json::from_str(&json_str).unwrap();
    assert_eq!(decoded.signature.as_deref(), Some("sig-xyz"));
    assert_eq!(decoded.seq_num, Some(42));
}

/// Verify notification JSON shape for agent lifecycle events.
#[test]
fn test_wire_compat_notification_shape() {
    let msg = WireMessage {
        id: "n-1".to_string(),
        signature: None,
        seq_num: None,
        kind: WireMessageKind::Notification(WireNotification::AgentSpawned {
            agent: RemoteAgentInfo {
                id: "a-1".to_string(),
                name: "coder".to_string(),
                description: "Coding agent".to_string(),
                tags: vec![],
                tools: vec!["file_read".to_string()],
                state: "running".to_string(),
            },
        }),
    };
    let json: serde_json::Value = serde_json::to_value(&msg).unwrap();

    assert_eq!(json["type"], "notification");
    assert_eq!(json["event"], "agent_spawned");
    assert_eq!(json["agent"]["name"], "coder");
}

/// Verify error response shape.
#[test]
fn test_wire_compat_error_response_shape() {
    let msg = WireMessage {
        id: "err-1".to_string(),
        signature: None,
        seq_num: None,
        kind: WireMessageKind::Response(WireResponse::Error {
            code: 403,
            message: "HMAC authentication failed".to_string(),
        }),
    };
    let json: serde_json::Value = serde_json::to_value(&msg).unwrap();

    assert_eq!(json["type"], "response");
    assert_eq!(json["method"], "error");
    assert_eq!(json["code"], 403);
    assert_eq!(json["message"], "HMAC authentication failed");
}

// ═══════════════════════════════════════════════════════════════════════
// 2. HMAC-SHA256 Authentication
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn test_hmac_sign_deterministic() {
    let sig1 = hmac_sign("secret", b"nonce123node-abc");
    let sig2 = hmac_sign("secret", b"nonce123node-abc");
    assert_eq!(sig1, sig2);
    assert!(!sig1.is_empty());
    // HMAC-SHA256 produces 64 hex chars
    assert_eq!(sig1.len(), 64);
}

#[test]
fn test_hmac_verify_correct_secret() {
    let sig = hmac_sign("my-secret", b"test-data");
    assert!(hmac_verify("my-secret", b"test-data", &sig));
}

#[test]
fn test_hmac_verify_wrong_secret_fails() {
    let sig = hmac_sign("correct-secret", b"test-data");
    assert!(!hmac_verify("wrong-secret", b"test-data", &sig));
}

#[test]
fn test_hmac_verify_tampered_data_fails() {
    let sig = hmac_sign("secret", b"original-data");
    assert!(!hmac_verify("secret", b"tampered-data", &sig));
}

#[test]
fn test_hmac_matches_openfang_format() {
    // OpenFang computes HMAC-SHA256(shared_secret, nonce + node_id).
    let secret = "test-shared-secret";
    let nonce = "random-nonce-xyz";
    let node_id = "node-abc";
    let data = format!("{}{}", nonce, node_id);
    let sig = hmac_sign(secret, data.as_bytes());

    // Must be valid hex and 64 chars
    assert_eq!(sig.len(), 64);
    assert!(sig.chars().all(|c| c.is_ascii_hexdigit()));

    // Verify passes
    assert!(hmac_verify(secret, data.as_bytes(), &sig));
}

// ═══════════════════════════════════════════════════════════════════════
// 3. Length-prefixed framing (encode/decode)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn test_encode_length_prefix_correct() {
    let msg = WireMessage {
        id: "f-1".to_string(),
        signature: None,
        seq_num: None,
        kind: WireMessageKind::Request(WireRequest::Ping),
    };
    let bytes = encode_message(&msg).unwrap();

    // First 4 bytes = big-endian u32 length of JSON
    let len = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    assert_eq!(len as usize, bytes.len() - 4);

    // The rest is valid JSON
    let json: serde_json::Value = serde_json::from_slice(&bytes[4..]).unwrap();
    assert_eq!(json["id"], "f-1");
}

#[test]
fn test_encode_decode_all_message_types() {
    let messages = vec![
        WireMessage {
            id: "1".into(),
            signature: None,
            seq_num: None,
            kind: WireMessageKind::Request(WireRequest::Ping),
        },
        WireMessage {
            id: "2".into(),
            signature: None,
            seq_num: None,
            kind: WireMessageKind::Response(WireResponse::Pong { uptime_secs: 42 }),
        },
        WireMessage {
            id: "3".into(),
            signature: None,
            seq_num: None,
            kind: WireMessageKind::Request(WireRequest::Discover {
                query: "security".into(),
            }),
        },
        WireMessage {
            id: "4".into(),
            signature: None,
            seq_num: None,
            kind: WireMessageKind::Request(WireRequest::AgentMessage {
                agent: "coder".into(),
                message: "Hello".into(),
                sender: Some("orchestrator".into()),
            }),
        },
        WireMessage {
            id: "5".into(),
            signature: None,
            seq_num: None,
            kind: WireMessageKind::Response(WireResponse::AgentResponse { text: "Hi".into() }),
        },
        WireMessage {
            id: "6".into(),
            signature: None,
            seq_num: None,
            kind: WireMessageKind::Notification(WireNotification::ShuttingDown),
        },
    ];

    for msg in &messages {
        let bytes = encode_message(msg).unwrap();
        let len = decode_length(&[bytes[0], bytes[1], bytes[2], bytes[3]]);
        let decoded = decode_message(&bytes[4..]).unwrap();
        assert_eq!(decoded.id, msg.id);
        assert_eq!(len as usize, bytes.len() - 4);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 4. TCP handshake integration test (real sockets)
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_tcp_handshake_success() {
    let shared_secret = "test-secret-key-42";

    // Start server on random port
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let server_addr = listener.local_addr().unwrap();

    let server_handle = tokio::spawn({
        let secret = shared_secret.to_string();
        async move {
            let (stream, _) = listener.accept().await.unwrap();
            let (mut reader, mut writer) = stream.into_split();

            // Read handshake
            let msg = read_framed_message(&mut reader).await.unwrap();
            match msg.kind {
                WireMessageKind::Request(WireRequest::Handshake {
                    node_id,
                    nonce,
                    auth_hmac,
                    ..
                }) => {
                    // Verify client HMAC
                    let expected_data = format!("{}{}", nonce, node_id);
                    assert!(hmac_verify(&secret, expected_data.as_bytes(), &auth_hmac));

                    // Send ack
                    let ack_nonce = "server-nonce".to_string();
                    let ack_data = format!("{}server-node", ack_nonce);
                    let ack_hmac = hmac_sign(&secret, ack_data.as_bytes());

                    let ack = WireMessage {
                        id: msg.id,
                        signature: None,
                        seq_num: None,
                        kind: WireMessageKind::Response(WireResponse::HandshakeAck {
                            node_id: "server-node".to_string(),
                            node_name: "test-server".to_string(),
                            protocol_version: PROTOCOL_VERSION,
                            agents: vec![],
                            nonce: ack_nonce,
                            auth_hmac: ack_hmac,
                            extensions: None,
                        }),
                    };
                    write_framed_message(&mut writer, &ack).await.unwrap();
                }
                _ => panic!("Expected handshake request"),
            }

            // Read ping
            let ping = read_framed_message(&mut reader).await.unwrap();
            match &ping.kind {
                WireMessageKind::Request(WireRequest::Ping) => {}
                _ => panic!("Expected ping"),
            }

            // Send pong
            let pong = WireMessage {
                id: ping.id,
                signature: None,
                seq_num: None,
                kind: WireMessageKind::Response(WireResponse::Pong { uptime_secs: 99 }),
            };
            write_framed_message(&mut writer, &pong).await.unwrap();
        }
    });

    // Client side
    let stream = tokio::net::TcpStream::connect(server_addr).await.unwrap();
    let (mut reader, mut writer) = stream.into_split();

    // Send handshake
    let nonce = "client-nonce".to_string();
    let node_id = "client-node".to_string();
    let auth_data = format!("{}{}", nonce, node_id);
    let auth_hmac = hmac_sign(shared_secret, auth_data.as_bytes());

    let handshake = WireMessage {
        id: "hs-test".to_string(),
        signature: None,
        seq_num: None,
        kind: WireMessageKind::Request(WireRequest::Handshake {
            node_id: node_id.clone(),
            node_name: "test-client".to_string(),
            protocol_version: PROTOCOL_VERSION,
            agents: vec![],
            nonce,
            auth_hmac,
            extensions: None,
        }),
    };
    write_framed_message(&mut writer, &handshake).await.unwrap();

    // Read ack
    let ack = read_framed_message(&mut reader).await.unwrap();
    match &ack.kind {
        WireMessageKind::Response(WireResponse::HandshakeAck {
            node_id,
            nonce,
            auth_hmac,
            ..
        }) => {
            assert_eq!(node_id, "server-node");
            // Verify server's HMAC
            let expected_data = format!("{}{}", nonce, node_id);
            assert!(hmac_verify(
                shared_secret,
                expected_data.as_bytes(),
                auth_hmac
            ));
        }
        _ => panic!("Expected HandshakeAck, got {:?}", ack.kind),
    }

    // Send ping
    let ping = WireMessage {
        id: "ping-1".to_string(),
        signature: None,
        seq_num: None,
        kind: WireMessageKind::Request(WireRequest::Ping),
    };
    write_framed_message(&mut writer, &ping).await.unwrap();

    // Read pong
    let pong = read_framed_message(&mut reader).await.unwrap();
    match pong.kind {
        WireMessageKind::Response(WireResponse::Pong { uptime_secs }) => {
            assert_eq!(uptime_secs, 99);
        }
        _ => panic!("Expected Pong"),
    }

    server_handle.await.unwrap();
}

#[tokio::test]
async fn test_tcp_handshake_bad_hmac_rejected() {
    let server_secret = "correct-secret";

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let server_addr = listener.local_addr().unwrap();

    let server_handle = tokio::spawn({
        let secret = server_secret.to_string();
        async move {
            let (stream, _) = listener.accept().await.unwrap();
            let (mut reader, mut writer) = stream.into_split();

            let msg = read_framed_message(&mut reader).await.unwrap();
            match msg.kind {
                WireMessageKind::Request(WireRequest::Handshake {
                    node_id,
                    nonce,
                    auth_hmac,
                    ..
                }) => {
                    let expected_data = format!("{}{}", nonce, node_id);
                    let valid = hmac_verify(&secret, expected_data.as_bytes(), &auth_hmac);
                    assert!(!valid, "HMAC should fail with wrong secret");

                    // Send error
                    let err = WireMessage {
                        id: msg.id,
                        signature: None,
                        seq_num: None,
                        kind: WireMessageKind::Response(WireResponse::Error {
                            code: 403,
                            message: "HMAC authentication failed".to_string(),
                        }),
                    };
                    write_framed_message(&mut writer, &err).await.unwrap();
                }
                _ => panic!("Expected handshake"),
            }
        }
    });

    // Client uses wrong secret
    let stream = tokio::net::TcpStream::connect(server_addr).await.unwrap();
    let (mut reader, mut writer) = stream.into_split();

    let nonce = "nonce".to_string();
    let node_id = "bad-client".to_string();
    let auth_data = format!("{}{}", nonce, node_id);
    let auth_hmac = hmac_sign("WRONG-SECRET", auth_data.as_bytes());

    let handshake = WireMessage {
        id: "hs-bad".to_string(),
        signature: None,
        seq_num: None,
        kind: WireMessageKind::Request(WireRequest::Handshake {
            node_id,
            node_name: "bad-client".to_string(),
            protocol_version: PROTOCOL_VERSION,
            agents: vec![],
            nonce,
            auth_hmac,
            extensions: None,
        }),
    };
    write_framed_message(&mut writer, &handshake).await.unwrap();

    // Should get error response
    let resp = read_framed_message(&mut reader).await.unwrap();
    match resp.kind {
        WireMessageKind::Response(WireResponse::Error { code, .. }) => {
            assert_eq!(code, 403);
        }
        _ => panic!("Expected Error response, got {:?}", resp.kind),
    }

    server_handle.await.unwrap();
}

// ═══════════════════════════════════════════════════════════════════════
// 5. Extension negotiation
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_tcp_extension_negotiation() {
    let secret = "ext-secret";

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let server_addr = listener.local_addr().unwrap();

    let server_handle = tokio::spawn({
        let secret = secret.to_string();
        async move {
            let (stream, _) = listener.accept().await.unwrap();
            let (mut reader, mut writer) = stream.into_split();

            let msg = read_framed_message(&mut reader).await.unwrap();
            match msg.kind {
                WireMessageKind::Request(WireRequest::Handshake {
                    node_id,
                    nonce,
                    auth_hmac,
                    extensions,
                    ..
                }) => {
                    // Verify HMAC
                    let data = format!("{}{}", nonce, node_id);
                    assert!(hmac_verify(&secret, data.as_bytes(), &auth_hmac));

                    // Client requested msg_hmac → agree to it
                    let client_exts = extensions.unwrap_or_default();
                    let mut agreed = Vec::new();
                    if client_exts.contains(&"msg_hmac".to_string()) {
                        agreed.push("msg_hmac".to_string());
                    }
                    // Don't agree to "resilience" even if requested (simulate partial support)

                    let ack_nonce = "srv-nonce".to_string();
                    let ack_data = format!("{}server-ext", ack_nonce);
                    let ack_hmac_val = hmac_sign(&secret, ack_data.as_bytes());

                    let ack = WireMessage {
                        id: msg.id,
                        signature: None,
                        seq_num: None,
                        kind: WireMessageKind::Response(WireResponse::HandshakeAck {
                            node_id: "server-ext".to_string(),
                            node_name: "ext-server".to_string(),
                            protocol_version: PROTOCOL_VERSION,
                            agents: vec![],
                            nonce: ack_nonce,
                            auth_hmac: ack_hmac_val,
                            extensions: if agreed.is_empty() {
                                None
                            } else {
                                Some(agreed)
                            },
                        }),
                    };
                    write_framed_message(&mut writer, &ack).await.unwrap();
                }
                _ => panic!("Expected handshake"),
            }
        }
    });

    let stream = tokio::net::TcpStream::connect(server_addr).await.unwrap();
    let (mut reader, mut writer) = stream.into_split();

    let nonce = "cli-nonce".to_string();
    let node_id = "client-ext".to_string();
    let auth_data = format!("{}{}", nonce, node_id);
    let auth_hmac = hmac_sign(secret, auth_data.as_bytes());

    // Request both msg_hmac and resilience
    let handshake = WireMessage {
        id: "hs-ext".to_string(),
        signature: None,
        seq_num: None,
        kind: WireMessageKind::Request(WireRequest::Handshake {
            node_id,
            node_name: "ext-client".to_string(),
            protocol_version: PROTOCOL_VERSION,
            agents: vec![],
            nonce,
            auth_hmac,
            extensions: Some(vec!["msg_hmac".to_string(), "resilience".to_string()]),
        }),
    };
    write_framed_message(&mut writer, &handshake).await.unwrap();

    let ack = read_framed_message(&mut reader).await.unwrap();
    match &ack.kind {
        WireMessageKind::Response(WireResponse::HandshakeAck { extensions, .. }) => {
            let exts = extensions.as_ref().expect("Should have extensions");
            // Server agreed to msg_hmac but NOT resilience
            assert!(exts.contains(&"msg_hmac".to_string()));
            assert!(!exts.contains(&"resilience".to_string()));
        }
        _ => panic!("Expected HandshakeAck"),
    }

    server_handle.await.unwrap();
}

// ═══════════════════════════════════════════════════════════════════════
// 6. PeerRegistry tests
// ═══════════════════════════════════════════════════════════════════════

use autonoetic_gateway::server::registry::{PeerEntry, PeerRegistry, PeerState};

#[tokio::test]
async fn test_registry_add_and_get() {
    let reg = PeerRegistry::new();
    reg.add_peer(PeerEntry {
        node_id: "node-1".to_string(),
        node_name: "peer-alpha".to_string(),
        address: "127.0.0.1:9001".parse().unwrap(),
        agents: vec![RemoteAgentInfo {
            id: "a-1".into(),
            name: "echo".into(),
            description: "".into(),
            tags: vec![],
            tools: vec![],
            state: "running".into(),
        }],
        state: PeerState::Connected,
        connected_at: chrono::Utc::now(),
        protocol_version: 1,
        negotiated_extensions: vec![],
    })
    .await;

    let peer = reg.get_peer("node-1").await.unwrap();
    assert_eq!(peer.node_name, "peer-alpha");
    assert_eq!(peer.agents.len(), 1);
}

#[tokio::test]
async fn test_registry_connected_peers_filters() {
    let reg = PeerRegistry::new();
    reg.add_peer(PeerEntry {
        node_id: "n-1".into(),
        node_name: "connected".into(),
        address: "127.0.0.1:9001".parse().unwrap(),
        agents: vec![],
        state: PeerState::Connected,
        connected_at: chrono::Utc::now(),
        protocol_version: 1,
        negotiated_extensions: vec![],
    })
    .await;
    reg.add_peer(PeerEntry {
        node_id: "n-2".into(),
        node_name: "disconnected".into(),
        address: "127.0.0.1:9002".parse().unwrap(),
        agents: vec![],
        state: PeerState::Disconnected,
        connected_at: chrono::Utc::now(),
        protocol_version: 1,
        negotiated_extensions: vec![],
    })
    .await;

    let connected = reg.connected_peers().await;
    assert_eq!(connected.len(), 1);
    assert_eq!(connected[0].node_id, "n-1");
}

#[tokio::test]
async fn test_registry_resolve_agent_node() {
    let reg = PeerRegistry::new();
    reg.add_peer(PeerEntry {
        node_id: "n-1".into(),
        node_name: "peer-1".into(),
        address: "127.0.0.1:9001".parse().unwrap(),
        agents: vec![RemoteAgentInfo {
            id: "agent-x".into(),
            name: "coder".into(),
            description: "".into(),
            tags: vec![],
            tools: vec![],
            state: "running".into(),
        }],
        state: PeerState::Connected,
        connected_at: chrono::Utc::now(),
        protocol_version: 1,
        negotiated_extensions: vec![],
    })
    .await;

    // Find by ID
    assert_eq!(
        reg.resolve_agent_node("agent-x").await.as_deref(),
        Some("n-1")
    );
    // Find by name
    assert_eq!(
        reg.resolve_agent_node("coder").await.as_deref(),
        Some("n-1")
    );
    // Not found
    assert!(reg.resolve_agent_node("nonexistent").await.is_none());
}

#[tokio::test]
async fn test_registry_mark_disconnected() {
    let reg = PeerRegistry::new();
    reg.add_peer(PeerEntry {
        node_id: "n-1".into(),
        node_name: "p".into(),
        address: "127.0.0.1:9001".parse().unwrap(),
        agents: vec![],
        state: PeerState::Connected,
        connected_at: chrono::Utc::now(),
        protocol_version: 1,
        negotiated_extensions: vec![],
    })
    .await;

    reg.mark_disconnected("n-1").await;

    let peer = reg.get_peer("n-1").await.unwrap();
    assert_eq!(peer.state, PeerState::Disconnected);
    assert!(reg.connected_peers().await.is_empty());
}

#[tokio::test]
async fn test_registry_add_remove_agent() {
    let reg = PeerRegistry::new();
    reg.add_peer(PeerEntry {
        node_id: "n-1".into(),
        node_name: "p".into(),
        address: "127.0.0.1:9001".parse().unwrap(),
        agents: vec![],
        state: PeerState::Connected,
        connected_at: chrono::Utc::now(),
        protocol_version: 1,
        negotiated_extensions: vec![],
    })
    .await;

    // Add an agent dynamically (AgentSpawned notification)
    reg.add_agent(
        "n-1",
        RemoteAgentInfo {
            id: "a-new".into(),
            name: "researcher".into(),
            description: "".into(),
            tags: vec![],
            tools: vec![],
            state: "running".into(),
        },
    )
    .await;

    let peer = reg.get_peer("n-1").await.unwrap();
    assert_eq!(peer.agents.len(), 1);
    assert_eq!(peer.agents[0].name, "researcher");

    // Remove agent (AgentTerminated notification)
    reg.remove_agent("n-1", "a-new").await;
    let peer = reg.get_peer("n-1").await.unwrap();
    assert!(peer.agents.is_empty());
}

#[tokio::test]
async fn test_registry_lifecycle_via_real_ofp_server() {
    let reg = PeerRegistry::new();
    let shared_secret = "registry-live-secret".to_string();

    // Reserve a free local port, then start OFP server on it.
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let server_addr = listener.local_addr().unwrap();
    drop(listener);

    // Create a dummy router for the test
    let config = autonoetic_types::config::GatewayConfig::default();
    let router = std::sync::Arc::new(autonoetic_gateway::router::JsonRpcRouter::new(config));

    let server = tokio::spawn(start_ofp_server(
        server_addr,
        "server-node".to_string(),
        "registry-server".to_string(),
        shared_secret.clone(),
        reg.clone(),
        router,
    ));

    // Give the listener task a brief moment to bind.
    tokio::time::sleep(Duration::from_millis(50)).await;

    let stream = tokio::net::TcpStream::connect(server_addr).await.unwrap();
    let (mut reader, mut writer) = stream.into_split();

    // Perform handshake as a remote peer.
    let nonce = "client-nonce".to_string();
    let node_id = "client-live-node".to_string();
    let auth_data = format!("{}{}", nonce, node_id);
    let auth_hmac = hmac_sign(&shared_secret, auth_data.as_bytes());
    let handshake = WireMessage {
        id: "hs-registry-live".to_string(),
        signature: None,
        seq_num: None,
        kind: WireMessageKind::Request(WireRequest::Handshake {
            node_id: node_id.clone(),
            node_name: "live-client".to_string(),
            protocol_version: PROTOCOL_VERSION,
            agents: vec![],
            nonce,
            auth_hmac,
            extensions: Some(vec!["msg_hmac".to_string()]),
        }),
    };
    write_framed_message(&mut writer, &handshake).await.unwrap();

    let ack = read_framed_message(&mut reader).await.unwrap();
    match ack.kind {
        WireMessageKind::Response(WireResponse::HandshakeAck { .. }) => {}
        other => panic!("Expected HandshakeAck, got {:?}", other),
    }

    // Peer should now be tracked as connected.
    tokio::time::sleep(Duration::from_millis(50)).await;
    let peer = reg
        .get_peer("client-live-node")
        .await
        .expect("peer should exist");
    assert_eq!(peer.state, PeerState::Connected);
    assert_eq!(peer.node_name, "live-client");

    // Disconnect client socket and verify registry state changes.
    drop(reader);
    drop(writer);
    tokio::time::sleep(Duration::from_millis(100)).await;
    let peer = reg
        .get_peer("client-live-node")
        .await
        .expect("peer should still exist");
    assert_eq!(peer.state, PeerState::Disconnected);

    server.abort();
}

// ═══════════════════════════════════════════════════════════════════════
// 7. Inbound AgentMessage Handling tests
// ═══════════════════════════════════════════════════════════════════════

use autonoetic_gateway::router::JsonRpcRouter;
use autonoetic_types::config::GatewayConfig;
use std::sync::Arc;
use tempfile::TempDir;

#[tokio::test]
async fn test_inbound_agent_message() {
    let reg = PeerRegistry::new();
    let shared_secret = "agent-msg-secret".to_string();

    // 1. Setup a temporary agent directory for the router
    let temp_dir = TempDir::new().unwrap();
    let agents_dir = temp_dir.path().join("agents");
    std::fs::create_dir_all(&agents_dir).unwrap();

    let target_agent_id = "test_target_agent";
    let agent_dir = agents_dir.join(target_agent_id);
    std::fs::create_dir_all(&agent_dir).unwrap();

    let manifest_yaml = format!(
        r#"
name: "{}"
description: "Integration test agent"
metadata:
  autonoetic:
    version: "1.0"
    agent:
      id: "{}"
      name: "target-agent"
      description: "mock agent"
    llm_config:
      provider: "mock"
      model: "mock"
    capabilities: []
"#,
        target_agent_id, target_agent_id
    );

    let skill_md = format!(
        "---\n{}\n---\n# Instructions\nYou are a mock agent.",
        manifest_yaml.trim()
    );
    std::fs::write(agent_dir.join("SKILL.md"), skill_md).unwrap();

    // 2. Configure Gateway and Router
    let config = GatewayConfig {
        port: 0,
        ofp_port: 0,
        agents_dir,
        ..Default::default()
    };
    let router = Arc::new(JsonRpcRouter::new(config));

    // 3. Start OFP server on a random port
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let server_addr = listener.local_addr().unwrap();
    drop(listener);

    let server_node_id = "server-node-id".to_string();
    let server = tokio::spawn(start_ofp_server(
        server_addr,
        server_node_id.clone(),
        "server-node-name".to_string(),
        shared_secret.clone(),
        reg.clone(),
        router,
    ));

    tokio::time::sleep(Duration::from_millis(50)).await;

    // 4. Connect as a client
    let stream = tokio::net::TcpStream::connect(server_addr).await.unwrap();
    let (mut reader, mut writer) = stream.into_split();

    // Handshake
    let client_node_id = "client-node-id".to_string();
    let nonce = "client-nonce".to_string();
    let auth_data = format!("{}{}", nonce, client_node_id);
    let auth_hmac = hmac_sign(&shared_secret, auth_data.as_bytes());

    let handshake = WireMessage {
        id: "hs-1".to_string(),
        signature: None,
        seq_num: None,
        kind: WireMessageKind::Request(WireRequest::Handshake {
            node_id: client_node_id.clone(),
            node_name: "client-node".to_string(),
            protocol_version: PROTOCOL_VERSION,
            agents: vec![RemoteAgentInfo {
                id: "remote_sender_agent".to_string(),
                name: "remote_sender_agent".to_string(),
                description: "Remote sender agent".to_string(),
                tags: vec!["test".to_string()],
                tools: vec![],
                state: "running".to_string(),
            }],
            nonce,
            auth_hmac,
            extensions: Some(vec!["msg_hmac".to_string()]),
        }),
    };
    write_framed_message(&mut writer, &handshake).await.unwrap();

    // Read HandshakeAck
    let ack = read_framed_message(&mut reader).await.unwrap();
    match ack.kind {
        WireMessageKind::Response(WireResponse::HandshakeAck { .. }) => {}
        other => panic!("Expected HandshakeAck, got {:?}", other),
    }

    // 5. Send an inbound AgentMessage request
    let source_agent_id = "remote_sender_agent";
    let agent_msg = WireMessage {
        id: "msg-1".to_string(),
        signature: None,
        seq_num: Some(1),
        kind: WireMessageKind::Request(WireRequest::AgentMessage {
            agent: target_agent_id.to_string(),
            message: "Hello from OFP client!".to_string(),
            sender: Some(source_agent_id.to_string()),
        }),
    };

    // Sign the message since msg_hmac is negotiated
    let mut signed_msg = agent_msg.clone();
    signed_msg.signature = Some(
        autonoetic_gateway::server::ofp::sign_wire_message(&shared_secret, &signed_msg).unwrap(),
    );

    write_framed_message(&mut writer, &signed_msg)
        .await
        .unwrap();

    // 6. Read AgentResponse
    let resp = read_framed_message(&mut reader).await.unwrap();

    // Verify signature of the response
    autonoetic_gateway::server::ofp::verify_wire_message(&shared_secret, &resp, 1).unwrap();

    match resp.kind {
        WireMessageKind::Response(WireResponse::Error { code, message }) => {
            assert_eq!(code, 500);
            assert!(
                message.contains("Unknown provider 'mock'"),
                "Expected unknown provider error, got: {}",
                message
            );
        }
        other => panic!("Expected Error response, got {:?}", other),
    }

    server.abort();
}

#[tokio::test]
async fn test_inbound_agent_message_rejects_unadvertised_sender() {
    let reg = PeerRegistry::new();
    let shared_secret = "agent-msg-secret".to_string();

    let temp_dir = TempDir::new().unwrap();
    let agents_dir = temp_dir.path().join("agents");
    std::fs::create_dir_all(&agents_dir).unwrap();

    let target_agent_id = "test_target_agent";
    let agent_dir = agents_dir.join(target_agent_id);
    std::fs::create_dir_all(&agent_dir).unwrap();
    let skill_md = format!(
        "---\n{}\n---\n# Instructions\nYou are a mock agent.",
        format!(
            r#"
name: "{}"
description: "Integration test agent"
metadata:
  autonoetic:
    version: "1.0"
    agent:
      id: "{}"
      name: "target-agent"
      description: "mock agent"
    llm_config:
      provider: "mock"
      model: "mock"
    capabilities: []
"#,
            target_agent_id, target_agent_id
        )
        .trim()
    );
    std::fs::write(agent_dir.join("SKILL.md"), skill_md).unwrap();

    let config = GatewayConfig {
        port: 0,
        ofp_port: 0,
        agents_dir,
        ..Default::default()
    };
    let router = Arc::new(JsonRpcRouter::new(config));

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let server_addr = listener.local_addr().unwrap();
    drop(listener);

    let server = tokio::spawn(start_ofp_server(
        server_addr,
        "server-node-id".to_string(),
        "server-node-name".to_string(),
        shared_secret.clone(),
        reg,
        router,
    ));

    tokio::time::sleep(Duration::from_millis(50)).await;

    let stream = tokio::net::TcpStream::connect(server_addr).await.unwrap();
    let (mut reader, mut writer) = stream.into_split();

    let client_node_id = "client-node-id".to_string();
    let nonce = "client-nonce".to_string();
    let auth_data = format!("{}{}", nonce, client_node_id);
    let auth_hmac = hmac_sign(&shared_secret, auth_data.as_bytes());
    let handshake = WireMessage {
        id: "hs-2".to_string(),
        signature: None,
        seq_num: None,
        kind: WireMessageKind::Request(WireRequest::Handshake {
            node_id: client_node_id,
            node_name: "client-node".to_string(),
            protocol_version: PROTOCOL_VERSION,
            agents: vec![],
            nonce,
            auth_hmac,
            extensions: Some(vec!["msg_hmac".to_string()]),
        }),
    };
    write_framed_message(&mut writer, &handshake).await.unwrap();
    let _ack = read_framed_message(&mut reader).await.unwrap();

    let mut agent_msg = WireMessage {
        id: "msg-2".to_string(),
        signature: None,
        seq_num: Some(1),
        kind: WireMessageKind::Request(WireRequest::AgentMessage {
            agent: target_agent_id.to_string(),
            message: "Hello from OFP client!".to_string(),
            sender: Some("spoofed_sender".to_string()),
        }),
    };
    agent_msg.signature = Some(
        autonoetic_gateway::server::ofp::sign_wire_message(&shared_secret, &agent_msg).unwrap(),
    );

    write_framed_message(&mut writer, &agent_msg).await.unwrap();
    let resp = read_framed_message(&mut reader).await.unwrap();
    autonoetic_gateway::server::ofp::verify_wire_message(&shared_secret, &resp, 1).unwrap();

    match resp.kind {
        WireMessageKind::Response(WireResponse::Error { code, message }) => {
            assert_eq!(code, 403);
            assert!(
                message.contains("not advertised"),
                "unexpected message: {}",
                message
            );
        }
        other => panic!("Expected Error response, got {:?}", other),
    }

    server.abort();
}
