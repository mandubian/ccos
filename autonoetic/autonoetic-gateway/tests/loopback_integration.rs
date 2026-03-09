//! Integration tests for loopback channels and memory execution loops.

mod support;

use support::agents::install_memory_recall_agent;
use support::{spawn_gateway_server, EnvGuard, JsonRpcClient, OpenAiStub, TestWorkspace};

#[tokio::test]
async fn test_loopback_memory_audit_and_negatives() {
    let stub = OpenAiStub::spawn(|_, body_json| async move {
        let messages = body_json["messages"].as_array().cloned().unwrap_or_default();
        let latest_user_message = messages
            .iter()
            .rev()
            .find_map(|message| {
                if message["role"].as_str() == Some("user") {
                    message["content"].as_str()
                } else {
                    None
                }
            })
            .unwrap_or("");
        let has_tool_result_turn = messages
            .iter()
            .any(|message| message["role"].as_str() == Some("tool"));

        if latest_user_message.contains("delay message") {
            tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;
        }

        if latest_user_message.contains("store this data") && !has_tool_result_turn {
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
        } else if latest_user_message.contains("store this data") && has_tool_result_turn {
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
        } else if latest_user_message.contains("what is the data") && !has_tool_result_turn {
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
        } else if latest_user_message.contains("what is the data") && has_tool_result_turn {
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
        } else if latest_user_message.contains("read missing") && !has_tool_result_turn {
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
        } else if latest_user_message.contains("read missing") && has_tool_result_turn {
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
        } else if latest_user_message.contains("read default") && !has_tool_result_turn {
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
        } else if latest_user_message.contains("read default") && has_tool_result_turn {
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
        }
    })
    .await
    .unwrap();
    let _guard1 = EnvGuard::set("AUTONOETIC_LLM_BASE_URL", stub.completion_url());
    let _guard2 = EnvGuard::set("OPENAI_API_KEY", "test-key");

    let workspace = TestWorkspace::new().unwrap();
    let agents_dir = workspace.agents_dir.clone();

    let agent_id = "memory_agent";
    let agent_dir = agents_dir.join(agent_id);
    std::fs::create_dir_all(&agent_dir).unwrap();

    install_memory_recall_agent(&agent_dir, agent_id).unwrap();

    // Use small limits to easily trigger backpressure for negative tests
    let config = autonoetic_types::config::GatewayConfig {
        port: 0,
        ofp_port: 0,
        agents_dir: agents_dir.clone(),
        max_pending_spawns_per_agent: 1, // Only 1 concurrent execution per agent
        max_concurrent_spawns: 5,
        ..Default::default()
    };

    let (listen_addr, server_task) = spawn_gateway_server(config).await.unwrap();
    let mut client = JsonRpcClient::connect(listen_addr).await.unwrap();

    let session_id = "test-session-123";

    // --- Turn 1: Write to memory (Happy path) ---
    let resp1 = client
        .event_ingest(
            "1",
            agent_id,
            session_id,
            "chat",
            "please store this data",
            None,
        )
        .await
        .unwrap();
    assert!(resp1.error.is_none(), "Request 1 failed: {:?}", resp1.error);

    // Check Tier1 memory substrate
    let state_file = agent_dir.join("state").join("secret.txt");
    assert!(state_file.exists(), "State file was not written!");
    assert_eq!(
        std::fs::read_to_string(&state_file).unwrap(),
        "secret_value_123"
    );

    // --- Turn 2: Read from memory (Happy path) ---
    let resp2 = client
        .event_ingest("2", agent_id, session_id, "chat", "what is the data?", None)
        .await
        .unwrap();
    assert!(resp2.error.is_none(), "Request 2 failed: {:?}", resp2.error);
    assert!(resp2
        .result
        .unwrap()
        .to_string()
        .contains("The data is secret_value_123"));

    // --- Turn 3: Negative Path - Missing memory key ---
    // The tool returns a structured error (not a gateway failure), and the agent continues.
    // This is the desired iterative repair behavior: tool errors are visible to the agent.
    let resp3 = client
        .event_ingest("3", agent_id, session_id, "chat", "read missing", None)
        .await
        .unwrap();
    // Gateway succeeds - the tool error is returned as a tool_result to the agent, not as a JSON-RPC error
    assert!(
        resp3.error.is_none(),
        "Gateway should succeed; tool error should be returned to agent for repair"
    );
    // The agent should report the file is missing based on the tool's structured error response
    let result3 = resp3.result.unwrap();
    let result_str = result3.to_string();
    assert!(
        result_str.contains("missing") || result_str.contains("not found") || result_str.contains("File"),
        "Expected agent to report missing file, got: {}",
        result_str
    );

    // --- Turn 3.5: Negative Path - Missing memory key with default_value ---
    let resp3_5 = client
        .event_ingest("3.5", agent_id, session_id, "chat", "read default", None)
        .await
        .unwrap();
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
    let mut slow_client = JsonRpcClient::connect(listen_addr).await.unwrap();

    // Fire off the delaying request. It acquires the single semaphore capacity.
    let prior_stub_requests = stub.captured_bodies().len();
    slow_client
        .send(autonoetic_gateway::router::JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: "4".to_string(),
            method: "event.ingest".to_string(),
            params: serde_json::json!({
                "target_agent_id": agent_id,
                "session_id": "session-slow",
                "event_type": "chat",
                "message": "delay message",
            }),
        })
        .await
        .unwrap();

    // Wait until the slow request has actually reached the LLM stub.
    for _ in 0..20 {
        if stub.captured_bodies().len() > prior_stub_requests {
            break;
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(20)).await;
    }

    // While the first request is holding the semaphore, fire the second request using the main client.
    // It should hit the reliability control backpressure immediately.
    let resp_rejected = client
        .event_ingest(
            "5",
            agent_id,
            "session-fast",
            "chat",
            "please store this data",
            None,
        )
        .await
        .unwrap();
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
    let resp4 = slow_client.recv().await.unwrap();
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
        agent_session_ends, 4,
        "Expected exactly 4 session ends in agent history for session (write, read data, read missing, read default)"
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
        gateway_completions, 4,
        "Expected exactly 4 gateway ingest completions for session (all succeed; tool errors returned to agent)"
    );
    assert_eq!(
        gateway_failures, 0,
        "Expected 0 gateway failures (tool errors are returned to agent, not as gateway failures)"
    );

    server_task.abort();
}
