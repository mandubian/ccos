//! Integration tests for loopback channels and memory execution loops.

use autonoetic_gateway::router::{JsonRpcRequest, JsonRpcResponse, JsonRpcRouter};
use autonoetic_gateway::server::jsonrpc::start_jsonrpc_server;
use autonoetic_types::config::GatewayConfig;
use tempfile::TempDir;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};

struct EnvGuard {
    key: &'static str,
    previous: Option<String>,
}

impl EnvGuard {
    fn set(key: &'static str, value: impl Into<String>) -> Self {
        let previous = std::env::var(key).ok();
        std::env::set_var(key, value.into());
        Self { key, previous }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        if let Some(previous) = self.previous.take() {
            std::env::set_var(self.key, previous);
        } else {
            std::env::remove_var(self.key);
        }
    }
}

async fn mock_openai_server() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();

    tokio::spawn(async move {
        loop {
            let accept_res = listener.accept().await;
            if accept_res.is_err() {
                continue;
            }
            let (mut stream, _) = accept_res.unwrap();
            let mut read_buf = Vec::new();
            let mut buf = [0u8; 1024];

            loop {
                let n = stream.read(&mut buf).await.unwrap_or(0);
                if n == 0 {
                    break;
                }
                read_buf.extend_from_slice(&buf[..n]);
                if read_buf.windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }

            let headers_str = String::from_utf8_lossy(&read_buf);
            let mut content_length = 0;
            for line in headers_str.lines() {
                if line.to_lowercase().starts_with("content-length:") {
                    content_length = line[15..].trim().parse().unwrap_or(0);
                }
            }

            let header_len = headers_str.find("\r\n\r\n").unwrap() + 4;
            let mut body_bytes = read_buf[header_len..].to_vec();

            while body_bytes.len() < content_length {
                let n = stream.read(&mut buf).await.unwrap_or(0);
                if n == 0 {
                    break;
                }
                body_bytes.extend_from_slice(&buf[..n]);
            }

            let body_str = String::from_utf8_lossy(&body_bytes);

            if body_str.contains("delay message") {
                tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;
            }

            let response_json = if body_str.contains("store this data")
                && !body_str.contains("\"role\":\"tool\"")
            {
                serde_json::json!({
                    "id": "chatcmpl-1",
                    "object": "chat.completion",
                    "created": 1,
                    "model": "gpt-4o",
                    "choices": [{
                        "index": 0,
                        "message": { "role": "assistant", "tool_calls": [{ "id": "call_1", "type": "function", "function": { "name": "memory.write", "arguments": "{\"path\":\"secret.txt\",\"content\":\"secret_value_123\"}" } }] },
                        "finish_reason": "tool_calls"
                    }],
                    "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}
                })
            } else if body_str.contains("store this data") && body_str.contains("\"role\":\"tool\"")
            {
                serde_json::json!({
                    "id": "chatcmpl-2",
                    "object": "chat.completion",
                    "created": 2,
                    "model": "gpt-4o",
                    "choices": [{
                        "index": 0,
                        "message": { "role": "assistant", "content": "I stored it" },
                        "finish_reason": "stop"
                    }],
                    "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}
                })
            } else if body_str.contains("what is the data")
                && !body_str.contains("\"role\":\"tool\"")
            {
                serde_json::json!({
                    "id": "chatcmpl-3",
                    "object": "chat.completion",
                    "created": 3,
                    "model": "gpt-4o",
                    "choices": [{
                        "index": 0,
                        "message": { "role": "assistant", "tool_calls": [{ "id": "call_2", "type": "function", "function": { "name": "memory.read", "arguments": "{\"path\":\"secret.txt\"}" } }] },
                        "finish_reason": "tool_calls"
                    }],
                    "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}
                })
            } else if body_str.contains("what is the data")
                && body_str.contains("\"role\":\"tool\"")
            {
                serde_json::json!({
                    "id": "chatcmpl-4",
                    "object": "chat.completion",
                    "created": 4,
                    "model": "gpt-4o",
                    "choices": [{
                        "index": 0,
                        "message": { "role": "assistant", "content": "The data is secret_value_123" },
                        "finish_reason": "stop"
                    }],
                    "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}
                })
            } else if body_str.contains("read missing") && !body_str.contains("\"role\":\"tool\"") {
                serde_json::json!({
                    "id": "chatcmpl-5",
                    "object": "chat.completion",
                    "created": 5,
                    "model": "gpt-4o",
                    "choices": [{
                        "index": 0,
                        "message": { "role": "assistant", "tool_calls": [{ "id": "call_3", "type": "function", "function": { "name": "memory.read", "arguments": "{\"path\":\"missing.txt\"}" } }] },
                        "finish_reason": "tool_calls"
                    }],
                    "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}
                })
            } else if body_str.contains("read missing") && body_str.contains("\"role\":\"tool\"") {
                serde_json::json!({
                    "id": "chatcmpl-6",
                    "object": "chat.completion",
                    "created": 6,
                    "model": "gpt-4o",
                    "choices": [{
                        "index": 0,
                        "message": { "role": "assistant", "content": "File is missing" },
                        "finish_reason": "stop"
                    }],
                    "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}
                })
            } else if body_str.contains("read default") && !body_str.contains("\"role\":\"tool\"") {
                serde_json::json!({
                    "id": "chatcmpl-7",
                    "object": "chat.completion",
                    "created": 7,
                    "model": "gpt-4o",
                    "choices": [{
                        "index": 0,
                        "message": { "role": "assistant", "tool_calls": [{ "id": "call_4", "type": "function", "function": { "name": "memory.read", "arguments": "{\"path\":\"missing-default.txt\",\"default_value\":\"my_default_fallback\"}" } }] },
                        "finish_reason": "tool_calls"
                    }],
                    "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}
                })
            } else if body_str.contains("read default") && body_str.contains("\"role\":\"tool\"") {
                serde_json::json!({
                    "id": "chatcmpl-8",
                    "object": "chat.completion",
                    "created": 8,
                    "model": "gpt-4o",
                    "choices": [{
                        "index": 0,
                        "message": { "role": "assistant", "content": "Fallback is my_default_fallback" },
                        "finish_reason": "stop"
                    }],
                    "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}
                })
            } else if body_str.contains("read missing") && body_str.contains("\"role\":\"tool\"") {
                serde_json::json!({
                    "id": "chatcmpl-6",
                    "object": "chat.completion",
                    "created": 6,
                    "model": "gpt-4o",
                    "choices": [{
                        "index": 0,
                        "message": { "role": "assistant", "content": "File is missing" },
                        "finish_reason": "stop"
                    }],
                    "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}
                })
            } else {
                serde_json::json!({
                    "id": "chatcmpl-default",
                    "object": "chat.completion",
                    "created": 7,
                    "model": "gpt-4o",
                    "choices": [{
                        "index": 0,
                        "message": { "role": "assistant", "content": "Unknown flow" },
                        "finish_reason": "stop"
                    }],
                    "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}
                })
            };

            let body_bytes = serde_json::to_string(&response_json).unwrap();
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body_bytes.len(),
                body_bytes
            );
            let _ = stream.write_all(resp.as_bytes()).await;
        }
    });

    format!("http://127.0.0.1:{}", port)
}

async fn send_jsonrpc(write_half: &mut tokio::net::tcp::OwnedWriteHalf, req: JsonRpcRequest) {
    let msg = serde_json::to_string(&req).unwrap();
    write_half.write_all(msg.as_bytes()).await.unwrap();
    write_half.write_all(b"\n").await.unwrap();
    write_half.flush().await.unwrap();
}

async fn recv_jsonrpc(
    lines: &mut tokio::io::Lines<BufReader<tokio::net::tcp::OwnedReadHalf>>,
) -> JsonRpcResponse {
    let line = lines
        .next_line()
        .await
        .unwrap()
        .expect("End of stream before response");
    serde_json::from_str(&line).unwrap()
}

#[tokio::test]
async fn test_loopback_memory_audit_and_negatives() {
    let mock_url = mock_openai_server().await;
    let _guard1 = EnvGuard::set("AUTONOETIC_LLM_BASE_URL", mock_url);
    let _guard2 = EnvGuard::set("OPENAI_API_KEY", "test-key");

    let temp_dir = TempDir::new().unwrap();
    let agents_dir = temp_dir.path().join("agents");
    std::fs::create_dir_all(&agents_dir).unwrap();

    let agent_id = "memory_agent";
    let agent_dir = agents_dir.join(agent_id);
    std::fs::create_dir_all(&agent_dir).unwrap();

    let manifest_yaml = format!(
        r#"
name: "Memory Agent"
description: "Integration test memory agent"
metadata:
  autonoetic:
    version: "1.0"
    agent:
      id: "{agent_id}"
      name: "memory_agent"
      description: "mock agent"
    llm_config:
      provider: "openai"
      model: "gpt-4o"
    capabilities:
      - type: "MemoryWrite"
        scopes: ["*"]
      - type: "MemoryRead"
        scopes: ["*"]
"#
    );

    let skill_md = format!(
        "---\n{}\n---\n# Instructions\nYou are a memory agent.",
        manifest_yaml.trim()
    );
    std::fs::write(agent_dir.join("SKILL.md"), skill_md).unwrap();

    // Use small limits to easily trigger backpressure for negative tests
    let config = GatewayConfig {
        port: 0,
        ofp_port: 0,
        agents_dir: agents_dir.clone(),
        max_pending_spawns_per_agent: 1, // Only 1 concurrent execution per agent
        max_concurrent_spawns: 5,
        ..Default::default()
    };

    let router = JsonRpcRouter::new(config);

    // 1. Start REAL JSON-RPC server loopback transport on ephemeral port
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let listen_addr = listener.local_addr().unwrap();
    drop(listener); // Free the port for the JSON-RPC server

    let server_task = tokio::spawn(async move {
        start_jsonrpc_server(listen_addr, router).await.unwrap();
    });

    // Connect client
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
    let stream = TcpStream::connect(listen_addr).await.unwrap();
    let (read_half, mut write_half) = stream.into_split();
    let mut lines = BufReader::new(read_half).lines();

    let session_id = "test-session-123";

    // --- Turn 1: Write to memory (Happy path) ---
    send_jsonrpc(
        &mut write_half,
        JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: "1".to_string(),
            method: "event.ingest".to_string(),
            params: serde_json::json!({
                "target_agent_id": agent_id,
                "session_id": session_id,
                "event_type": "chat",
                "message": "please store this data",
            }),
        },
    )
    .await;
    let resp1 = recv_jsonrpc(&mut lines).await;
    assert!(resp1.error.is_none(), "Request 1 failed: {:?}", resp1.error);

    // Check Tier1 memory substrate
    let state_file = agent_dir.join("state").join("secret.txt");
    assert!(state_file.exists(), "State file was not written!");
    assert_eq!(
        std::fs::read_to_string(&state_file).unwrap(),
        "secret_value_123"
    );

    // --- Turn 2: Read from memory (Happy path) ---
    send_jsonrpc(
        &mut write_half,
        JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: "2".to_string(),
            method: "event.ingest".to_string(),
            params: serde_json::json!({
                "target_agent_id": agent_id,
                "session_id": session_id,
                "event_type": "chat",
                "message": "what is the data?",
            }),
        },
    )
    .await;
    let resp2 = recv_jsonrpc(&mut lines).await;
    assert!(resp2.error.is_none(), "Request 2 failed: {:?}", resp2.error);
    assert!(resp2
        .result
        .unwrap()
        .to_string()
        .contains("The data is secret_value_123"));

    // --- Turn 3: Negative Path - Missing memory key ---
    send_jsonrpc(
        &mut write_half,
        JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: "3".to_string(),
            method: "event.ingest".to_string(),
            params: serde_json::json!({
                "target_agent_id": agent_id,
                "session_id": session_id,
                "event_type": "chat",
                "message": "read missing",
            }),
        },
    )
    .await;
    let resp3 = recv_jsonrpc(&mut lines).await;
    assert!(
        resp3.error.is_some(),
        "Expected Request 3 to fail due to missing memory key"
    );
    let err3 = resp3.error.unwrap();
    assert_eq!(err3.code, -32000, "Expected internal error code");
    assert!(
        err3.message.contains("File not found in Tier 1 memory"),
        "Unexpected error message: {}",
        err3.message
    );

    // --- Turn 3.5: Negative Path - Missing memory key with default_value ---
    send_jsonrpc(
        &mut write_half,
        JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: "3.5".to_string(),
            method: "event.ingest".to_string(),
            params: serde_json::json!({
                "target_agent_id": agent_id,
                "session_id": session_id,
                "event_type": "chat",
                "message": "read default",
            }),
        },
    )
    .await;
    let resp3_5 = recv_jsonrpc(&mut lines).await;
    assert!(
        resp3_5.error.is_none(),
        "Request 3.5 failed: {:?}",
        resp3_5.error
    );
    assert!(resp3_5
        .result
        .unwrap()
        .to_string()
        .contains("Fallback is my_default_fallback"));

    // --- Turn 4: Negative Path - Outbound backpressure / rejection ---
    // First, spawn a background client connecting to send a long-running request.
    let stream_b = TcpStream::connect(listen_addr).await.unwrap();
    let (read_half_b, mut write_half_b) = stream_b.into_split();
    let mut lines_b = BufReader::new(read_half_b).lines();

    // Fire off the delaying request. It acquires the single semaphore capacity.
    send_jsonrpc(
        &mut write_half_b,
        JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: "4".to_string(),
            method: "event.ingest".to_string(),
            params: serde_json::json!({
                "target_agent_id": agent_id,
                "session_id": "session-slow",
                "event_type": "chat",
                "message": "delay message",
            }),
        },
    )
    .await;

    // Wait a tiny bit to ensure the first request locked the semaphore
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    // While the first request is holding the semaphore, fire the second request using the main client.
    // It should hit the reliability control backpressure immediately.
    send_jsonrpc(
        &mut write_half,
        JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: "5".to_string(),
            method: "event.ingest".to_string(),
            params: serde_json::json!({
                "target_agent_id": agent_id,
                "session_id": "session-fast",
                "event_type": "chat",
                "message": "please store this data",
            }),
        },
    )
    .await;

    let resp_rejected = recv_jsonrpc(&mut lines).await;
    assert!(
        resp_rejected.error.is_some(),
        "Expected second request to be rejected due to backpressure"
    );
    let err = resp_rejected.error.unwrap();
    assert_eq!(err.code, -32000, "Expected backpressure error code -32000");
    assert!(
        err.message.contains("pending execution queue is full"),
        "Expected backpressure message, got: {}",
        err.message
    );

    // Read the delayed success result to cleanly finish
    let resp4 = recv_jsonrpc(&mut lines_b).await;
    assert!(resp4.error.is_none());

    // --- Causal Lineage Auditing ---
    // Agent-local log
    let agent_history_file = agent_dir.join("history").join("causal_chain.jsonl");
    let agent_history = std::fs::read_to_string(&agent_history_file).unwrap();

    let mut agent_mem_writes = 0;
    let mut agent_mem_reads = 0;
    let mut agent_session_ends = 0;

    for line in agent_history.lines() {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(line) {
            if value["session_id"].as_str() == Some(session_id) {
                if value["action"].as_str() == Some("requested") {
                    if value["payload"]["tool_name"].as_str() == Some("memory.write") {
                        agent_mem_writes += 1;
                    } else if value["payload"]["tool_name"].as_str() == Some("memory.read") {
                        agent_mem_reads += 1;
                    }
                } else if value["category"].as_str() == Some("session")
                    && value["action"].as_str() == Some("end")
                {
                    agent_session_ends += 1;
                }
            }
        }
    }

    assert_eq!(
        agent_mem_writes, 1,
        "Expected exactly 1 memory.write in agent history for session"
    );
    assert_eq!(
        agent_mem_reads, 3,
        "Expected exactly 3 memory.read in agent history for session"
    );
    assert_eq!(
        agent_session_ends, 3,
        "Expected exactly 3 session ends in agent history for session"
    );

    // Gateway-owned log
    let gateway_history_file = agents_dir
        .join(".gateway")
        .join("history")
        .join("causal_chain.jsonl");
    let gateway_history = std::fs::read_to_string(&gateway_history_file).unwrap_or_default();

    let mut gateway_requests = 0;
    let mut gateway_completions = 0;
    let mut gateway_failures = 0;

    for line in gateway_history.lines() {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(line) {
            if value["session_id"].as_str() == Some(session_id) {
                let action = value["action"].as_str().unwrap_or("");
                if action == "event.ingest.requested" {
                    gateway_requests += 1;
                } else if action == "event.ingest.completed" {
                    gateway_completions += 1;
                } else if action == "event.ingest.failed" {
                    gateway_failures += 1;
                }
            }
        }
    }

    assert_eq!(
        gateway_requests, 4,
        "Expected exactly 4 gateway ingest requests for session"
    );
    assert_eq!(
        gateway_completions, 3,
        "Expected exactly 3 gateway ingest completions for session"
    );
    assert_eq!(
        gateway_failures, 1,
        "Expected exactly 1 gateway ingest failure for session"
    );

    server_task.abort();
}
