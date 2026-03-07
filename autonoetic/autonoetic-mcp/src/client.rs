//! MCP client runtime with stdio and SSE transports.

use crate::protocol::{JsonRpcRequest, JsonRpcResponse};
use crate::types::{McpServer, McpTool, McpToolCallResult, McpTransportConfig};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

enum Transport {
    Stdio(StdioTransport),
    Sse {
        client: reqwest::Client,
        url: String,
    },
}

struct StdioTransport {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl StdioTransport {
    async fn send(&mut self, req: &JsonRpcRequest) -> anyhow::Result<JsonRpcResponse> {
        let payload = serde_json::to_vec(req)?;
        self.stdin.write_all(&payload).await?;
        self.stdin.write_all(b"\n").await?;
        self.stdin.flush().await?;

        let mut line = String::new();
        loop {
            line.clear();
            let bytes = self.stdout.read_line(&mut line).await?;
            if bytes == 0 {
                anyhow::bail!("MCP stdio process exited before returning response");
            }
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Ok(resp) = serde_json::from_str::<JsonRpcResponse>(trimmed) {
                if resp.id == req.id {
                    return Ok(resp);
                }
            }
        }
    }
}

impl Drop for StdioTransport {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
    }
}

/// Gateway-side MCP client.
pub struct McpClient {
    server_name: String,
    next_id: u64,
    transport: Transport,
}

impl McpClient {
    /// Connect to a configured MCP server.
    pub async fn connect(server: &McpServer) -> anyhow::Result<Self> {
        let transport = match &server.transport {
            McpTransportConfig::Stdio => {
                let mut child = Command::new(&server.command)
                    .args(&server.args)
                    .stdin(std::process::Stdio::piped())
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::null())
                    .spawn()?;
                let stdin = child
                    .stdin
                    .take()
                    .ok_or_else(|| anyhow::anyhow!("Failed to open MCP child stdin"))?;
                let stdout = child
                    .stdout
                    .take()
                    .ok_or_else(|| anyhow::anyhow!("Failed to open MCP child stdout"))?;
                Transport::Stdio(StdioTransport {
                    child,
                    stdin,
                    stdout: BufReader::new(stdout),
                })
            }
            McpTransportConfig::Sse { url } => Transport::Sse {
                client: reqwest::Client::new(),
                url: url.clone(),
            },
        };
        Ok(Self {
            server_name: server.name.clone(),
            next_id: 1,
            transport,
        })
    }

    async fn request(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> anyhow::Result<serde_json::Value> {
        let req = JsonRpcRequest::new(self.next_id, method, params);
        self.next_id += 1;

        let response = match &mut self.transport {
            Transport::Stdio(t) => t.send(&req).await?,
            Transport::Sse { client, url } => {
                let resp = client.post(url.as_str()).json(&req).send().await?;
                if !resp.status().is_success() {
                    anyhow::bail!("MCP SSE transport HTTP error {}", resp.status());
                }
                resp.json::<JsonRpcResponse>().await?
            }
        };
        response.into_result()
    }

    /// Discover tools from an MCP server and namespace them.
    pub async fn list_tools(&mut self) -> anyhow::Result<Vec<McpTool>> {
        let result = self.request("tools/list", serde_json::json!({})).await?;
        let tools = result
            .get("tools")
            .and_then(|v| v.as_array())
            .or_else(|| result.as_array())
            .ok_or_else(|| anyhow::anyhow!("Invalid tools/list response shape"))?;

        let mut discovered = Vec::with_capacity(tools.len());
        for t in tools {
            let raw_name = t
                .get("name")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Tool entry missing name"))?;
            let namespaced = format!("mcp_{}_{}", self.server_name, raw_name);
            discovered.push(McpTool {
                name: namespaced,
                description: t
                    .get("description")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
                input_schema: t
                    .get("inputSchema")
                    .cloned()
                    .or_else(|| t.get("input_schema").cloned()),
            });
        }
        Ok(discovered)
    }

    /// Execute a discovered MCP tool.
    pub async fn call_tool(
        &mut self,
        namespaced_tool_name: &str,
        arguments: serde_json::Value,
    ) -> anyhow::Result<McpToolCallResult> {
        let remote_name = strip_tool_namespace(&self.server_name, namespaced_tool_name);
        let payload = self
            .request(
                "tools/call",
                serde_json::json!({
                    "name": remote_name,
                    "arguments": arguments
                }),
            )
            .await?;
        Ok(McpToolCallResult { payload })
    }
}

fn strip_tool_namespace(server_name: &str, tool_name: &str) -> String {
    let prefix = format!("mcp_{}_", server_name);
    tool_name
        .strip_prefix(&prefix)
        .map(|s| s.to_string())
        .unwrap_or_else(|| tool_name.to_string())
}

#[cfg(test)]
mod tests {
    use super::strip_tool_namespace;

    #[test]
    fn test_strip_tool_namespace_exact_match() {
        let out = strip_tool_namespace("search", "mcp_search_web_lookup");
        assert_eq!(out, "web_lookup");
    }

    #[test]
    fn test_strip_tool_namespace_no_prefix() {
        let out = strip_tool_namespace("search", "web_lookup");
        assert_eq!(out, "web_lookup");
    }
}
