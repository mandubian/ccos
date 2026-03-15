//! Integration tests for schema enforcement on agent.spawn.
//!
//! Note: These tests verify the schema enforcement hook is in place.
//! The actual enforcement requires the caller's payload to match the target's io.accepts schema.
//!
//! WARNING: Tests using EnvGuard must run serially to avoid environment variable races.

mod support;

use support::{spawn_gateway_server, EnvGuard, JsonRpcClient, OpenAiStub, TestWorkspace};
use autonoetic_types::config::GatewayConfig;

const LLM_BASE_URL_OVERRIDE_ENV: &str = "AUTONOETIC_LLM_BASE_URL";
const LLM_API_KEY_OVERRIDE_ENV: &str = "AUTONOETIC_LLM_API_KEY";

fn install_target_agent_with_schema(agent_dir: &std::path::Path, agent_id: &str) -> anyhow::Result<()> {
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
  description: "Target agent with input schema"
io:
  accepts:
    type: object
    properties:
      message:
        type: string
        description: "The message to process"
      priority:
        type: integer
        default: 5
capabilities: []
llm_config:
  provider: "openai"
  model: "test-model"
  temperature: 0.0
---
# Target Agent
Reply with "Done".
"#),
    )?;
    std::fs::write(
        agent_dir.join("runtime.lock"),
        "dependencies: []",
    )?;
    Ok(())
}

/// Tests using EnvGuard must run serially to avoid environment variable races.
#[tokio::test]
#[serial_test::serial]
async fn test_schema_enforcement_hook_in_place() -> anyhow::Result<()> {
    let workspace = TestWorkspace::new()?;
    
    let target_id = "target-with-schema";
    install_target_agent_with_schema(&workspace.agents_dir.join(target_id), target_id)?;

    let stub = OpenAiStub::spawn(|_, _| async move {
        serde_json::json!({
            "choices": [{
                "message": { "content": "Done" },
                "finish_reason": "stop"
            }],
            "usage": { "prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2 }
        })
    }).await?;
    
    // Set environment BEFORE spawning gateway - child processes inherit at spawn time
    let _env = EnvGuard::set(LLM_BASE_URL_OVERRIDE_ENV, stub.completion_url());
    let _key = EnvGuard::set(LLM_API_KEY_OVERRIDE_ENV, "test-key");

    let (server_addr, _shutdown) = spawn_gateway_server(GatewayConfig {
        agents_dir: workspace.agents_dir.clone(),
        ..workspace.gateway_config()
    }).await?;
    
    // Give gateway time to fully start
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    
    let mut client = JsonRpcClient::connect(server_addr).await?;

    let response = client
        .agent_spawn(
            "test-spawn-1",
            target_id,
            "do the task",
            None,
            Some("session-enforcement-test"),
        )
        .await?;

    assert!(response.error.is_none(), "Should not error: {:?}", response.error);
    
    Ok(())
}

/// Tests using EnvGuard must run serially to avoid environment variable races.
#[tokio::test]
#[serial_test::serial]
async fn test_schema_enforcement_with_disabled_mode() -> anyhow::Result<()> {
    let workspace = TestWorkspace::new()?;
    
    let target_id = "target-no-schema";
    let target_dir = workspace.agents_dir.join(target_id);
    std::fs::create_dir_all(&target_dir)?;
    std::fs::write(
        target_dir.join("SKILL.md"),
        r#"---
version: "1.0"
runtime:
  engine: "autonoetic"
  gateway_version: "0.1.0"
  sdk_version: "0.1.0"
  type: "stateful"
  sandbox: "bubblewrap"
  runtime_lock: "runtime.lock"
agent:
  id: "target-no-schema"
  name: "target-no-schema"
  description: "Target without io schema"
capabilities: []
llm_config:
  provider: "openai"
  model: "test-model"
  temperature: 0.0
---
# Target Agent
Reply with "Done".
"#,
    )?;
    std::fs::write(target_dir.join("runtime.lock"), "dependencies: []")?;

    let stub = OpenAiStub::spawn(|_, _| async move {
        serde_json::json!({
            "choices": [{
                "message": { "content": "Done" },
                "finish_reason": "stop"
            }],
            "usage": { "prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2 }
        })
    }).await?;
    
    let _env = EnvGuard::set(LLM_BASE_URL_OVERRIDE_ENV, stub.completion_url());
    let _key = EnvGuard::set(LLM_API_KEY_OVERRIDE_ENV, "test-key");

    let (server_addr, _shutdown) = spawn_gateway_server(GatewayConfig {
        agents_dir: workspace.agents_dir.clone(),
        ..workspace.gateway_config()
    }).await?;
    let mut client = JsonRpcClient::connect(server_addr).await?;

    let response = client
        .agent_spawn(
            "test-spawn-2",
            target_id,
            "do the task",
            None,
            Some("session-no-schema-test"),
        )
        .await?;

    assert!(response.error.is_none(), "Should not error without schema: {:?}", response.error);
    
    Ok(())
}
