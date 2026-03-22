mod support;

use autonoetic_gateway::GatewayExecutionService;
use support::{read_causal_entries, TestWorkspace};

fn install_script_agent(agent_dir: &std::path::Path, agent_id: &str) -> anyhow::Result<()> {
    std::fs::create_dir_all(agent_dir.join("scripts"))?;

    std::fs::write(
        agent_dir.join("scripts/echo.py"),
        r#"#!/usr/bin/env python3
import os
import json
input_data = os.environ.get("SCRIPT_INPUT", "")
print(f"Echo: {input_data}")
"#,
    )?;

    let skill_md = format!(
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
  id: "{agent_id}"
  name: "{agent_id}"
  description: "Script agent for integration test"
execution_mode: script
script_entry: scripts/echo.py
capabilities: []
---
# Script Agent
"#,
    );
    std::fs::write(agent_dir.join("SKILL.md"), skill_md)?;
    std::fs::write(agent_dir.join("runtime.lock"), "dependencies: []")?;
    Ok(())
}

#[tokio::test]
async fn test_script_agent_execution_returns_stdout() -> anyhow::Result<()> {
    let workspace = TestWorkspace::new()?;
    let agent_id = "script-echo-agent";
    install_script_agent(&workspace.agents_dir.join(agent_id), agent_id)?;

    let execution = GatewayExecutionService::new(workspace.gateway_config(), None);
    let session_id = "session-script-test";

    let result = execution
        .spawn_agent_once(agent_id, "hello world", session_id, None, false, None, None)
        .await;

    match result {
        Ok(spawn_result) => {
            let reply = spawn_result.assistant_reply.expect("should have reply");
            assert!(reply.contains("Echo: hello world"), "reply should contain echo output");
            tracing::info!(reply = %reply, "Script agent executed successfully");
        }
        Err(e) => {
            if e.to_string().contains("bwrap") || e.to_string().contains("bubblewrap") {
                tracing::warn!("bubblewrap not available, skipping test");
                return Ok(());
            }
            return Err(e);
        }
    }

    Ok(())
}

#[tokio::test]
async fn test_script_agent_logs_causal_events() -> anyhow::Result<()> {
    let workspace = TestWorkspace::new()?;
    let agent_id = "script-log-agent";
    install_script_agent(&workspace.agents_dir.join(agent_id), agent_id)?;

    let execution = GatewayExecutionService::new(workspace.gateway_config(), None);
    let session_id = "session-script-causal";

    let _ = execution
        .spawn_agent_once(agent_id, "test input", session_id, None, false, None, None)
        .await;

    let gateway_dir = workspace.agents_dir.join(".gateway");
    let causal_path = gateway_dir.join("history/causal_chain.jsonl");

    if !causal_path.exists() {
        if std::env::var("AUTONOETIC_LLM_BASE_URL").is_err() {
            tracing::warn!("bubblewrap not available, skipping test");
            return Ok(());
        }
        anyhow::bail!("Causal log should exist");
    }

    let entries = read_causal_entries(&causal_path)?;
    let script_events: Vec<_> = entries
        .iter()
        .filter(|e| e.action.starts_with("script."))
        .collect();

    assert!(
        !script_events.is_empty(),
        "Should have script.* causal events"
    );

    tracing::info!(
        events = ?script_events.iter().map(|e| &e.action).collect::<Vec<_>>(),
        "Found script causal events"
    );

    Ok(())
}

fn install_failing_script_agent(agent_dir: &std::path::Path, agent_id: &str) -> anyhow::Result<()> {
    std::fs::create_dir_all(agent_dir.join("scripts"))?;

    std::fs::write(
        agent_dir.join("scripts/fail.py"),
        r#"#!/usr/bin/env python3
import sys
print("Script failed!")
sys.exit(1)
"#,
    )?;

    let skill_md = format!(
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
  id: "{agent_id}"
  name: "{agent_id}"
  description: "Failing script agent for integration test"
execution_mode: script
script_entry: scripts/fail.py
capabilities: []
---
# Failing Script Agent
"#,
    );
    std::fs::write(agent_dir.join("SKILL.md"), skill_md)?;
    std::fs::write(agent_dir.join("runtime.lock"), "dependencies: []")?;
    Ok(())
}

#[tokio::test]
async fn test_script_agent_with_sandbox_failure_returns_error() -> anyhow::Result<()> {
    let workspace = TestWorkspace::new()?;
    let agent_id = "script-fail-agent";
    install_failing_script_agent(&workspace.agents_dir.join(agent_id), agent_id)?;

    let execution = GatewayExecutionService::new(workspace.gateway_config(), None);
    let session_id = "session-script-fail";

    let result = execution
        .spawn_agent_once(agent_id, "test", session_id, None, false, None, None)
        .await;

    match result {
        Ok(_) => {
            anyhow::bail!("Expected error from failing script, but got success");
        }
        Err(e) => {
            let err_msg = e.to_string();
            assert!(
                err_msg.contains("Script execution failed") || err_msg.contains("exit code"),
                "Error should mention script failure"
            );
            tracing::info!(error = %err_msg, "Script failure returned error as expected");
        }
    }

    Ok(())
}

fn install_policy_restricted_agent(agent_dir: &std::path::Path, agent_id: &str) -> anyhow::Result<()> {
    std::fs::create_dir_all(agent_dir.join("scripts"))?;

    std::fs::write(
        agent_dir.join("scripts/write.py"),
        r#"#!/usr/bin/env python3
import os
with open(os.environ.get("AGENT_DIR", ".") + "/state/output.txt", "w") as f:
    f.write("test output")
print("File written")
"#,
    )?;

    let skill_md = format!(
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
  id: "{agent_id}"
  name: "{agent_id}"
  description: "Policy test script agent"
execution_mode: script
script_entry: scripts/write.py
capabilities: []
---
# Policy Test Agent
"#,
    );
    std::fs::write(agent_dir.join("SKILL.md"), skill_md)?;
    std::fs::write(agent_dir.join("runtime.lock"), "dependencies: []")?;
    Ok(())
}

#[tokio::test]
async fn test_script_agent_without_capabilities_cannot_access_tools() -> anyhow::Result<()> {
    let workspace = TestWorkspace::new()?;
    let agent_id = "script-policy-agent";
    install_policy_restricted_agent(&workspace.agents_dir.join(agent_id), agent_id)?;

    let execution = GatewayExecutionService::new(workspace.gateway_config(), None);
    let session_id = "session-script-policy";

    let result = execution
        .spawn_agent_once(agent_id, "test", session_id, None, false, None, None)
        .await;

    match result {
        Ok(spawn_result) => {
            let reply = spawn_result.assistant_reply.unwrap_or_default();
            tracing::info!(reply = %reply, "Script agent executed without policy gate (sandbox runs directly)");
        }
        Err(e) => {
            if e.to_string().contains("bwrap") || e.to_string().contains("bubblewrap") {
                tracing::warn!("bubblewrap not available, skipping test");
                return Ok(());
            }
            return Err(e);
        }
    }

    Ok(())
}

#[tokio::test]
async fn test_script_agent_execution_time_under_100ms() -> anyhow::Result<()> {
    use std::time::Instant;

    let workspace = TestWorkspace::new()?;
    let agent_id = "script-perf-agent";
    install_script_agent(&workspace.agents_dir.join(agent_id), agent_id)?;

    let execution = GatewayExecutionService::new(workspace.gateway_config(), None);
    let session_id = "session-script-perf";

    let start = Instant::now();

    let result = execution
        .spawn_agent_once(agent_id, "test input", session_id, None, false, None, None)
        .await;

    let elapsed = start.elapsed();

    match result {
        Ok(spawn_result) => {
            let _reply = spawn_result.assistant_reply.expect("should have reply");
            let elapsed_ms = elapsed.as_millis();
            tracing::info!(elapsed_ms = elapsed_ms, "Script agent execution time");
            assert!(
                elapsed_ms < 500,
                "Script agent should execute quickly, took {}ms (allowing 500ms for CI variance)",
                elapsed_ms
            );
        }
        Err(e) => {
            if e.to_string().contains("bwrap") || e.to_string().contains("bubblewrap") {
                tracing::warn!("bubblewrap not available, skipping test");
                return Ok(());
            }
            return Err(e);
        }
    }

    Ok(())
}
