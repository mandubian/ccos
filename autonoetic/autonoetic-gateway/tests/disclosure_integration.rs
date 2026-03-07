//! Integration tests for Disclosure Policy reply filtering.

mod support;

use support::{spawn_gateway_server, EnvGuard, JsonRpcClient, OpenAiStub, TestWorkspace};

#[tokio::test]
async fn test_disclosure_policy_integration() {
    let stub = OpenAiStub::spawn(|body_str, _| async move {
        if body_str.contains("read secret") && !body_str.contains("\"role\":\"tool\"") {
            serde_json::json!({
                "id": "chat-secret",
                "object": "chat.completion",
                "created": 1,
                "model": "gpt-4o",
                "choices": [{
                    "index": 0,
                    "message": { "role": "assistant", "tool_calls": [{ "id": "call_1", "type": "function", "function": { "name": "memory.read", "arguments": "{\"path\":\"secrets/test.txt\"}" } }] },
                    "finish_reason": "tool_calls"
                }],
                "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}
            })
        } else if body_str.contains("read secret") && body_str.contains("\"role\":\"tool\"") {
            serde_json::json!({
                "id": "chat-secret-reply",
                "object": "chat.completion",
                "created": 2,
                "model": "gpt-4o",
                "choices": [{
                    "index": 0,
                    "message": { "role": "assistant", "content": "The top secret password is super_secret_wahoo" },
                    "finish_reason": "stop"
                }],
                "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}
            })
        } else if body_str.contains("read confidential") && !body_str.contains("\"role\":\"tool\"")
        {
            serde_json::json!({
                "id": "chat-confidential",
                "object": "chat.completion",
                "created": 3,
                "model": "gpt-4o",
                "choices": [{
                    "index": 0,
                    "message": { "role": "assistant", "tool_calls": [{ "id": "call_2", "type": "function", "function": { "name": "memory.read", "arguments": "{\"path\":\"internal/docs.txt\"}" } }] },
                    "finish_reason": "tool_calls"
                }],
                "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}
            })
        } else if body_str.contains("read confidential") && body_str.contains("\"role\":\"tool\"")
        {
            serde_json::json!({
                "id": "chat-confidential-reply",
                "object": "chat.completion",
                "created": 4,
                "model": "gpt-4o",
                "choices": [{
                    "index": 0,
                    "message": { "role": "assistant", "content": "Internal docs say: confidential_business_plan_v2" },
                    "finish_reason": "stop"
                }],
                "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}
            })
        } else if body_str.contains("read public") && !body_str.contains("\"role\":\"tool\"") {
            serde_json::json!({
                "id": "chat-public",
                "object": "chat.completion",
                "created": 5,
                "model": "gpt-4o",
                "choices": [{
                    "index": 0,
                    "message": { "role": "assistant", "tool_calls": [{ "id": "call_3", "type": "function", "function": { "name": "memory.read", "arguments": "{\"path\":\"public/hello.txt\"}" } }] },
                    "finish_reason": "tool_calls"
                }],
                "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}
            })
        } else if body_str.contains("read public") && body_str.contains("\"role\":\"tool\"") {
            serde_json::json!({
                "id": "chat-public-reply",
                "object": "chat.completion",
                "created": 6,
                "model": "gpt-4o",
                "choices": [{
                    "index": 0,
                    "message": { "role": "assistant", "content": "The public data is safe_public_data." },
                    "finish_reason": "stop"
                }],
                "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}
            })
        } else {
            serde_json::json!({
                "id": "chat-default",
                "object": "chat.completion",
                "created": 7,
                "model": "gpt-4o",
                "choices": [{
                    "index": 0,
                    "message": { "role": "assistant", "content": "Unknown test flow" },
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

    let agent_id = "disclosure_agent";
    let agent_dir = agents_dir.join(agent_id);
    std::fs::create_dir_all(&agent_dir).unwrap();

    let manifest_yaml = format!(
        r#"
version: "1.0"
runtime:
  engine: "autonoetic"
  gateway_version: "0.1.0"
  sdk_version: "0.1.0"
  type: "stateful"
  sandbox: "bubblewrap"
  runtime_lock: "uv.lock"
agent:
  id: "{agent_id}"
  name: "disclosure_agent"
  description: "testing"
llm_config:
  provider: "openai"
  model: "gpt-4o"
capabilities:
  - type: "MemoryRead"
    scopes: ["*"]
disclosure:
  default_class: "public"
  rules:
    - source: "memory.read"
      path_pattern: "secrets/*"
      class: "secret"
    - source: "memory.read"
      path_pattern: "internal/*"
      class: "confidential"
"#
    );

    let skill_md = format!(
        "---\n{}\n---\n# Instructions\nYou are an agent that respects disclosure rules.",
        manifest_yaml.trim()
    );
    std::fs::write(agent_dir.join("SKILL.md"), skill_md).unwrap();

    let state_dir = agent_dir.join("state");
    std::fs::create_dir_all(state_dir.join("secrets")).unwrap();
    std::fs::create_dir_all(state_dir.join("internal")).unwrap();
    std::fs::create_dir_all(state_dir.join("public")).unwrap();
    std::fs::write(state_dir.join("secrets/test.txt"), "super_secret_wahoo").unwrap();
    std::fs::write(
        state_dir.join("internal/docs.txt"),
        "confidential_business_plan_v2",
    )
    .unwrap();
    std::fs::write(state_dir.join("public/hello.txt"), "safe_public_data").unwrap();

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

    let resp1 = client
        .event_ingest(
            "1",
            agent_id,
            "test-session-secret",
            "chat",
            "read secret",
            None,
        )
        .await
        .unwrap();
    assert!(resp1.error.is_none(), "Secret flow fail: {:?}", resp1.error);
    let text1 = resp1.result.unwrap().to_string();
    assert!(text1.contains("[REDACTED: Secret content]"));
    assert!(!text1.contains("super_secret_wahoo"));

    let resp2 = client
        .event_ingest(
            "2",
            agent_id,
            "test-session-confidential",
            "chat",
            "read confidential",
            None,
        )
        .await
        .unwrap();
    assert!(
        resp2.error.is_none(),
        "Confidential flow fail: {:?}",
        resp2.error
    );
    let text2 = resp2.result.unwrap().to_string();
    assert!(text2.contains("[REDACTED: Confidential content]"));
    assert!(!text2.contains("confidential_business_plan_v2"));

    let resp3 = client
        .event_ingest(
            "3",
            agent_id,
            "test-session-public",
            "chat",
            "read public",
            None,
        )
        .await
        .unwrap();
    assert!(resp3.error.is_none(), "Public flow fail: {:?}", resp3.error);
    let text3 = resp3.result.unwrap().to_string();
    assert!(!text3.contains("[REDACTED"));
    assert!(text3.contains("safe_public_data"));

    server_task.abort();
}
