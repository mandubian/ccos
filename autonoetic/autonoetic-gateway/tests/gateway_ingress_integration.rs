//! End-to-end integration test for live JSON-RPC ingress.

mod support;

use support::agents::install_outbound_reply_agent;
use support::{
    read_causal_entries, spawn_gateway_server, EnvGuard, JsonRpcClient, OpenAiStub, TestWorkspace,
};

const LLM_BASE_URL_OVERRIDE_ENV: &str = "AUTONOETIC_LLM_BASE_URL";
const LLM_API_KEY_OVERRIDE_ENV: &str = "AUTONOETIC_LLM_API_KEY";

#[tokio::test]
async fn test_event_ingest_live_jsonrpc_ingress_writes_gateway_and_agent_traces(
) -> anyhow::Result<()> {
    let workspace = TestWorkspace::new()?;
    let agents_dir = workspace.agents_dir.clone();
    let target_agent_id = "agent_ingress_test";
    install_outbound_reply_agent(&agents_dir.join(target_agent_id), target_agent_id)?;

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
    let llm_usage = result["llm_usage"].as_array().expect("llm_usage should be an array");
    assert_eq!(llm_usage.len(), 1);
    assert_eq!(llm_usage[0]["input_tokens"], 12);
    assert_eq!(llm_usage[0]["output_tokens"], 3);

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
    
    // Per-turn correlation: gateway entries should reference the session
    let gateway_session_entries: Vec<_> = gateway_entries
        .iter()
        .filter(|e| e.session_id == session_id)
        .collect();
    
    assert!(!gateway_session_entries.is_empty(), 
        "Gateway should have entries for session {}", session_id);
    assert!(gateway_session_entries.iter().any(|entry| entry.action == "event.ingest.requested"),
        "Gateway should have event.ingest.requested for session");
    assert!(gateway_session_entries.iter().any(|entry| entry.action == "event.ingest.completed"),
        "Gateway should have event.ingest.completed for session");
    
    // Verify turn_id consistency - all gateway entries for this session should have matching turn_id
    let turn_ids: std::collections::HashSet<_> = gateway_session_entries
        .iter()
        .filter_map(|e| e.turn_id.as_ref())
        .collect();
    assert!(turn_ids.len() <= 1, 
        "All gateway entries for same session should share turn_id, found: {:?}", turn_ids);

    let agent_entries = read_causal_entries(
        &agents_dir
            .join(target_agent_id)
            .join("history")
            .join("causal_chain.jsonl"),
    )?;
    
    // Per-turn correlation: agent entries should match session AND turn
    let agent_session_entries: Vec<_> = agent_entries
        .iter()
        .filter(|e| e.session_id == session_id)
        .collect();
    
    assert!(agent_session_entries.iter().any(|entry| entry.category == "session" && entry.action == "start"),
        "Agent should have session start for {}", session_id);
    assert!(agent_session_entries.iter().any(|entry| entry.category == "session" && entry.action == "end"),
        "Agent should have session end for {}", session_id);
    
    // Cross-source turn correlation: gateway and agent entries should share turn_id
    if let (Some(gateway_turn), Some(agent_turn)) = (
        gateway_session_entries.first().and_then(|e| e.turn_id.as_ref()),
        agent_session_entries.first().and_then(|e| e.turn_id.as_ref()),
    ) {
        assert_eq!(gateway_turn, agent_turn,
            "Gateway and agent should share turn_id for proper per-turn correlation");
    }

    server.abort();
    Ok(())
}
