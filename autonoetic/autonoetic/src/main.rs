mod cli;

use clap::Parser;
use cli::common::{Cli, Commands, dirs_or_default, mcp_registry_path};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let log_level = cli.log_level.as_deref().unwrap_or("info");
    tracing_subscriber::fmt()
        .with_env_filter(format!("autonoetic={log_level},{log_level}"))
        .init();

    let config_path = cli
        .config
        .map(|s| std::path::PathBuf::from(s))
        .unwrap_or_else(|| dirs_or_default().join("config.yaml"));
    std::env::set_var(
        "AUTONOETIC_MCP_REGISTRY_PATH",
        mcp_registry_path(&config_path).display().to_string(),
    );

    match &cli.command {
        Commands::Gateway(args) => match &args.command {
            cli::common::GatewayCommands::Start { daemon, port, tls } => {
                cli::gateway::handle_gateway_start(&config_path, *daemon, *port, *tls).await?;
            }
            cli::common::GatewayCommands::Stop => {
                cli::gateway::handle_gateway_stop();
            }
            cli::common::GatewayCommands::Status { json } => {
                cli::gateway::handle_gateway_status(&config_path, *json).await?;
            }
            cli::common::GatewayCommands::Approvals { command } => {
                cli::gateway::handle_gateway_approvals(&config_path, command)?;
            }
        },

        Commands::Agent(args) => match &args.command {
            cli::common::AgentCommands::Init { agent_id, template } => {
                cli::agent::init_agent_scaffold(
                    &config_path,
                    agent_id,
                    template.as_deref(),
                )?;
            }
            cli::common::AgentCommands::Run {
                agent_id,
                message,
                interactive,
                headless,
            } => {
                cli::agent::handle_agent_run(
                    &config_path,
                    agent_id,
                    message.as_deref(),
                    *interactive,
                    *headless,
                )
                .await?;
            }
            cli::common::AgentCommands::List => {
                cli::agent::handle_agent_list(&config_path).await?;
            }
            cli::common::AgentCommands::Bootstrap { from, overwrite } => {
                cli::agent::handle_agent_bootstrap(&config_path, from.as_deref(), *overwrite)?;
            }
        },

        Commands::Chat(args) => {
            cli::chat::handle_chat(&config_path, &args).await?;
        }

        Commands::Trace(args) => match &args.command {
            cli::common::TraceCommands::Sessions { agent, json } => {
                cli::trace::handle_trace_sessions(&config_path, agent.as_deref(), *json)?;
            }
            cli::common::TraceCommands::Show {
                session_id,
                agent,
                json,
            } => {
                cli::trace::handle_trace_session(
                    &config_path,
                    session_id,
                    agent.as_deref(),
                    *json,
                )?;
            }
            cli::common::TraceCommands::Event {
                log_id,
                agent,
                json,
            } => {
                cli::trace::handle_trace_event(&config_path, log_id, agent.as_deref(), *json)?;
            }
            cli::common::TraceCommands::Rebuild {
                session_id,
                agent,
                json,
                skip_checks,
            } => {
                cli::trace::handle_trace_rebuild(
                    &config_path,
                    session_id,
                    agent.as_deref(),
                    *json,
                    *skip_checks,
                )?;
            }
            cli::common::TraceCommands::Follow {
                session_id,
                agent,
                json,
            } => {
                cli::trace::handle_trace_follow(
                    &config_path,
                    session_id,
                    agent.as_deref(),
                    *json,
                ).await?;
            }
        },

        Commands::Skill(args) => match &args.command {
            cli::common::SkillCommands::Install { url_or_id, agent } => {
                tracing::info!("Installing Skill {} (agent: {:?})", url_or_id, agent);
            }
            cli::common::SkillCommands::Uninstall { skill_name, agent } => {
                tracing::info!("Uninstalling Skill {} from agent {}", skill_name, agent);
            }
        },

        Commands::Federate(args) => match &args.command {
            cli::common::FederateCommands::Join { peer_address } => {
                tracing::info!("Joining peer {}", peer_address);
            }
            cli::common::FederateCommands::List => {
                tracing::info!("Listing peers");
            }
        },

        Commands::Mcp(args) => match &args.command {
            cli::common::McpCommands::Add {
                server_name,
                command,
                sse_url,
                args,
            } => {
                cli::mcp::handle_mcp_add(
                    &config_path,
                    server_name.clone(),
                    command.clone(),
                    sse_url.clone(),
                    args.clone(),
                )
                .await?;
            }
            cli::common::McpCommands::Expose { agent_id } => {
                cli::mcp::handle_mcp_expose(agent_id, &config_path).await?;
            }
        },
    }

    Ok(())
}
