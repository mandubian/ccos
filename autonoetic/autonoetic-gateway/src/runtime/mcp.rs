//! MCP tool dispatcher for agent runtime.
//!
//! Loads registered MCP servers from a registry file, discovers tools, and
//! dispatches `mcp_<server>_<tool>` calls during the agent execution loop.

use crate::llm::ToolDefinition;
use autonoetic_mcp::{McpClient, McpServer, McpTool};
use std::collections::HashMap;
use std::path::PathBuf;

const MCP_REGISTRY_PATH_ENV: &str = "AUTONOETIC_MCP_REGISTRY_PATH";

pub struct McpToolRuntime {
    clients: HashMap<String, McpClient>,
    tools_by_name: HashMap<String, McpTool>,
    tool_server: HashMap<String, String>,
}

impl McpToolRuntime {
    /// Load MCP runtime from the registry path provided in env.
    ///
    /// If the env variable is absent or the file does not exist, returns an
    /// empty runtime (no MCP tools available).
    pub async fn from_env() -> anyhow::Result<Self> {
        let Ok(path) = std::env::var(MCP_REGISTRY_PATH_ENV) else {
            tracing::debug!("{} is not set; MCP runtime disabled", MCP_REGISTRY_PATH_ENV);
            return Ok(Self::empty());
        };
        Self::from_registry_path(PathBuf::from(path)).await
    }

    pub fn empty() -> Self {
        Self {
            clients: HashMap::new(),
            tools_by_name: HashMap::new(),
            tool_server: HashMap::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.tools_by_name.is_empty()
    }

    pub fn has_tool(&self, tool_name: &str) -> bool {
        self.tools_by_name.contains_key(tool_name)
    }

    pub fn tool_definitions(&self) -> anyhow::Result<Vec<ToolDefinition>> {
        let mut defs = Vec::with_capacity(self.tools_by_name.len());
        for tool in self.tools_by_name.values() {
            let description = tool
                .description
                .clone()
                .ok_or_else(|| anyhow::anyhow!("MCP tool '{}' missing description", tool.name))?;
            let input_schema = tool
                .input_schema
                .clone()
                .ok_or_else(|| anyhow::anyhow!("MCP tool '{}' missing input_schema", tool.name))?;
            defs.push(ToolDefinition {
                name: tool.name.clone(),
                description,
                input_schema,
            });
        }
        Ok(defs)
    }

    pub async fn call_tool(
        &mut self,
        tool_name: &str,
        arguments_json: &str,
    ) -> anyhow::Result<String> {
        let server_name = self
            .tool_server
            .get(tool_name)
            .ok_or_else(|| anyhow::anyhow!("Unknown MCP tool '{}'", tool_name))?
            .to_string();
        let client = self
            .clients
            .get_mut(&server_name)
            .ok_or_else(|| anyhow::anyhow!("MCP server client '{}' not found", server_name))?;

        let arguments: serde_json::Value = serde_json::from_str(arguments_json).map_err(|e| {
            anyhow::anyhow!("Invalid JSON arguments for tool '{}': {}", tool_name, e)
        })?;
        let result = client.call_tool(tool_name, arguments).await?;
        Ok(serde_json::to_string(&result.payload)?)
    }

    async fn from_registry_path(path: PathBuf) -> anyhow::Result<Self> {
        if !path.exists() {
            tracing::debug!(
                "MCP registry path {} not found; MCP runtime disabled",
                path.display()
            );
            return Ok(Self::empty());
        }
        let raw = std::fs::read_to_string(&path)?;
        let servers: Vec<McpServer> = serde_json::from_str(&raw)?;

        let mut clients = HashMap::new();
        let mut tools_by_name = HashMap::new();
        let mut tool_server = HashMap::new();

        for server in servers {
            let server_name = server.name.clone();
            let mut client = McpClient::connect(&server).await?;
            let tools = client.list_tools().await?;
            for tool in tools {
                if tools_by_name.contains_key(&tool.name) {
                    anyhow::bail!("Duplicate MCP tool name '{}'", tool.name);
                }
                tool_server.insert(tool.name.clone(), server_name.clone());
                tools_by_name.insert(tool.name.clone(), tool);
            }
            clients.insert(server_name, client);
        }

        Ok(Self {
            clients,
            tools_by_name,
            tool_server,
        })
    }
}
