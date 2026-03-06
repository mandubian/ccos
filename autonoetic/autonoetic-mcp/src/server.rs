//! MCP server-side adapter exposing Autonoetic agents as tools.

use crate::protocol::{JsonRpcRequest, JsonRpcResponse};
use crate::types::{ExposedAgent, McpTool};
use async_trait::async_trait;
use std::collections::HashMap;

/// Execution interface used by MCP server adapter to call agents.
#[async_trait]
pub trait AgentExecutor: Send + Sync {
    async fn call_agent(&self, agent_id: &str, message: &str) -> anyhow::Result<String>;
}

/// In-memory registry exposing local agents as MCP tools.
pub struct AgentMcpServer<E: AgentExecutor> {
    executor: E,
    agents: HashMap<String, ExposedAgent>,
}

impl<E: AgentExecutor> AgentMcpServer<E> {
    pub fn new(executor: E) -> Self {
        Self {
            executor,
            agents: HashMap::new(),
        }
    }

    pub fn register_agent(&mut self, agent: ExposedAgent) {
        self.agents.insert(agent.id.clone(), agent);
    }

    pub fn list_tools(&self) -> Vec<McpTool> {
        self.agents
            .values()
            .map(|agent| McpTool {
                name: format!("autonoetic_agent_{}", agent.name),
                description: Some(agent.description.clone()),
                input_schema: Some(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "message": {
                            "type": "string",
                            "description": "Message sent to the agent"
                        }
                    },
                    "required": ["message"]
                })),
            })
            .collect()
    }

    /// Handle one MCP JSON-RPC request and return JSON-RPC response.
    pub async fn handle(&self, req: JsonRpcRequest) -> JsonRpcResponse {
        match req.method.as_str() {
            "tools/list" => JsonRpcResponse::ok(req.id, serde_json::json!({ "tools": self.list_tools() })),
            "tools/call" => match self.handle_tools_call(&req.params).await {
                Ok(result) => JsonRpcResponse::ok(req.id, result),
                Err(e) => JsonRpcResponse::err(req.id, -32602, e.to_string()),
            },
            _ => JsonRpcResponse::err(req.id, -32601, "Method not found"),
        }
    }

    async fn handle_tools_call(&self, params: &serde_json::Value) -> anyhow::Result<serde_json::Value> {
        let tool_name = params
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing tool name"))?;
        let args = params
            .get("arguments")
            .ok_or_else(|| anyhow::anyhow!("Missing tool arguments"))?;
        let message = args
            .get("message")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'message' argument"))?;

        let agent = self
            .resolve_agent_for_tool(tool_name)
            .ok_or_else(|| anyhow::anyhow!("Unknown tool '{}'", tool_name))?;
        let text = self.executor.call_agent(&agent.id, message).await?;
        Ok(serde_json::json!({ "content": [{ "type": "text", "text": text }] }))
    }

    fn resolve_agent_for_tool(&self, tool_name: &str) -> Option<&ExposedAgent> {
        self.agents.values().find(|agent| {
            tool_name == format!("autonoetic_agent_{}", agent.name)
                || tool_name == format!("autonoetic_agent_{}", agent.id)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockExec;

    #[async_trait]
    impl AgentExecutor for MockExec {
        async fn call_agent(&self, agent_id: &str, message: &str) -> anyhow::Result<String> {
            Ok(format!("{}:{}", agent_id, message))
        }
    }

    #[tokio::test]
    async fn test_tools_list_and_call() {
        let mut server = AgentMcpServer::new(MockExec);
        server.register_agent(ExposedAgent {
            id: "agent-1".to_string(),
            name: "coder".to_string(),
            description: "Writes code".to_string(),
        });

        let list_req = JsonRpcRequest::new(1, "tools/list", serde_json::json!({}));
        let list_resp = server.handle(list_req).await;
        let tools = list_resp.result.unwrap()["tools"].as_array().unwrap().clone();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["name"], "autonoetic_agent_coder");

        let call_req = JsonRpcRequest::new(
            2,
            "tools/call",
            serde_json::json!({
                "name": "autonoetic_agent_coder",
                "arguments": {"message": "hello"}
            }),
        );
        let call_resp = server.handle(call_req).await;
        assert!(call_resp.error.is_none());
        let text = call_resp.result.unwrap()["content"][0]["text"]
            .as_str()
            .unwrap()
            .to_string();
        assert_eq!(text, "agent-1:hello");
    }
}
