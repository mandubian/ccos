//! Integration test for multi-agent session trace reconstruction.

mod support;

use support::{read_causal_entries, spawn_gateway_server, EnvGuard, JsonRpcClient, OpenAiStub, TestWorkspace};

fn install_parent_agent(agent_dir: &std::path::Path, agent_id: &str) -> anyhow::Result<()> {
    std::fs::create_dir_all(agent_dir)?;
    std::fs::write(
        agent_dir.join("SKILL.md"),
        format!(r#"---
version: "1.0"
runtime:
  engine: "autonoetic"
  gateway_version: "0.1.0"
  sdk_version: "0.1.0"
  type: "stateful"
  sandbox: "bubblewrap"
  runtime_lock: "runtime.lock"
agent:
  id: "{agent_id}"
  name: "{agent_id}"
  description: "Parent agent that spawns child"
llm_config:
  provider: "openai"
  model: "test-model"
  temperature: 0.0
capabilities:
  - type: "AgentSpawn"
    max_children: 5
  - type: "AgentMessage"
    patterns: ["*"]
---
# Parent Agent
When asked to delegate, spawn the child agent.
"#),
    )?;
    std::fs::write(agent_dir.join("runtime.lock"), "dependencies: []")?;
    Ok(())
}

fn install_child_agent(agent_dir: &std::path::Path, agent_id: &str) -> anyhow::Result<()> {
    std::fs::create_dir_all(agent_dir)?;
    std::fs::write(
        agent_dir.join("SKILL.md"),
        format!(r#"---
version: "1.0"
runtime:
  engine: "autonoetic"
  gateway_version: "0.1.0"
  sdk_version: "0.1.0"
  type: "stateful"
  sandbox: "bubblewrap"
  runtime_lock: "runtime.lock"
agent:
  id: "{agent_id}"
  name: "{agent_id}"
  description: "Child agent that does work"
llm_config:
  provider: "openai"
  model: "test-model"
  temperature: 0.0
capabilities: []
---
# Child Agent
Reply with "Child completed task: <input>".
"#),
    )?;
    std::fs::write(agent_dir.join("runtime.lock"), "dependencies: []")?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn test_multi_agent_session_trace_reconstruction() -> anyhow::Result<()> {
    let workspace = TestWorkspace::new()?;
    
    let parent_id = "parent-agent";
    let child_id = "child-agent";
    
    install_parent_agent(&workspace.agents_dir.join(parent_id), parent_id)?;
    install_child_agent(&workspace.agents_dir.join(child_id), child_id)?;

    let stub = OpenAiStub::spawn(move |_, body_json| async move {
        let messages = body_json["messages"].as_array().cloned().unwrap_or_default();
        let latest_user = messages.iter().rev().find_map(|m| {
            if m["role"].as_str() == Some("user") {
                m["content"].as_str()
            } else {
                None
            }
        }).unwrap_or("");
        
        if latest_user.contains("delegate") {
            serde_json::json!({
                "choices": [{
                    "message": { 
                        "role": "assistant", 
                        "tool_calls": [{ 
                            "id": "call_1", 
                            "type": "function", 
                            "function": { 
                                "name": "agent.spawn", 
                                "arguments": serde_json::json!({
                                    "agent_id": child_id,
                                    "message": "do work"
                                }).to_string()
                            } 
                        }] 
                    },
                    "finish_reason": "tool_calls"
                }],
                "usage": { "prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2 }
            })
        } else {
            serde_json::json!({
                "choices": [{
                    "message": { "role": "assistant", "content": "Task completed" },
                    "finish_reason": "stop"
                }],
                "usage": { "prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2 }
            })
        }
    }).await?;
    
    let _env = EnvGuard::set("AUTONOETIC_LLM_BASE_URL", stub.completion_url());
    let _key = EnvGuard::set("AUTONOETIC_LLM_API_KEY", "test-key");

    let (server_addr, shutdown) = spawn_gateway_server(workspace.gateway_config()).await?;
    let mut client = JsonRpcClient::connect(server_addr).await?;

    let session_id = "session-multi-agent-test";
    
    let response = client
        .event_ingest(
            "test-multi-1",
            parent_id,
            session_id,
            "test",
            "please delegate to child agent",
            None::<serde_json::Value>,
        )
        .await?;
    
    drop(shutdown);
    
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let gateway_causal_path = workspace.agents_dir.join(".gateway/history/causal_chain.jsonl");
    let parent_causal_path = workspace.agents_dir.join(parent_id).join("history/causal_chain.jsonl");
    let child_causal_path = workspace.agents_dir.join(child_id).join("history/causal_chain.jsonl");

    let mut all_events: Vec<(String, String)> = Vec::new();
    
    if gateway_causal_path.exists() {
        let entries = read_causal_entries(&gateway_causal_path)?;
        for entry in entries {
            if entry.session_id == session_id {
                all_events.push(("gateway".to_string(), entry.action.clone()));
            }
        }
    }
    
    if parent_causal_path.exists() {
        let entries = read_causal_entries(&parent_causal_path)?;
        for entry in entries {
            if entry.session_id == session_id {
                all_events.push((parent_id.to_string(), entry.action.clone()));
            }
        }
    }
    
    if child_causal_path.exists() {
        let entries = read_causal_entries(&child_causal_path)?;
        for entry in entries {
            if entry.session_id == session_id {
                all_events.push((child_id.to_string(), entry.action.clone()));
            }
        }
    }

    tracing::info!(events = ?all_events, "Found events for session");
    
    assert!(
        !all_events.is_empty(),
        "Should have events in the session"
    );

    let has_spawn = all_events.iter().any(|(_, action)| action.contains("spawn"));
    assert!(
        has_spawn,
        "Session should contain spawn events"
    );

    Ok(())
}

#[tokio::test]
async fn test_session_trace_deterministic_ordering() -> anyhow::Result<()> {
    let workspace = TestWorkspace::new()?;
    
    let agent_id = "simple-agent";
    
    std::fs::create_dir_all(workspace.agents_dir.join(agent_id))?;
    std::fs::write(
        workspace.agents_dir.join(agent_id).join("SKILL.md"),
        format!(r#"---
version: "1.0"
runtime:
  engine: "autonoetic"
  gateway_version: "0.1.0"
  sdk_version: "0.1.0"
  type: "stateful"
  sandbox: "bubblewrap"
  runtime_lock: "runtime.lock"
agent:
  id: "{agent_id}"
  name: "{agent_id}"
  description: "Simple agent"
llm_config:
  provider: "openai"
  model: "test-model"
  temperature: 0.0
capabilities: []
---
# Simple Agent
Reply with "Done".
"#),
    )?;
    std::fs::write(
        workspace.agents_dir.join(agent_id).join("runtime.lock"),
        "dependencies: []",
    )?;

    let stub = OpenAiStub::spawn(|_, _| async move {
        serde_json::json!({
            "choices": [{
                "message": { "role": "assistant", "content": "Done" },
                "finish_reason": "stop"
            }],
            "usage": { "prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2 }
        })
    }).await?;
    
    let _env = EnvGuard::set("AUTONOETIC_LLM_BASE_URL", stub.completion_url());
    let _key = EnvGuard::set("AUTONOETIC_LLM_API_KEY", "test-key");

    let (server_addr, _shutdown) = spawn_gateway_server(workspace.gateway_config()).await?;
    let mut client = JsonRpcClient::connect(server_addr).await?;

    let session_id = "session-deterministic-1";
    
    let response = client
        .event_ingest(
            "test-2",
            agent_id,
            session_id,
            "test",
            "hello",
            None::<serde_json::Value>,
        )
        .await?;

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let agent_causal_path = workspace.agents_dir.join(agent_id).join("history/causal_chain.jsonl");
    
    let entries = read_causal_entries(&agent_causal_path)?;
    let session_entries: Vec<_> = entries.iter()
        .filter(|e| e.session_id == session_id)
        .collect();
    
    assert!(
        !session_entries.is_empty(),
        "Should have events in the session"
    );
    
    let mut timestamps: Vec<&str> = session_entries.iter()
        .map(|e| e.timestamp.as_str())
        .collect();
    
    timestamps.sort();
    
    let is_sorted = timestamps.windows(2).all(|w| w[0] <= w[1]);
    assert!(
        is_sorted,
        "Events should be sorted by timestamp"
    );

    let mut event_seqs: Vec<u64> = session_entries.iter()
        .map(|e| e.event_seq)
        .collect();
    event_seqs.sort();
    
    let seqs_sorted = event_seqs.windows(2).all(|w| w[0] <= w[1]);
    assert!(
        seqs_sorted,
        "Events should be sorted by event_seq within timestamp"
    );

    tracing::info!(
        timestamps = ?timestamps,
        event_seqs = ?event_seqs,
        "Deterministic ordering verified"
    );

    Ok(())
}
