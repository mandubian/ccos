use std::path::Path;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use super::common::{CliAgentExecutor, mcp_registry_path, load_mcp_servers, save_mcp_servers};
use autonoetic_mcp::{
    AgentMcpServer, ExposedAgent, McpClient, McpServer, McpTransportConfig,
};
use autonoetic_mcp::protocol::{
    JsonRpcRequest as McpJsonRpcRequest, JsonRpcResponse as McpJsonRpcResponse,
};

pub async fn handle_mcp_add(
    config_path: &Path,
    server_name: String,
    command: Option<String>,
    sse_url: Option<String>,
    args: Vec<String>,
) -> anyhow::Result<()> {
    let command = command.clone();
    let transport = match (command.clone(), sse_url.clone()) {
        (Some(_), None) => McpTransportConfig::Stdio,
        (None, Some(url)) => McpTransportConfig::Sse { url },
        (Some(_), Some(_)) => {
            anyhow::bail!(
                "Specify exactly one MCP transport: either --command or --sse-url"
            )
        }
        (None, None) => {
            anyhow::bail!("Missing MCP transport: provide --command or --sse-url")
        }
    };

    let server = McpServer {
        name: server_name.clone(),
        command: command.unwrap_or_default(),
        args: args.clone(),
        transport,
    };

    let mut client = McpClient::connect(&server).await?;
    let tools = client.list_tools().await?;
    let registry_path = mcp_registry_path(config_path);
    let mut servers = load_mcp_servers(&registry_path)?;

    if let Some(existing) = servers.iter_mut().find(|s| s.name == *server_name) {
        *existing = server;
    } else {
        servers.push(server);
    }
    save_mcp_servers(&registry_path, &servers)?;

    println!(
        "Registered MCP server '{}' with {} discovered tool(s).",
        server_name,
        tools.len()
    );
    for t in tools {
        println!(" - {}", t.name);
    }
    Ok(())
}

pub async fn handle_mcp_expose(agent_id: &str, config_path: &Path) -> anyhow::Result<()> {
    let config = autonoetic_gateway::config::load_config(config_path)?;
    let repo = autonoetic_gateway::AgentRepository::from_config(&config);
    let loaded = repo.get(agent_id).await?;

    let mut server = AgentMcpServer::new(CliAgentExecutor {
        agents_dir: config.agents_dir,
        client: reqwest::Client::new(),
    });
    server.register_agent(ExposedAgent {
        id: loaded.manifest.agent.id,
        name: loaded.manifest.agent.name,
        description: loaded.manifest.agent.description,
    });

    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    let mut stdout = tokio::io::stdout();
    while let Some(line) = lines.next_line().await? {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let response = match serde_json::from_str::<McpJsonRpcRequest>(trimmed) {
            Ok(req) => server.handle(req).await,
            Err(e) => McpJsonRpcResponse::err(
                serde_json::Value::Null,
                -32700,
                format!("Parse error: {}", e),
            ),
        };

        let encoded = serde_json::to_vec(&response)?;
        stdout.write_all(&encoded).await?;
        stdout.write_all(b"\n").await?;
        stdout.flush().await?;
    }
    Ok(())
}
