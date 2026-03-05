use clap::{Parser, Subcommand, Args};
use tracing::info;

use std::path::PathBuf;

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
    Status,
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
        /// Subprocess command
        #[arg(long)]
        command: String,
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

    match &cli.command {
        // ---- Gateway ----
        Commands::Gateway(args) => match &args.command {
            GatewayCommands::Start { daemon, port, tls } => {
                let config = autonoetic_gateway::config::load_config(&config_path)?;
                let agents = autonoetic_gateway::agent::scan_agents(&config.agents_dir)?;

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

                // Start tokio event loop
                let server = autonoetic_gateway::GatewayServer::new(config);
                if let Err(e) = server.run().await {
                    tracing::error!("Gateway server error: {:?}", e);
                }
            }
            GatewayCommands::Stop => {
                info!("Stopping Gateway");
            }
            GatewayCommands::Status => {
                info!("Gateway Status");
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
            McpCommands::Add { server_name, command, args } => {
                info!("Adding MCP Server {} (cmd: {} args: {:?})", server_name, command, args);
            }
            McpCommands::Expose { agent_id } => {
                info!("Exposing Agent {} via MCP", agent_id);
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
