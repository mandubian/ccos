//! Integration tests for MCP runtime wiring in gateway.
//!
//! Run with:
//!   cargo test -p autonoetic-gateway --test mcp_integration -- --nocapture

use autonoetic_gateway::runtime::mcp::McpToolRuntime;
use autonoetic_mcp::protocol::JsonRpcRequest;
use autonoetic_mcp::{AgentExecutor, AgentMcpServer, ExposedAgent, McpServer, McpTransportConfig};
use tempfile::tempdir;

fn write_mock_stdio_mcp_server_script(script_path: &std::path::Path) -> anyhow::Result<()> {
    let script = r#"#!/usr/bin/env bash
set -euo pipefail
while IFS= read -r line; do
  id="$(printf '%s' "$line" | sed -n 's/.*"id":[[:space:]]*\([0-9][0-9]*\).*/\1/p')"
  if [[ -z "${id}" ]]; then
    id=1
  fi

  if [[ "$line" == *"\"tools/list\""* ]]; then
    echo "{\"jsonrpc\":\"2.0\",\"id\":${id},\"result\":{\"tools\":[{\"name\":\"echo\",\"description\":\"Echo input\",\"inputSchema\":{\"type\":\"object\",\"properties\":{\"text\":{\"type\":\"string\"}},\"required\":[\"text\"]}}]}}"
  elif [[ "$line" == *"\"tools/call\""* ]]; then
    echo "{\"jsonrpc\":\"2.0\",\"id\":${id},\"result\":{\"content\":[{\"type\":\"text\",\"text\":\"mock-echo-ok\"}]}}"
  else
    echo "{\"jsonrpc\":\"2.0\",\"id\":${id},\"error\":{\"code\":-32601,\"message\":\"Method not found\"}}"
  fi
done
"#;
    std::fs::write(script_path, script)?;
    Ok(())
}

struct MockAgentExec;

#[async_trait::async_trait]
impl AgentExecutor for MockAgentExec {
    async fn call_agent(&self, agent_id: &str, message: &str) -> anyhow::Result<String> {
        Ok(format!("agent={} message={}", agent_id, message))
    }
}

#[tokio::test]
async fn test_mcp_integration_loads_existing_server_and_exposes_agent_tool() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let script_path = tmp.path().join("mock-mcp.sh");
    let registry_path = tmp.path().join("mcp_servers.json");
    write_mock_stdio_mcp_server_script(&script_path)?;

    let servers = vec![McpServer {
        name: "mock".to_string(),
        command: "bash".to_string(),
        args: vec![script_path.display().to_string()],
        transport: McpTransportConfig::Stdio,
    }];
    std::fs::write(&registry_path, serde_json::to_vec(&servers)?)?;

    // 1) Gateway runtime loads existing MCP server and dispatches tool call.
    let old_registry = std::env::var("AUTONOETIC_MCP_REGISTRY_PATH").ok();
    std::env::set_var(
        "AUTONOETIC_MCP_REGISTRY_PATH",
        registry_path.display().to_string(),
    );

    let mut runtime = McpToolRuntime::from_env().await?;
    assert!(!runtime.is_empty(), "Expected MCP runtime to load tools");

    let defs = runtime.tool_definitions()?;
    assert!(
        defs.iter().any(|d| d.name == "mcp_mock_echo"),
        "Expected namespaced MCP tool mcp_mock_echo"
    );

    let call_result = runtime
        .call_tool("mcp_mock_echo", r#"{"text":"hello"}"#)
        .await?;
    let call_json: serde_json::Value = serde_json::from_str(&call_result)?;
    assert_eq!(call_json["content"][0]["text"], "mock-echo-ok");

    match old_registry {
        Some(v) => std::env::set_var("AUTONOETIC_MCP_REGISTRY_PATH", v),
        None => std::env::remove_var("AUTONOETIC_MCP_REGISTRY_PATH"),
    }

    // 2) MCP server side exposes agent as callable MCP tool.
    let mut agent_server = AgentMcpServer::new(MockAgentExec);
    agent_server.register_agent(ExposedAgent {
        id: "agent-42".to_string(),
        name: "researcher".to_string(),
        description: "Research specialist".to_string(),
    });

    let list_req = JsonRpcRequest::new(1, "tools/list", serde_json::json!({}));
    let list_resp = agent_server.handle(list_req).await;
    let tools = list_resp
        .result
        .ok_or_else(|| anyhow::anyhow!("Expected tools/list result"))?["tools"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("Expected tools array"))?
        .clone();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0]["name"], "autonoetic_agent_researcher");

    let call_req = JsonRpcRequest::new(
        2,
        "tools/call",
        serde_json::json!({
            "name": "autonoetic_agent_researcher",
            "arguments": { "message": "ping" }
        }),
    );
    let call_resp = agent_server.handle(call_req).await;
    let text = call_resp
        .result
        .ok_or_else(|| anyhow::anyhow!("Expected tools/call result"))?["content"][0]["text"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Expected text content"))?
        .to_string();
    assert_eq!(text, "agent=agent-42 message=ping");

    Ok(())
}
