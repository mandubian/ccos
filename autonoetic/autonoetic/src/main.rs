use clap::{Parser, Subcommand, Args};
use tracing::info;

use autonoetic_gateway::llm::{CompletionRequest, Message};
use autonoetic_gateway::runtime::parser::SkillParser;
use autonoetic_mcp::protocol::{JsonRpcRequest, JsonRpcResponse};
use autonoetic_mcp::{
    AgentExecutor, AgentMcpServer, ExposedAgent, McpClient, McpServer, McpTool,
    McpTransportConfig,
};
use std::path::{Path, PathBuf};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[derive(Parser)]
#[command(name = "autonoetic", about = "CLI for managing the Autonoetic Agent System", version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Path to a custom config.yaml or policy.yaml (default: ~/.ccos/)
    #[arg(global = true, long)]
    config: Option<String>,

    /// Overrides the Gateway log level (trace, debug, info, warn, error)
    #[arg(global = true, long)]
    log_level: Option<String>,

    /// Disables all prompts (essential for CI/CD)
    #[arg(global = true, long)]
    non_interactive: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Manage the Gateway lifecycle
    Gateway(GatewayArgs),
    /// Manage Autonoetic Agents
    Agent(AgentArgs),
    /// Ecosystem and Skills management
    Skill(SkillArgs),
    /// Federation and Cluster management
    Federate(FederateArgs),
    /// MCP Integration management
    Mcp(McpArgs),
}

// ---------------------------------------------------------------------------
// Gateway
// ---------------------------------------------------------------------------

#[derive(Args)]
struct GatewayArgs {
    #[command(subcommand)]
    command: GatewayCommands,
}

#[derive(Subcommand)]
enum GatewayCommands {
    /// Starts the Gateway daemon in the foreground
    Start {
        /// Run in the background
        #[arg(short, long)]
        daemon: bool,
        /// Override the default HTTP/TCP ports
        #[arg(long)]
        port: Option<u16>,
        /// Force TLS wrapping on the OFP federation port
        #[arg(long)]
        tls: bool,
    },
    /// Gracefully terminates a background Gateway daemon
    Stop,
    /// Outputs a table of Gateway health, loaded policies, etc.
    Status {
        /// Emit machine-readable JSON output.
        #[arg(long)]
        json: bool,
    },
}

// ---------------------------------------------------------------------------
// Agent
// ---------------------------------------------------------------------------

#[derive(Args)]
struct AgentArgs {
    #[command(subcommand)]
    command: AgentCommands,
}

#[derive(Subcommand)]
enum AgentCommands {
    /// Scaffolds a new Autonoetic Agent directory
    Init {
        /// Agent ID to create
        agent_id: String,
        /// Template to use (e.g., researcher, coder, auditor)
        #[arg(long)]
        template: Option<String>,
    },
    /// Boots an Agent and connects it to the Gateway
    Run {
        /// Agent ID to run
        agent_id: String,
        /// Initial message kickoff
        message: Option<String>,
        /// Drops the user into a persistent chat loop
        #[arg(short, long)]
        interactive: bool,
        /// Boots the agent headless
        #[arg(long)]
        headless: bool,
    },
    /// Lists all local Agents registered with the Gateway
    List,
}

// ---------------------------------------------------------------------------
// Skill
// ---------------------------------------------------------------------------

#[derive(Args)]
struct SkillArgs {
    #[command(subcommand)]
    command: SkillCommands,
}

#[derive(Subcommand)]
enum SkillCommands {
    /// Downloads and installs an AgentSkills.io compliant bundle
    Install {
        /// GitHub URL or Skill ID
        url_or_id: String,
        /// Target agent ID
        #[arg(long)]
        agent: Option<String>,
    },
    /// Removes a skill from an Agent's capability list
    Uninstall {
        /// Name of the skill to uninstall
        skill_name: String,
        /// Target agent ID
        #[arg(long)]
        agent: String,
    },
}

// ---------------------------------------------------------------------------
// Federate
// ---------------------------------------------------------------------------

#[derive(Args)]
struct FederateArgs {
    #[command(subcommand)]
    command: FederateCommands,
}

#[derive(Subcommand)]
enum FederateCommands {
    /// Connects the local Gateway to a remote peer via OFP
    Join {
        /// Remote peer address
        peer_address: String,
    },
    /// Outputs the local PeerRegistry
    List,
}

// ---------------------------------------------------------------------------
// MCP
// ---------------------------------------------------------------------------

#[derive(Args)]
struct McpArgs {
    #[command(subcommand)]
    command: McpCommands,
}

#[derive(Subcommand)]
enum McpCommands {
    /// Registers a local MCP server with the Gateway
    Add {
        /// MCP Server name
        server_name: String,
        /// Subprocess command (stdio transport).
        #[arg(long)]
        command: Option<String>,
        /// Optional SSE endpoint transport URL.
        #[arg(long)]
        sse_url: Option<String>,
        /// Optional command arguments
        #[arg(last = true)]
        args: Vec<String>,
    },
    /// Temporarily runs the Gateway as an MCP Server on stdio
    Expose {
        /// Agent ID to expose
        agent_id: String,
    },
}

// ===========================================================================
// main
// ===========================================================================

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Tracing setup
    let log_level = cli.log_level.as_deref().unwrap_or("info");
    tracing_subscriber::fmt()
        .with_env_filter(format!("autonoetic={log_level},{log_level}"))
        .init();

    // Resolve config path
    let config_path = cli.config
        .map(PathBuf::from)
        .unwrap_or_else(|| dirs_or_default().join("config.yaml"));
    std::env::set_var(
        "AUTONOETIC_MCP_REGISTRY_PATH",
        mcp_registry_path(&config_path).display().to_string(),
    );

    match &cli.command {
        // ---- Gateway ----
        Commands::Gateway(args) => match &args.command {
            GatewayCommands::Start { daemon, port, tls } => {
                let config = autonoetic_gateway::config::load_config(&config_path)?;
                let agents = autonoetic_gateway::agent::scan_agents(&config.agents_dir)?;
                let mcp_runtime = activate_registered_mcp_servers(&config_path).await?;

                info!(
                    "Gateway starting — port: {}, agents: {}, daemon: {}, tls: {}",
                    port.unwrap_or(config.port),
                    agents.len(),
                    daemon,
                    tls,
                );

                for a in &agents {
                    info!("  Agent: {} ({})", a.id, a.dir.display());
                }
                for line in mcp_runtime.summary_lines() {
                    info!("{}", line);
                }

                // Start tokio event loop
                let server = autonoetic_gateway::GatewayServer::new(config);
                // Keep MCP clients alive while gateway runs.
                let _mcp_runtime = mcp_runtime;
                if let Err(e) = server.run().await {
                    tracing::error!("Gateway server error: {:?}", e);
                }
            }
            GatewayCommands::Stop => {
                info!("Stopping Gateway");
            }
            GatewayCommands::Status { json } => {
                print_gateway_status(&config_path, *json).await?;
            }
        },

        // ---- Agent ----
        Commands::Agent(args) => match &args.command {
            AgentCommands::Init { agent_id, template } => {
                info!("Initializing Agent {} (template: {:?})", agent_id, template);
            }
            AgentCommands::Run { agent_id, message, interactive, headless } => {
                info!("Running Agent {} (interactive: {}, headless: {})", agent_id, interactive, headless);
                if let Some(msg) = message {
                    info!("Kickoff message: {}", msg);
                }
            }
            AgentCommands::List => {
                let config = autonoetic_gateway::config::load_config(&config_path)?;
                let agents = autonoetic_gateway::agent::scan_agents(&config.agents_dir)?;
                if agents.is_empty() {
                    println!("No agents found in {}", config.agents_dir.display());
                } else {
                    println!("{:<30} {}", "AGENT ID", "DIRECTORY");
                    for a in &agents {
                        println!("{:<30} {}", a.id, a.dir.display());
                    }
                }
            }
        },

        // ---- Skill ----
        Commands::Skill(args) => match &args.command {
            SkillCommands::Install { url_or_id, agent } => {
                info!("Installing Skill {} (agent: {:?})", url_or_id, agent);
            }
            SkillCommands::Uninstall { skill_name, agent } => {
                info!("Uninstalling Skill {} from agent {}", skill_name, agent);
            }
        },

        // ---- Federate ----
        Commands::Federate(args) => match &args.command {
            FederateCommands::Join { peer_address } => {
                info!("Joining peer {}", peer_address);
            }
            FederateCommands::List => {
                info!("Listing peers");
            }
        },

        // ---- MCP ----
        Commands::Mcp(args) => match &args.command {
            McpCommands::Add {
                server_name,
                command,
                sse_url,
                args,
            } => {
                let transport = match (command, sse_url) {
                    (Some(_), None) => McpTransportConfig::Stdio,
                    (None, Some(url)) => McpTransportConfig::Sse { url: url.clone() },
                    (Some(_), Some(_)) => {
                        anyhow::bail!("Specify exactly one MCP transport: either --command or --sse-url")
                    }
                    (None, None) => anyhow::bail!("Missing MCP transport: provide --command or --sse-url"),
                };

                let server = McpServer {
                    name: server_name.clone(),
                    command: command.clone().unwrap_or_default(),
                    args: args.clone(),
                    transport,
                };

                let mut client = McpClient::connect(&server).await?;
                let tools = client.list_tools().await?;
                let registry_path = mcp_registry_path(&config_path);
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
            }
            McpCommands::Expose { agent_id } => {
                run_mcp_stdio_server(agent_id, &config_path).await?;
            }
        },
    }

    Ok(())
}

/// Resolve the default config directory (~/.ccos/).
fn dirs_or_default() -> PathBuf {
    dirs::home_dir()
        .map(|h| h.join(".ccos"))
        .unwrap_or_else(|| PathBuf::from(".ccos"))
}

fn mcp_registry_path(config_path: &Path) -> PathBuf {
    config_path
        .parent()
        .map(|p| p.join("mcp_servers.json"))
        .unwrap_or_else(|| PathBuf::from("mcp_servers.json"))
}

fn load_mcp_servers(path: &Path) -> anyhow::Result<Vec<McpServer>> {
    if !path.exists() {
        return Ok(vec![]);
    }
    let raw = std::fs::read_to_string(path)?;
    let servers = serde_json::from_str::<Vec<McpServer>>(&raw)?;
    Ok(servers)
}

fn save_mcp_servers(path: &Path, servers: &[McpServer]) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let body = serde_json::to_string_pretty(servers)?;
    std::fs::write(path, body)?;
    Ok(())
}

struct ActivatedMcpServer {
    name: String,
    tools: Vec<McpTool>,
    _client: McpClient,
}

struct McpRuntime {
    servers: Vec<ActivatedMcpServer>,
}

impl McpRuntime {
    fn empty() -> Self {
        Self { servers: vec![] }
    }

    fn summary_lines(&self) -> Vec<String> {
        if self.servers.is_empty() {
            return vec!["MCP activation: no registered MCP servers.".to_string()];
        }

        let mut lines = vec![format!(
            "MCP activation: {} server(s) active, {} tool(s) total.",
            self.servers.len(),
            self.servers.iter().map(|s| s.tools.len()).sum::<usize>()
        )];
        for server in &self.servers {
            lines.push(format!(
                "  MCP server '{}' => {} tool(s)",
                server.name,
                server.tools.len()
            ));
            for tool in &server.tools {
                lines.push(format!("    - {}", tool.name));
            }
        }
        lines
    }
}

async fn activate_registered_mcp_servers(config_path: &Path) -> anyhow::Result<McpRuntime> {
    let registry_path = mcp_registry_path(config_path);
    let servers = load_mcp_servers(&registry_path)?;
    if servers.is_empty() {
        return Ok(McpRuntime::empty());
    }

    let mut activated = Vec::with_capacity(servers.len());
    for server in servers {
        let mut client = McpClient::connect(&server).await?;
        let tools = client.list_tools().await?;
        activated.push(ActivatedMcpServer {
            name: server.name,
            tools,
            _client: client,
        });
    }

    Ok(McpRuntime { servers: activated })
}

async fn print_gateway_status(config_path: &Path, json_output: bool) -> anyhow::Result<()> {
    let config = autonoetic_gateway::config::load_config(config_path)?;
    let agents = autonoetic_gateway::agent::scan_agents(&config.agents_dir)?;
    let registry_path = mcp_registry_path(config_path);
    let servers = load_mcp_servers(&registry_path)?;

    let mut mcp_server_rows: Vec<(
        String,
        String,
        serde_json::Value,
        Vec<McpTool>,
    )> = Vec::with_capacity(servers.len());
    for server in servers {
        let mut client = McpClient::connect(&server).await?;
        let tools = client.list_tools().await?;
        let (transport_name, transport_details) = match &server.transport {
            McpTransportConfig::Stdio => (
                "stdio".to_string(),
                serde_json::json!({
                    "type": "stdio",
                    "command": server.command,
                    "args": server.args
                }),
            ),
            McpTransportConfig::Sse { url } => (
                "sse".to_string(),
                serde_json::json!({
                    "type": "sse",
                    "url": url
                }),
            ),
        };
        mcp_server_rows.push((server.name, transport_name, transport_details, tools));
    }

    if json_output {
        let agents_json = agents
            .iter()
            .map(|agent| {
                serde_json::json!({
                    "id": agent.id,
                    "dir": agent.dir.display().to_string()
                })
            })
            .collect::<Vec<_>>();
        let mcp_servers_json = mcp_server_rows
            .iter()
            .map(|(name, _transport_name, transport_details, tools)| {
                serde_json::json!({
                    "name": name,
                    "transport": transport_details,
                    "tools_count": tools.len(),
                    "tools": tools.iter().map(|tool| serde_json::json!({
                        "name": tool.name,
                        "description": tool.description,
                        "input_schema": tool.input_schema
                    })).collect::<Vec<_>>()
                })
            })
            .collect::<Vec<_>>();

        let body = serde_json::json!({
            "gateway": {
                "config_path": config_path.display().to_string(),
                "jsonrpc_port": config.port,
                "ofp_port": config.ofp_port,
                "ofp_tls": config.tls
            },
            "agents": {
                "dir": config.agents_dir.display().to_string(),
                "count": agents.len(),
                "items": agents_json
            },
            "mcp": {
                "registry_path": registry_path.display().to_string(),
                "servers_count": mcp_server_rows.len(),
                "servers": mcp_servers_json
            }
        });
        println!("{}", serde_json::to_string_pretty(&body)?);
        return Ok(());
    }

    println!("Gateway status");
    println!(" config_path: {}", config_path.display());
    println!(" jsonrpc_port: {}", config.port);
    println!(" ofp_port: {}", config.ofp_port);
    println!(" ofp_tls: {}", config.tls);
    println!(" agents_dir: {}", config.agents_dir.display());
    println!(" agents_count: {}", agents.len());
    for agent in &agents {
        println!("  - agent: {}", agent.id);
    }

    println!(" mcp_registry_path: {}", registry_path.display());
    println!(" mcp_servers_count: {}", mcp_server_rows.len());
    for (server_name, transport_name, _transport_details, tools) in mcp_server_rows {
        println!(
            "  - mcp_server: {} (transport={}, tools={})",
            server_name,
            transport_name,
            tools.len()
        );
        for tool in tools {
            println!("      - tool: {}", tool.name);
        }
    }

    Ok(())
}

struct CliAgentExecutor {
    agents_dir: PathBuf,
    client: reqwest::Client,
}

#[async_trait::async_trait]
impl AgentExecutor for CliAgentExecutor {
    async fn call_agent(&self, agent_id: &str, message: &str) -> anyhow::Result<String> {
        let agents = autonoetic_gateway::agent::scan_agents(&self.agents_dir)?;
        let target = agents
            .into_iter()
            .find(|a| a.id == agent_id)
            .ok_or_else(|| anyhow::anyhow!("Agent '{}' not found", agent_id))?;
        let skill_path = target.dir.join("SKILL.md");
        let skill_content = std::fs::read_to_string(&skill_path)?;
        let (manifest, instructions) = SkillParser::parse(&skill_content)?;
        let llm_config = manifest
            .llm_config
            .ok_or_else(|| anyhow::anyhow!("Agent '{}' is missing llm_config", agent_id))?;

        let driver = autonoetic_gateway::llm::build_driver(llm_config.clone(), self.client.clone())?;
        let req = CompletionRequest::simple(
            llm_config.model,
            vec![Message::system(instructions), Message::user(message)],
        );
        let resp = driver.complete(&req).await?;
        if resp.text.trim().is_empty() {
            anyhow::bail!("Agent '{}' returned an empty response", agent_id);
        }
        Ok(resp.text)
    }
}

async fn run_mcp_stdio_server(agent_id: &str, config_path: &Path) -> anyhow::Result<()> {
    let config = autonoetic_gateway::config::load_config(config_path)?;
    let agents = autonoetic_gateway::agent::scan_agents(&config.agents_dir)?;
    let meta = agents
        .into_iter()
        .find(|a| a.id == agent_id)
        .ok_or_else(|| anyhow::anyhow!("Agent '{}' not found in {}", agent_id, config.agents_dir.display()))?;
    let skill_path = meta.dir.join("SKILL.md");
    let skill_content = std::fs::read_to_string(&skill_path)?;
    let (manifest, _) = SkillParser::parse(&skill_content)?;

    let mut server = AgentMcpServer::new(CliAgentExecutor {
        agents_dir: config.agents_dir,
        client: reqwest::Client::new(),
    });
    server.register_agent(ExposedAgent {
        id: manifest.agent.id,
        name: manifest.agent.name,
        description: manifest.agent.description,
    });

    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    let mut stdout = tokio::io::stdout();
    while let Some(line) = lines.next_line().await? {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let response = match serde_json::from_str::<JsonRpcRequest>(trimmed) {
            Ok(req) => server.handle(req).await,
            Err(e) => JsonRpcResponse::err(serde_json::Value::Null, -32700, format!("Parse error: {}", e)),
        };

        let encoded = serde_json::to_vec(&response)?;
        stdout.write_all(&encoded).await?;
        stdout.write_all(b"\n").await?;
        stdout.flush().await?;
    }
    Ok(())
}
