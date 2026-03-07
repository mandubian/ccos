//! End-to-end integration test for live JSON-RPC ingress.

use std::path::Path;

mod support;

use support::{
    read_causal_entries, spawn_gateway_server, EnvGuard, JsonRpcClient, OpenAiStub, TestWorkspace,
};

const LLM_BASE_URL_OVERRIDE_ENV: &str = "AUTONOETIC_LLM_BASE_URL";
const LLM_API_KEY_OVERRIDE_ENV: &str = "AUTONOETIC_LLM_API_KEY";

fn write_test_agent(agent_dir: &Path, agent_id: &str) -> anyhow::Result<()> {
    std::fs::create_dir_all(agent_dir)?;
    let skill = format!(
        "---\nversion: \"1.0\"\nruntime:\n  engine: \"autonoetic\"\n  gateway_version: \"0.1.0\"\n  sdk_version: \"0.1.0\"\n  type: \"stateful\"\n  sandbox: \"bubblewrap\"\n  runtime_lock: \"runtime.lock\"\nagent:\n  id: \"{agent_id}\"\n  name: \"{agent_id}\"\n  description: \"Ingress test agent\"\nllm_config:\n  provider: \"openai\"\n  model: \"test-model\"\n  temperature: 0.0\n---\n# Instructions\nReply with the model output.\n",
    );
    std::fs::write(agent_dir.join("SKILL.md"), skill)?;
    Ok(())
}

#[tokio::test]
async fn test_event_ingest_live_jsonrpc_ingress_writes_gateway_and_agent_traces(
) -> anyhow::Result<()> {
    let workspace = TestWorkspace::new()?;
    let agents_dir = workspace.agents_dir.clone();
    let target_agent_id = "agent_ingress_test";
    write_test_agent(&agents_dir.join(target_agent_id), target_agent_id)?;

    let stub = OpenAiStub::spawn(|_, _| async move {
        serde_json::json!({
            "choices": [{
                "message": { "content": "stub assistant reply" },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 12,
                "completion_tokens": 3
            }
        })
    })
    .await?;
    let _base_url = EnvGuard::set(LLM_BASE_URL_OVERRIDE_ENV, stub.completion_url());
    let _api_key = EnvGuard::set(LLM_API_KEY_OVERRIDE_ENV, "test-key");

    let (jsonrpc_addr, server) = spawn_gateway_server(autonoetic_types::config::GatewayConfig {
        agents_dir: agents_dir.clone(),
        ..workspace.gateway_config()
    })
    .await?;
    let mut client = JsonRpcClient::connect(jsonrpc_addr).await?;

    let session_id = "session-e2e-ingress";
    let response = client
        .event_ingest(
            "ingress-1",
            target_agent_id,
            session_id,
            "webhook",
            "Incoming deployment event",
            Some(serde_json::json!({"source": "integration-test"})),
        )
        .await?;

    assert!(
        response.error.is_none(),
        "unexpected error: {:?}",
        response.error
    );
    let result = response.result.expect("result should exist");
    assert_eq!(result["assistant_reply"], "stub assistant reply");
    assert_eq!(result["session_id"], session_id);

    let request_bodies = stub.captured_bodies();
    assert_eq!(request_bodies.len(), 1);
    let body = &request_bodies[0];
    assert_eq!(body["model"], "test-model");
    let joined_messages = body["messages"]
        .as_array()
        .expect("messages should be an array")
        .iter()
        .filter_map(|msg| msg["content"].as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(joined_messages.contains("Gateway event type: webhook"));
    assert!(joined_messages.contains("Incoming deployment event"));

    let gateway_entries = read_causal_entries(
        &agents_dir
            .join(".gateway")
            .join("history")
            .join("causal_chain.jsonl"),
    )?;
    assert!(gateway_entries.iter().any(|entry| {
        entry.session_id == session_id && entry.action == "event.ingest.requested"
    }));
    assert!(gateway_entries.iter().any(|entry| {
        entry.session_id == session_id && entry.action == "event.ingest.completed"
    }));

    let agent_entries = read_causal_entries(
        &agents_dir
            .join(target_agent_id)
            .join("history")
            .join("causal_chain.jsonl"),
    )?;
    assert!(agent_entries.iter().any(|entry| {
        entry.session_id == session_id && entry.category == "session" && entry.action == "start"
    }));
    assert!(agent_entries.iter().any(|entry| {
        entry.session_id == session_id && entry.category == "session" && entry.action == "end"
    }));

    server.abort();
    Ok(())
}
