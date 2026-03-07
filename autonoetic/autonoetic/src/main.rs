use clap::{Args, Parser, Subcommand};
use tracing::info;

use autonoetic_gateway::llm::{CompletionRequest, Message};
use autonoetic_gateway::router::{
    JsonRpcRequest as GatewayJsonRpcRequest, JsonRpcResponse as GatewayJsonRpcResponse,
};
use autonoetic_gateway::runtime::parser::SkillParser;
use autonoetic_mcp::protocol::{
    JsonRpcRequest as McpJsonRpcRequest, JsonRpcResponse as McpJsonRpcResponse,
};
use autonoetic_mcp::{
    AgentExecutor as McpAgentExecutor, AgentMcpServer, ExposedAgent, McpClient, McpServer, McpTool,
    McpTransportConfig,
};
use autonoetic_types::causal_chain::CausalChainEntry;
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader as StdBufReader};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

#[derive(Parser)]
#[command(
    name = "autonoetic",
    about = "CLI for managing the Autonoetic Agent System",
    version
)]
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
    /// Chat with an agent through gateway JSON-RPC ingress
    Chat(ChatArgs),
    /// Inspect causal chain traces
    Trace(TraceArgs),
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
    /// Inspect or decide pending background approvals.
    Approvals {
        #[command(subcommand)]
        command: GatewayApprovalCommands,
    },
}

#[derive(Subcommand)]
enum GatewayApprovalCommands {
    /// List pending approval requests.
    List {
        /// Emit machine-readable JSON output.
        #[arg(long)]
        json: bool,
    },
    /// Approve one pending request.
    Approve {
        /// Approval request identifier.
        request_id: String,
        /// Optional approval note.
        #[arg(long)]
        reason: Option<String>,
    },
    /// Reject one pending request.
    Reject {
        /// Approval request identifier.
        request_id: String,
        /// Optional rejection note.
        #[arg(long)]
        reason: Option<String>,
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
// Chat
// ---------------------------------------------------------------------------

#[derive(Args)]
struct ChatArgs {
    /// Agent ID to send chat messages to.
    agent_id: String,
    /// Stable sender identity for the terminal client.
    #[arg(long)]
    sender_id: Option<String>,
    /// Stable channel identity for the terminal surface.
    #[arg(long)]
    channel_id: Option<String>,
    /// Stable conversation/session identifier.
    #[arg(long)]
    session_id: Option<String>,
    /// Suppress prompts and banners for deterministic scripted tests.
    #[arg(long)]
    test_mode: bool,
}

// ---------------------------------------------------------------------------
// Trace
// ---------------------------------------------------------------------------

#[derive(Args)]
struct TraceArgs {
    #[command(subcommand)]
    command: TraceCommands,
}

#[derive(Subcommand)]
enum TraceCommands {
    /// List known sessions across agent traces
    Sessions {
        /// Restrict lookup to one agent
        #[arg(long)]
        agent: Option<String>,
        /// Emit machine-readable JSON output
        #[arg(long)]
        json: bool,
    },
    /// Show all events for one session
    Show {
        /// Session identifier
        session_id: String,
        /// Restrict lookup to one agent
        #[arg(long)]
        agent: Option<String>,
        /// Emit machine-readable JSON output
        #[arg(long)]
        json: bool,
    },
    /// Show one specific event by log_id
    Event {
        /// Event/log identifier
        log_id: String,
        /// Restrict lookup to one agent
        #[arg(long)]
        agent: Option<String>,
        /// Emit machine-readable JSON output
        #[arg(long)]
        json: bool,
    },
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
    let config_path = cli
        .config
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
            GatewayCommands::Approvals { command } => {
                handle_gateway_approvals(&config_path, command)?;
            }
        },

        // ---- Agent ----
        Commands::Agent(args) => match &args.command {
            AgentCommands::Init { agent_id, template } => {
                info!("Initializing Agent {} (template: {:?})", agent_id, template);
                init_agent_scaffold(&config_path, agent_id, template.as_deref())?;
            }
            AgentCommands::Run {
                agent_id,
                message,
                interactive,
                headless,
            } => {
                info!(
                    "Running Agent {} (interactive: {}, headless: {})",
                    agent_id, interactive, headless
                );
                if let Some(msg) = message {
                    info!("Kickoff message: {}", msg);
                }
                run_agent_with_runtime(
                    &config_path,
                    agent_id,
                    message.as_deref(),
                    *interactive,
                    *headless,
                )
                .await?;
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

        // ---- Chat ----
        Commands::Chat(args) => {
            run_terminal_chat(&config_path, args).await?;
        }

        // ---- Trace ----
        Commands::Trace(args) => match &args.command {
            TraceCommands::Sessions { agent, json } => {
                print_trace_sessions(&config_path, agent.as_deref(), *json)?;
            }
            TraceCommands::Show {
                session_id,
                agent,
                json,
            } => {
                print_trace_session(&config_path, session_id, agent.as_deref(), *json)?;
            }
            TraceCommands::Event {
                log_id,
                agent,
                json,
            } => {
                print_trace_event(&config_path, log_id, agent.as_deref(), *json)?;
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

fn init_agent_scaffold(
    config_path: &Path,
    agent_id: &str,
    template: Option<&str>,
) -> anyhow::Result<()> {
    anyhow::ensure!(!agent_id.trim().is_empty(), "agent_id must not be empty");

    let config = autonoetic_gateway::config::load_config(config_path)?;
    std::fs::create_dir_all(&config.agents_dir)?;

    let agent_dir = config.agents_dir.join(agent_id);
    anyhow::ensure!(
        !agent_dir.exists(),
        "Agent '{}' already exists at {}",
        agent_id,
        agent_dir.display()
    );
    std::fs::create_dir_all(&agent_dir)?;
    std::fs::create_dir_all(agent_dir.join("state"))?;
    std::fs::create_dir_all(agent_dir.join("history"))?;
    std::fs::create_dir_all(agent_dir.join("skills"))?;
    std::fs::create_dir_all(agent_dir.join("scripts"))?;

    let skill_md = render_skill_template(agent_id, template);
    std::fs::write(agent_dir.join("SKILL.md"), skill_md)?;
    std::fs::write(
        agent_dir.join("runtime.lock"),
        default_runtime_lock_contents(),
    )?;

    println!(
        "Initialized agent '{}' in {}",
        agent_id,
        agent_dir.display()
    );
    Ok(())
}

fn render_skill_template(agent_id: &str, template: Option<&str>) -> String {
    let (name_suffix, description, body) = match template.unwrap_or("generic") {
        "researcher" => (
            "Researcher",
            "Research-focused autonomous agent.",
            "You are a researcher agent. Build evidence-based outputs and cite sources.",
        ),
        "coder" => (
            "Coder",
            "Software engineering autonomous agent.",
            "You are a coding agent. Produce tested, minimal, and auditable changes.",
        ),
        "auditor" => (
            "Auditor",
            "Audit and review autonomous agent.",
            "You are an auditor agent. Prioritize correctness, risks, and reproducibility.",
        ),
        _ => (
            "Agent",
            "General-purpose autonomous agent.",
            "You are an autonomous agent. Plan clearly and execute safely.",
        ),
    };
    format!(
        r#"---
name: "{agent_id}"
description: "{description}"
metadata:
  autonoetic:
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
      name: "{agent_id} {name_suffix}"
      description: "{description}"
    llm_config:
      provider: "openai"
      model: "gpt-4o"
      temperature: 0.2
---
# {agent_id}

{body}
"#
    )
}

fn default_runtime_lock_contents() -> &'static str {
    r#"gateway:
  artifact: "marketplace://gateway/autonoetic-gateway"
  version: "0.1.0"
  sha256: "replace-me"
sdk:
  version: "0.1.0"
sandbox:
  backend: "bubblewrap"
dependencies: []
artifacts: []
"#
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

async fn run_agent_with_runtime(
    config_path: &Path,
    agent_id: &str,
    kickoff_message: Option<&str>,
    interactive: bool,
    headless: bool,
) -> anyhow::Result<()> {
    let (manifest, instructions, agent_dir) = load_agent_runtime_context(config_path, agent_id)?;
    let llm_config = manifest
        .llm_config
        .clone()
        .ok_or_else(|| anyhow::anyhow!("Agent '{}' is missing llm_config", agent_id))?;
    let driver = autonoetic_gateway::llm::build_driver(llm_config, reqwest::Client::new())?;
    run_agent_with_runtime_with_driver(
        manifest,
        instructions,
        agent_dir,
        kickoff_message,
        interactive,
        headless,
        driver,
    )
    .await
}

fn load_agent_runtime_context(
    config_path: &Path,
    agent_id: &str,
) -> anyhow::Result<(autonoetic_types::agent::AgentManifest, String, PathBuf)> {
    let config = autonoetic_gateway::config::load_config(config_path)?;
    let agents = autonoetic_gateway::agent::scan_agents(&config.agents_dir)?;
    let target = agents
        .into_iter()
        .find(|a| a.id == agent_id)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Agent '{}' not found in {}",
                agent_id,
                config.agents_dir.display()
            )
        })?;

    let skill_path = target.dir.join("SKILL.md");
    let skill_content = std::fs::read_to_string(&skill_path)?;
    let (manifest, instructions) = SkillParser::parse(&skill_content)?;
    Ok((manifest, instructions, target.dir))
}

async fn run_agent_with_runtime_with_driver(
    manifest: autonoetic_types::agent::AgentManifest,
    instructions: String,
    agent_dir: PathBuf,
    kickoff_message: Option<&str>,
    interactive: bool,
    headless: bool,
    driver: Arc<dyn autonoetic_gateway::llm::LlmDriver>,
) -> anyhow::Result<()> {
    if headless {
        tracing::info!("Headless mode enabled.");
    }

    let mut runtime = autonoetic_gateway::runtime::lifecycle::AgentExecutor::new(
        manifest,
        instructions,
        driver,
        agent_dir,
        autonoetic_gateway::runtime::tools::default_registry(),
    );
    if let Some(message) = kickoff_message {
        runtime = runtime.with_initial_user_message(message.to_string());
    }
    if interactive {
        return run_interactive_session(&mut runtime, kickoff_message).await;
    }

    // Non-interactive mode should emit the assistant's final text reply to stdout.
    let mut history = vec![
        Message::system(runtime.instructions.clone()),
        Message::user(runtime.initial_user_message.clone()),
    ];
    match runtime.execute_with_history(&mut history).await {
        Ok(Some(reply)) => {
            println!("{}", reply);
            runtime.close_session("headless_complete")?;
        }
        Ok(None) => {
            println!("[No assistant text returned]");
            runtime.close_session("headless_complete_empty")?;
        }
        Err(e) => {
            let _ = runtime.close_session("headless_error");
            return Err(e);
        }
    }
    Ok(())
}

async fn run_interactive_session(
    runtime: &mut autonoetic_gateway::runtime::lifecycle::AgentExecutor,
    kickoff_message: Option<&str>,
) -> anyhow::Result<()> {
    let mut stdout = tokio::io::stdout();
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    let mut history = vec![Message::system(runtime.instructions.clone())];

    stdout
        .write_all(b"Interactive mode enabled. Type /exit to quit.\n")
        .await?;
    stdout.flush().await?;

    if let Some(message) = kickoff_message {
        history.push(Message::user(message.to_string()));
        match runtime.execute_with_history(&mut history).await {
            Ok(Some(reply)) => {
                stdout.write_all(reply.as_bytes()).await?;
                stdout.write_all(b"\n").await?;
                stdout.flush().await?;
            }
            Ok(None) => {}
            Err(e) => {
                let _ = runtime.close_session("interactive_error");
                return Err(e);
            }
        };
    }

    loop {
        stdout.write_all(b"> ").await?;
        stdout.flush().await?;

        let Some(line) = lines.next_line().await? else {
            break;
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed == "/exit" || trimmed == "/quit" {
            break;
        }

        history.push(Message::user(trimmed.to_string()));
        match runtime.execute_with_history(&mut history).await {
            Ok(Some(reply)) => {
                stdout.write_all(reply.as_bytes()).await?;
                stdout.write_all(b"\n").await?;
                stdout.flush().await?;
            }
            Ok(None) => {}
            Err(e) => {
                let _ = runtime.close_session("interactive_error");
                return Err(e);
            }
        };
    }
    runtime.close_session("interactive_exit")?;
    Ok(())
}

async fn run_terminal_chat(config_path: &Path, args: &ChatArgs) -> anyhow::Result<()> {
    let config = autonoetic_gateway::config::load_config(config_path)?;
    let session_id = args
        .session_id
        .clone()
        .unwrap_or_else(|| format!("terminal-session::{}", uuid::Uuid::new_v4()));
    let sender_id = args
        .sender_id
        .clone()
        .unwrap_or_else(default_terminal_sender_id);
    let channel_id = args
        .channel_id
        .clone()
        .unwrap_or_else(|| default_terminal_channel_id(&sender_id, &args.agent_id));
    let gateway_addr = format!("127.0.0.1:{}", config.port);
    let stream = TcpStream::connect(&gateway_addr).await.map_err(|e| {
        anyhow::anyhow!(
            "Failed to connect to gateway JSON-RPC at {}: {}",
            gateway_addr,
            e
        )
    })?;
    let (read_half, mut write_half) = stream.into_split();
    let mut gateway_lines = BufReader::new(read_half).lines();
    let mut stdin_lines = BufReader::new(tokio::io::stdin()).lines();
    let mut stdout = tokio::io::stdout();
    let envelope = terminal_channel_envelope(&channel_id, &sender_id, &session_id);
    let mut request_counter = 0_u64;

    if !args.test_mode {
        stdout
            .write_all(
                format!(
                    "Gateway terminal chat enabled for '{}' via {}. Type /exit to quit.\n",
                    args.agent_id, gateway_addr
                )
                .as_bytes(),
            )
            .await?;
        stdout.flush().await?;
    }

    loop {
        if !args.test_mode {
            stdout.write_all(b"> ").await?;
            stdout.flush().await?;
        }

        let Some(line) = stdin_lines.next_line().await? else {
            break;
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed == "/exit" || trimmed == "/quit" {
            break;
        }

        request_counter += 1;
        let request = GatewayJsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: format!("terminal-chat-{}", request_counter),
            method: "event.ingest".to_string(),
            params: serde_json::json!({
                "event_type": "chat",
                "target_agent_id": &args.agent_id,
                "message": trimmed,
                "session_id": &session_id,
                "metadata": envelope.clone(),
            }),
        };
        let encoded = serde_json::to_string(&request)?;
        write_half.write_all(encoded.as_bytes()).await?;
        write_half.write_all(b"\n").await?;
        write_half.flush().await?;

        let response_line = gateway_lines.next_line().await?.ok_or_else(|| {
            anyhow::anyhow!("Gateway JSON-RPC connection closed before a response was received")
        })?;
        let response: GatewayJsonRpcResponse = serde_json::from_str(&response_line)?;
        if let Some(error) = response.error {
            anyhow::bail!(
                "Gateway chat request failed (code {}): {}",
                error.code,
                error.message
            );
        }

        let reply = response
            .result
            .and_then(|value| {
                value
                    .get("assistant_reply")
                    .and_then(|reply| reply.as_str().map(ToOwned::to_owned))
            })
            .unwrap_or_else(|| "[No assistant text returned]".to_string());
        stdout.write_all(reply.as_bytes()).await?;
        stdout.write_all(b"\n").await?;
        stdout.flush().await?;
    }

    Ok(())
}

fn default_terminal_sender_id() -> String {
    std::env::var("USER")
        .ok()
        .or_else(|| std::env::var("USERNAME").ok())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "terminal-user".to_string())
}

fn default_terminal_channel_id(sender_id: &str, agent_id: &str) -> String {
    format!("terminal:{}:{}", sender_id, agent_id)
}

fn terminal_channel_envelope(
    channel_id: &str,
    sender_id: &str,
    session_id: &str,
) -> serde_json::Value {
    serde_json::json!({
        "channel": {
            "kind": "terminal",
            "channel_id": channel_id,
            "sender_id": sender_id,
            "session_id": session_id
        }
    })
}

async fn print_gateway_status(config_path: &Path, json_output: bool) -> anyhow::Result<()> {
    let config = autonoetic_gateway::config::load_config(config_path)?;
    let agents = autonoetic_gateway::agent::scan_agents(&config.agents_dir)?;
    let registry_path = mcp_registry_path(config_path);
    let servers = load_mcp_servers(&registry_path)?;

    let mut mcp_server_rows: Vec<(String, String, serde_json::Value, Vec<McpTool>)> =
        Vec::with_capacity(servers.len());
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
                "ofp_tls": config.tls,
                "background_scheduler_enabled": config.background_scheduler_enabled,
                "background_tick_secs": config.background_tick_secs,
                "background_min_interval_secs": config.background_min_interval_secs,
                "max_background_due_per_tick": config.max_background_due_per_tick
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
    println!(
        " background_scheduler: enabled={}, tick_secs={}, min_interval_secs={}, max_due_per_tick={}",
        config.background_scheduler_enabled,
        config.background_tick_secs,
        config.background_min_interval_secs,
        config.max_background_due_per_tick
    );
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

fn handle_gateway_approvals(
    config_path: &Path,
    command: &GatewayApprovalCommands,
) -> anyhow::Result<()> {
    let config = autonoetic_gateway::config::load_config(config_path)?;
    match command {
        GatewayApprovalCommands::List { json } => {
            let approvals = autonoetic_gateway::scheduler::load_approval_requests(&config)?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&approvals)?);
                return Ok(());
            }
            if approvals.is_empty() {
                println!("No pending background approval requests.");
                return Ok(());
            }
            println!(
                "{:<38} {:<20} {:<24} ACTION",
                "REQUEST ID", "AGENT", "CREATED AT"
            );
            for approval in approvals {
                println!(
                    "{:<38} {:<20} {:<24} {}",
                    approval.request_id,
                    approval.agent_id,
                    approval.created_at,
                    approval.action.kind()
                );
            }
        }
        GatewayApprovalCommands::Approve { request_id, reason } => {
            let decision = autonoetic_gateway::scheduler::approve_request(
                &config,
                request_id,
                "cli",
                reason.clone(),
            )?;
            println!(
                "Approved {} for agent {} ({})",
                decision.request_id,
                decision.agent_id,
                decision.action.kind()
            );
        }
        GatewayApprovalCommands::Reject { request_id, reason } => {
            let decision = autonoetic_gateway::scheduler::reject_request(
                &config,
                request_id,
                "cli",
                reason.clone(),
            )?;
            println!(
                "Rejected {} for agent {} ({})",
                decision.request_id,
                decision.agent_id,
                decision.action.kind()
            );
        }
    }
    Ok(())
}

#[derive(Debug)]
struct AgentTrace {
    agent_id: String,
    entries: Vec<CausalChainEntry>,
}

#[derive(Debug)]
struct SessionSummary {
    agent_id: String,
    session_id: String,
    first_timestamp: String,
    last_timestamp: String,
    event_count: usize,
    max_event_seq: u64,
}

fn print_trace_sessions(
    config_path: &Path,
    requested_agent: Option<&str>,
    json_output: bool,
) -> anyhow::Result<()> {
    let traces = load_agent_traces(config_path, requested_agent)?;
    let sessions = collect_session_summaries(&traces);
    if json_output {
        let body = sessions
            .iter()
            .map(|s| {
                serde_json::json!({
                    "agent_id": s.agent_id,
                    "session_id": s.session_id,
                    "first_timestamp": s.first_timestamp,
                    "last_timestamp": s.last_timestamp,
                    "event_count": s.event_count,
                    "max_event_seq": s.max_event_seq
                })
            })
            .collect::<Vec<_>>();
        println!("{}", serde_json::to_string_pretty(&body)?);
        return Ok(());
    }

    if sessions.is_empty() {
        println!("No trace sessions found.");
        return Ok(());
    }

    println!(
        "{:<30} {:<38} {:<26} {:<26} {:<8} {:<10}",
        "AGENT", "SESSION ID", "FIRST TS", "LAST TS", "EVENTS", "MAX SEQ"
    );
    for s in sessions {
        println!(
            "{:<30} {:<38} {:<26} {:<26} {:<8} {:<10}",
            s.agent_id,
            s.session_id,
            s.first_timestamp,
            s.last_timestamp,
            s.event_count,
            s.max_event_seq
        );
    }
    Ok(())
}

fn print_trace_session(
    config_path: &Path,
    session_id: &str,
    requested_agent: Option<&str>,
    json_output: bool,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        !session_id.trim().is_empty(),
        "session_id must not be empty"
    );
    let traces = load_agent_traces(config_path, requested_agent)?;
    let mut matches: Vec<(String, Vec<CausalChainEntry>)> = Vec::new();
    for trace in traces {
        let events = trace
            .entries
            .into_iter()
            .filter(|entry| entry.session_id == session_id)
            .collect::<Vec<_>>();
        if !events.is_empty() {
            matches.push((trace.agent_id, events));
        }
    }

    anyhow::ensure!(
        !matches.is_empty(),
        "No events found for session '{}'{}",
        session_id,
        requested_agent
            .map(|a| format!(" under agent '{}'", a))
            .unwrap_or_default()
    );
    if requested_agent.is_none() && matches.len() > 1 {
        let owners = matches
            .iter()
            .map(|(agent_id, _)| agent_id.clone())
            .collect::<Vec<_>>()
            .join(", ");
        anyhow::bail!(
            "Session '{}' found in multiple agents ({}). Re-run with --agent.",
            session_id,
            owners
        );
    }

    let (agent_id, mut entries) = matches
        .into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("failed to resolve session match"))?;
    entries.sort_by_key(|e| e.event_seq);

    if json_output {
        let body = serde_json::json!({
            "agent_id": agent_id,
            "session_id": session_id,
            "events": entries,
        });
        println!("{}", serde_json::to_string_pretty(&body)?);
        return Ok(());
    }

    println!("Agent: {}", agent_id);
    println!("Session: {}", session_id);
    println!(
        "{:<10} {:<28} {:<10} {:<24} {}",
        "EVENT_SEQ", "TIMESTAMP", "STATUS", "CATEGORY.ACTION", "LOG_ID"
    );
    for entry in entries {
        println!(
            "{:<10} {:<28} {:<10} {:<24} {}",
            entry.event_seq,
            entry.timestamp,
            format!("{:?}", entry.status),
            format!("{}.{}", entry.category, entry.action),
            entry.log_id
        );
    }
    Ok(())
}

fn print_trace_event(
    config_path: &Path,
    log_id: &str,
    requested_agent: Option<&str>,
    json_output: bool,
) -> anyhow::Result<()> {
    anyhow::ensure!(!log_id.trim().is_empty(), "log_id must not be empty");
    let traces = load_agent_traces(config_path, requested_agent)?;
    let mut matches: Vec<(String, CausalChainEntry)> = Vec::new();
    for trace in traces {
        for entry in trace.entries {
            if entry.log_id == log_id {
                matches.push((trace.agent_id.clone(), entry));
            }
        }
    }

    anyhow::ensure!(
        !matches.is_empty(),
        "No event found for log_id '{}'{}",
        log_id,
        requested_agent
            .map(|a| format!(" under agent '{}'", a))
            .unwrap_or_default()
    );
    if requested_agent.is_none() && matches.len() > 1 {
        let owners = matches
            .iter()
            .map(|(agent_id, _)| agent_id.clone())
            .collect::<Vec<_>>()
            .join(", ");
        anyhow::bail!(
            "Event '{}' found in multiple agents ({}). Re-run with --agent.",
            log_id,
            owners
        );
    }

    let (agent_id, entry) = matches
        .into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("failed to resolve event match"))?;

    if json_output {
        let body = serde_json::json!({
            "agent_id": agent_id,
            "event": entry,
        });
        println!("{}", serde_json::to_string_pretty(&body)?);
        return Ok(());
    }

    println!("Agent: {}", agent_id);
    println!("{}", serde_json::to_string_pretty(&entry)?);
    Ok(())
}

fn load_agent_traces(
    config_path: &Path,
    requested_agent: Option<&str>,
) -> anyhow::Result<Vec<AgentTrace>> {
    let config = autonoetic_gateway::config::load_config(config_path)?;
    let mut agents = autonoetic_gateway::agent::scan_agents(&config.agents_dir)?;
    if let Some(agent_id) = requested_agent {
        agents.retain(|a| a.id == agent_id);
        anyhow::ensure!(
            !agents.is_empty(),
            "Agent '{}' not found in {}",
            agent_id,
            config.agents_dir.display()
        );
    }

    let mut traces = Vec::new();
    for agent in agents {
        let path = agent.dir.join("history").join("causal_chain.jsonl");
        if !path.exists() {
            continue;
        }
        let entries = read_trace_entries(&path)?;
        traces.push(AgentTrace {
            agent_id: agent.id,
            entries,
        });
    }
    Ok(traces)
}

fn read_trace_entries(path: &Path) -> anyhow::Result<Vec<CausalChainEntry>> {
    let file = std::fs::File::open(path)?;
    let reader = StdBufReader::new(file);
    let mut entries = Vec::new();
    for (idx, line) in reader.lines().enumerate() {
        let line = line?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let entry: CausalChainEntry = serde_json::from_str(trimmed).map_err(|e| {
            anyhow::anyhow!(
                "Invalid JSON in {} at line {}: {}",
                path.display(),
                idx + 1,
                e
            )
        })?;
        validate_trace_entry(&entry, path, idx + 1)?;
        entries.push(entry);
    }
    Ok(entries)
}

fn validate_trace_entry(
    entry: &CausalChainEntry,
    path: &Path,
    line_no: usize,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        !entry.session_id.trim().is_empty(),
        "Invalid causal entry in {} at line {}: missing top-level session_id",
        path.display(),
        line_no
    );
    anyhow::ensure!(
        !entry.entry_hash.trim().is_empty(),
        "Invalid causal entry in {} at line {}: missing top-level entry_hash",
        path.display(),
        line_no
    );
    Ok(())
}

fn collect_session_summaries(traces: &[AgentTrace]) -> Vec<SessionSummary> {
    let mut by_session: BTreeMap<(String, String), SessionSummary> = BTreeMap::new();
    for trace in traces {
        for entry in &trace.entries {
            let key = (trace.agent_id.clone(), entry.session_id.clone());
            let summary = by_session.entry(key).or_insert_with(|| SessionSummary {
                agent_id: trace.agent_id.clone(),
                session_id: entry.session_id.clone(),
                first_timestamp: entry.timestamp.clone(),
                last_timestamp: entry.timestamp.clone(),
                event_count: 0,
                max_event_seq: entry.event_seq,
            });
            summary.event_count += 1;
            if entry.timestamp < summary.first_timestamp {
                summary.first_timestamp = entry.timestamp.clone();
            }
            if entry.timestamp > summary.last_timestamp {
                summary.last_timestamp = entry.timestamp.clone();
            }
            if entry.event_seq > summary.max_event_seq {
                summary.max_event_seq = entry.event_seq;
            }
        }
    }

    by_session.into_values().collect::<Vec<_>>()
}

struct CliAgentExecutor {
    agents_dir: PathBuf,
    client: reqwest::Client,
}

#[async_trait::async_trait]
impl McpAgentExecutor for CliAgentExecutor {
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

        let driver =
            autonoetic_gateway::llm::build_driver(llm_config.clone(), self.client.clone())?;
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
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Agent '{}' not found in {}",
                agent_id,
                config.agents_dir.display()
            )
        })?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use autonoetic_gateway::llm::{
        CompletionRequest, CompletionResponse, LlmDriver, StopReason, TokenUsage, ToolCall,
    };
    use tempfile::tempdir;

    struct DenySandboxExecDriver;

    #[async_trait::async_trait]
    impl LlmDriver for DenySandboxExecDriver {
        async fn complete(
            &self,
            request: &CompletionRequest,
        ) -> anyhow::Result<CompletionResponse> {
            if !request.tools.iter().any(|t| t.name == "sandbox.exec") {
                anyhow::bail!("sandbox.exec not exposed to model");
            }
            Ok(CompletionResponse {
                text: String::new(),
                tool_calls: vec![ToolCall {
                    id: "call_1".to_string(),
                    name: "sandbox.exec".to_string(),
                    arguments: serde_json::json!({
                        "command": "echo blocked"
                    })
                    .to_string(),
                }],
                stop_reason: StopReason::ToolUse,
                usage: TokenUsage::default(),
            })
        }
    }

    #[tokio::test]
    async fn test_agent_run_path_enforces_sandbox_shell_policy() {
        let temp = tempdir().expect("tempdir should create");
        let agents_dir = temp.path().join("agents");
        let agent_dir = agents_dir.join("agent_demo");
        std::fs::create_dir_all(&agent_dir).expect("agent dir should create");

        let skill = r#"---
version: "1.0"
runtime:
  engine: "autonoetic"
  gateway_version: "0.1.0"
  sdk_version: "0.1.0"
  type: "stateful"
  sandbox: "bubblewrap"
  runtime_lock: "runtime.lock"
agent:
  id: "agent_demo"
  name: "Agent Demo"
  description: "Demo agent"
capabilities:
  - type: "ShellExec"
    patterns:
      - "python3 scripts/*"
---
# Agent Demo
Use tools when needed.
"#;
        std::fs::write(agent_dir.join("SKILL.md"), skill).expect("skill should write");

        let config_path = temp.path().join("config.yaml");
        let config_yaml = format!(
            "agents_dir: \"{}\"\nport: 4000\nofp_port: 4200\ntls: false\n",
            agents_dir.display()
        );
        std::fs::write(&config_path, config_yaml).expect("config should write");

        let (manifest, instructions, loaded_agent_dir) =
            load_agent_runtime_context(&config_path, "agent_demo").expect("context should load");
        let err = run_agent_with_runtime_with_driver(
            manifest,
            instructions,
            loaded_agent_dir,
            Some("start"),
            false,
            true,
            Arc::new(DenySandboxExecDriver),
        )
        .await
        .expect_err("policy denial should fail runtime");

        assert!(
            err.to_string()
                .contains("sandbox command denied by ShellExec policy"),
            "error should indicate shell policy denial"
        );
    }

    #[test]
    fn test_init_agent_scaffold_creates_skill_and_runtime_lock() {
        let temp = tempdir().expect("tempdir should create");
        let config_path = temp.path().join("config.yaml");
        let agents_dir = temp.path().join("agents");
        let config_yaml = format!(
            "agents_dir: \"{}\"\nport: 4000\nofp_port: 4200\ntls: false\n",
            agents_dir.display()
        );
        std::fs::write(&config_path, config_yaml).expect("config should write");

        init_agent_scaffold(&config_path, "agent_bootstrap", Some("coder"))
            .expect("scaffold should succeed");

        let agent_dir = agents_dir.join("agent_bootstrap");
        let skill =
            std::fs::read_to_string(agent_dir.join("SKILL.md")).expect("SKILL.md should exist");
        let lock = std::fs::read_to_string(agent_dir.join("runtime.lock"))
            .expect("runtime.lock should exist");

        assert!(skill.contains("id: \"agent_bootstrap\""));
        assert!(skill.contains("description: \"Software engineering autonomous agent.\""));
        assert!(lock.contains("dependencies: []"));
    }

    #[test]
    fn test_read_trace_entries_rejects_missing_top_level_session_fields() {
        let temp = tempdir().expect("tempdir should create");
        let path = temp.path().join("causal_chain.jsonl");
        std::fs::write(
            &path,
            r#"{"timestamp":"2026-03-06T00:00:00Z","log_id":"l1","actor_id":"a1","category":"lifecycle","action":"wake","target":null,"status":"SUCCESS","reason":null,"payload":{"session_id":"legacy"},"prev_hash":"genesis","entry_hash":"abc"}"#,
        )
        .expect("trace should write");

        let err = read_trace_entries(&path).expect_err("legacy missing session_id should fail");
        assert!(
            err.to_string().contains("missing top-level session_id"),
            "expected strict top-level session_id validation"
        );
    }

    #[test]
    fn test_read_trace_entries_rejects_missing_top_level_hash_fields() {
        let temp = tempdir().expect("tempdir should create");
        let path = temp.path().join("causal_chain.jsonl");
        std::fs::write(
            &path,
            r#"{"timestamp":"2026-03-06T00:00:00Z","log_id":"l1","actor_id":"a1","session_id":"s1","turn_id":"turn-000001","event_seq":1,"category":"lifecycle","action":"wake","target":null,"status":"SUCCESS","reason":null,"payload":{"history_messages":2},"prev_hash":"genesis","entry_hash":""}"#,
        )
        .expect("trace should write");

        let err = read_trace_entries(&path).expect_err("missing entry_hash should fail");
        assert!(
            err.to_string().contains("missing top-level entry_hash"),
            "expected strict top-level entry_hash validation"
        );
    }

    #[test]
    fn test_collect_session_summaries_groups_sessions() {
        let traces = vec![AgentTrace {
            agent_id: "agent_demo".to_string(),
            entries: vec![
                CausalChainEntry {
                    timestamp: "2026-03-06T10:00:00Z".to_string(),
                    log_id: "l1".to_string(),
                    actor_id: "agent_demo".to_string(),
                    session_id: "s1".to_string(),
                    turn_id: Some("turn-000001".to_string()),
                    event_seq: 1,
                    category: "session".to_string(),
                    action: "start".to_string(),
                    target: None,
                    status: autonoetic_types::causal_chain::EntryStatus::Success,
                    reason: None,
                    payload: None,
                    payload_hash: None,
                    prev_hash: "genesis".to_string(),
                    entry_hash: "h1".to_string(),
                },
                CausalChainEntry {
                    timestamp: "2026-03-06T10:00:02Z".to_string(),
                    log_id: "l2".to_string(),
                    actor_id: "agent_demo".to_string(),
                    session_id: "s1".to_string(),
                    turn_id: Some("turn-000001".to_string()),
                    event_seq: 2,
                    category: "lifecycle".to_string(),
                    action: "wake".to_string(),
                    target: None,
                    status: autonoetic_types::causal_chain::EntryStatus::Success,
                    reason: None,
                    payload: None,
                    payload_hash: None,
                    prev_hash: "h1".to_string(),
                    entry_hash: "h2".to_string(),
                },
                CausalChainEntry {
                    timestamp: "2026-03-06T10:05:00Z".to_string(),
                    log_id: "l3".to_string(),
                    actor_id: "agent_demo".to_string(),
                    session_id: "s2".to_string(),
                    turn_id: Some("turn-000001".to_string()),
                    event_seq: 1,
                    category: "session".to_string(),
                    action: "start".to_string(),
                    target: None,
                    status: autonoetic_types::causal_chain::EntryStatus::Success,
                    reason: None,
                    payload: None,
                    payload_hash: None,
                    prev_hash: "genesis".to_string(),
                    entry_hash: "h3".to_string(),
                },
            ],
        }];

        let sessions = collect_session_summaries(&traces);
        assert_eq!(sessions.len(), 2, "expected one summary per session");
        let s1 = sessions
            .iter()
            .find(|s| s.session_id == "s1")
            .expect("s1 should be present");
        assert_eq!(s1.event_count, 2);
        assert_eq!(s1.max_event_seq, 2);
    }
}
