//! End-to-end integration test for coder content generation workflow.
//!
//! Tests that a coder agent can generate files via content.write.
//! Follows the exact pattern of the working loopback_integration test.

mod support;

use support::agents::install_content_agent;
use support::{spawn_gateway_server, EnvGuard, JsonRpcClient, OpenAiStub, TestWorkspace};

/// Test: Coder generates files via content.write tool calls.
/// This verifies the tool execution is properly integrated into the agent lifecycle.
#[tokio::test]
#[serial_test::serial]
async fn test_coder_content_write_via_tool_calls() {
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

        if latest_user_message.contains("store this data") && !has_tool_result_turn {
            serde_json::json!({
                "id": "chatcmpl-1",
                "object": "chat.completion",
                "created": 1,
                "model": "gpt-4o",
                "choices": [{
                    "index": 0,
                    "message": { "role": "assistant", "tool_calls": [{ "id": "call_1", "type": "function", "function": { "name": "content.write", "arguments": "{\"name\":\"secret.txt\",\"content\":\"secret_value_123\"}" } }] },
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

    let agent_id = "content_agent";
    let agent_dir = agents_dir.join(agent_id);
    std::fs::create_dir_all(&agent_dir).unwrap();
    install_content_agent(&agent_dir, agent_id).unwrap();

    let config = autonoetic_types::config::GatewayConfig {
        port: 0,
        ofp_port: 0,
        agents_dir: agents_dir.clone(),
        max_pending_spawns_per_agent: 1,
        max_concurrent_spawns: 5,
        ..Default::default()
    };

    let (listen_addr, server_task) = spawn_gateway_server(config).await.unwrap();
    let mut client = JsonRpcClient::connect(listen_addr).await.unwrap();

    let session_id = "session-coder-content-test";

    // Send request to trigger coder
    let resp = client
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

    assert!(resp.error.is_none(), "Request failed: {:?}", resp.error);

    // Verify LLM was called and received our prompt
    let request_bodies = stub.captured_bodies();
    assert!(request_bodies.len() >= 1, "Should have made at least 1 LLM call");

    // Verify causal chain has content.write entry
    let agent_history_file = agents_dir.join(agent_id).join("history").join("causal_chain.jsonl");
    let agent_history = std::fs::read_to_string(&agent_history_file).unwrap();
    
    let mut content_write_count = 0;
    let mut session_start_count = 0;
    let mut session_end_count = 0;
    
    for line in agent_history.lines() {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(line) {
            if value["session_id"].as_str() == Some(session_id) {
                if value["action"].as_str() == Some("requested")
                    && value["payload"]["tool_name"].as_str() == Some("content.write")
                {
                    content_write_count += 1;
                }
                if value["category"].as_str() == Some("session") && value["action"].as_str() == Some("start") {
                    session_start_count += 1;
                }
                if value["category"].as_str() == Some("session") && value["action"].as_str() == Some("end") {
                    session_end_count += 1;
                }
            }
        }
    }
    
    // Verify tool was executed
    assert_eq!(content_write_count, 1, "Expected exactly 1 content.write in agent history for session {}", session_id);
    
    // Verify session completed successfully
    assert_eq!(session_start_count, 1, "Expected 1 session start");
    assert_eq!(session_end_count, 1, "Expected 1 session end");

    // Verify the response indicates the tool was used
    let result = resp.result.unwrap();
    let reply = result["assistant_reply"].as_str().unwrap_or("");
    assert!(reply.contains("stored"), "Response should indicate content was stored, got: {}", reply);

    server_task.abort();
}

/// Test: Multiple tool calls in a single turn (write script + SKILL.md).
#[tokio::test]
#[serial_test::serial]
async fn test_coder_multiple_tool_calls_single_turn() {
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

        if latest_user_message.contains("create a weather") && !has_tool_result_turn {
            // Return TWO tool calls in a single response
            serde_json::json!({
                "id": "chatcmpl-1",
                "object": "chat.completion",
                "created": 1,
                "model": "gpt-4o",
                "choices": [{
                    "index": 0,
                    "message": {
                        "role": "assistant",
                        "content": "I'll create the weather script and SKILL.md.",
                        "tool_calls": [
                            { "id": "call_1", "type": "function", "function": { "name": "content.write", "arguments": "{\"name\":\"weather.py\",\"content\":\"import json\\nprint(json.dumps({\\\"temp\\\": 22}))\\n\"}" } },
                            { "id": "call_2", "type": "function", "function": { "name": "content.write", "arguments": "{\"name\":\"SKILL.md\",\"content\":\"---\\nname: weather\\ndescription: Weather script\\nscript_entry: weather.py\\n---\\n# Weather Script\\n\"}" } }
                        ]
                    },
                    "finish_reason": "tool_calls"
                }],
                "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}
            })
        } else if latest_user_message.contains("create a weather") && has_tool_result_turn {
            serde_json::json!({
                "id": "chatcmpl-2",
                "object": "chat.completion",
                "created": 2,
                "model": "gpt-4o",
                "choices": [{
                    "index": 0,
                    "message": { "role": "assistant", "content": "Created weather.py and SKILL.md." },
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
                    "message": { "role": "assistant", "content": "Unknown" },
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

    let agent_id = "content_agent";
    let agent_dir = agents_dir.join(agent_id);
    std::fs::create_dir_all(&agent_dir).unwrap();
    install_content_agent(&agent_dir, agent_id).unwrap();

    let config = autonoetic_types::config::GatewayConfig {
        port: 0,
        ofp_port: 0,
        agents_dir: agents_dir.clone(),
        max_pending_spawns_per_agent: 1,
        max_concurrent_spawns: 5,
        ..Default::default()
    };

    let (listen_addr, server_task) = spawn_gateway_server(config).await.unwrap();
    let mut client = JsonRpcClient::connect(listen_addr).await.unwrap();

    let session_id = "session-multi-tool-test";

    let resp = client
        .event_ingest(
            "1",
            agent_id,
            session_id,
            "chat",
            "create a weather script",
            None,
        )
        .await
        .unwrap();

    assert!(resp.error.is_none(), "Request failed: {:?}", resp.error);

    // Verify multiple content.write calls were made
    let agent_history_file = agents_dir.join(agent_id).join("history").join("causal_chain.jsonl");
    let agent_history = std::fs::read_to_string(&agent_history_file).unwrap();
    
    let mut content_write_count = 0;
    for line in agent_history.lines() {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(line) {
            if value["session_id"].as_str() == Some(session_id)
                && value["action"].as_str() == Some("requested")
                && value["payload"]["tool_name"].as_str() == Some("content.write")
            {
                content_write_count += 1;
            }
        }
    }
    
    assert_eq!(content_write_count, 2, "Expected 2 content.write calls (script + SKILL.md)");

    server_task.abort();
}
