//! CCOS CLI - Unified command-line interface for CCOS
//!
//! # Usage
//!
//! ```bash
//! # Show help
//! ccos --help
//!
//! # Configuration commands
//! ccos config show
//! ccos config validate
//! ccos config init
//!
//! # Discovery commands (future)
//! ccos discover goal "send SMS to customers"
//! ccos discover server github
//! ccos discover search "issues"
//!
//! # Server management (future)
//! ccos server list
//! ccos server health
//!
//! # Approval queue (future)
//! ccos approval pending
//! ccos approval approve <id>
//! ```

use ccos::cli::commands::config::ConfigCommand;
use ccos::cli::{CliContext, OutputFormat};
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "ccos")]
#[command(author = "CCOS Team")]
#[command(version)]
#[command(about = "CCOS - Cognitive Capability Operating System", long_about = None)]
#[command(propagate_version = true)]
struct Cli {
    /// Path to configuration file
    #[arg(short, long, global = true, value_name = "FILE")]
    config: Option<PathBuf>,

    /// Output format (table, json, rtfs, plain)
    #[arg(short, long, global = true, default_value = "table")]
    output_format: String,

    /// Suppress status messages
    #[arg(short, long, global = true)]
    quiet: bool,

    /// Enable verbose output
    #[arg(short, long, global = true)]
    verbose: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Configuration management
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },

    /// Capability discovery
    Discover {
        #[command(subcommand)]
        command: DiscoverCommand,
    },

    /// Server management
    Server {
        #[command(subcommand)]
        command: ServerCommand,
    },

    /// Approval queue management
    Approval {
        #[command(subcommand)]
        command: ApprovalCommand,
    },

    /// Execute a capability
    Call {
        /// Capability ID to execute
        capability_id: String,

        /// Arguments as JSON or key=value pairs
        #[arg(trailing_var_arg = true)]
        args: Vec<String>,
    },

    /// RTFS operations
    Rtfs {
        #[command(subcommand)]
        command: RtfsCommand,
    },

    /// Interactive capability explorer (TUI)
    Explore {
        /// Start with a specific server selected
        #[arg(short, long)]
        server: Option<String>,
    },
}

#[derive(Subcommand)]
enum DiscoverCommand {
    /// Discover capabilities from a goal description
    Goal {
        /// Natural language goal description
        goal: String,
    },

    /// Discover capabilities from a specific server
    Server {
        /// Server name
        name: String,

        /// Filter hint
        #[arg(short = 'f', long)]
        filter: Option<String>,
    },

    /// Search discovered capabilities
    Search {
        /// Search query
        query: String,
    },

    /// List all discovered capabilities
    List,

    /// Inspect a specific capability
    Inspect {
        /// Capability ID
        id: String,
    },
}

#[derive(Subcommand)]
enum ServerCommand {
    /// List configured servers
    List,

    /// Add a new server
    Add {
        /// Server URL
        url: String,

        /// Server name
        #[arg(short, long)]
        name: Option<String>,
    },

    /// Remove a server
    Remove {
        /// Server name or ID
        name: String,
    },

    /// Show server health status
    Health {
        /// Specific server (all if not specified)
        name: Option<String>,
    },

    /// Dismiss a failing server
    Dismiss {
        /// Server name
        name: String,

        /// Reason for dismissal
        #[arg(short, long)]
        reason: Option<String>,
    },

    /// Retry a dismissed server
    Retry {
        /// Server name
        name: String,
    },
}

#[derive(Subcommand)]
enum ApprovalCommand {
    /// List pending approvals
    Pending,

    /// Approve a pending discovery
    Approve {
        /// Discovery ID
        id: String,

        /// Approval reason
        #[arg(short, long)]
        reason: Option<String>,
    },

    /// Reject a pending discovery
    Reject {
        /// Discovery ID
        id: String,

        /// Rejection reason
        #[arg(short, long)]
        reason: String,
    },

    /// List timed-out discoveries
    Timeout,
}

#[derive(Subcommand)]
enum RtfsCommand {
    /// Evaluate an RTFS expression
    Eval {
        /// RTFS expression to evaluate
        expr: String,
    },

    /// Run an RTFS file
    Run {
        /// Path to RTFS file
        file: PathBuf,
    },

    /// Start interactive REPL
    Repl,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    // Initialize logging
    if cli.verbose {
        std::env::set_var("RUST_LOG", "debug");
    }

    // Parse output format
    let output_format: OutputFormat = cli.output_format.parse().unwrap_or_else(|e| {
        eprintln!("Warning: {}. Using table format.", e);
        OutputFormat::Table
    });

    // Create CLI context
    let mut ctx = match cli.config {
        Some(path) => match CliContext::new(path) {
            Ok(ctx) => ctx,
            Err(e) => {
                eprintln!("Error loading configuration: {}", e);
                std::process::exit(1);
            }
        },
        None => match CliContext::with_defaults() {
            Ok(ctx) => ctx,
            Err(e) => {
                eprintln!("Error initializing context: {}", e);
                std::process::exit(1);
            }
        },
    };

    ctx.output_format = output_format;
    ctx.quiet = cli.quiet;
    ctx.verbose = cli.verbose;

    // Execute command
    let result = match cli.command {
        Commands::Config { command } => command.execute(&ctx).await,
        Commands::Discover { command } => execute_discover(&mut ctx, command).await,
        Commands::Server { command } => execute_server(&mut ctx, command).await,
        Commands::Approval { command } => execute_approval(&mut ctx, command).await,
        Commands::Call { capability_id, args } => execute_call(&mut ctx, &capability_id, args).await,
        Commands::Rtfs { command } => execute_rtfs(&mut ctx, command).await,
        Commands::Explore { server } => execute_explore(&mut ctx, server).await,
    };

    if let Err(e) = result {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}

// Placeholder implementations for future commands

async fn execute_discover(
    ctx: &mut CliContext,
    command: DiscoverCommand,
) -> rtfs::runtime::error::RuntimeResult<()> {
    let formatter = ccos::cli::OutputFormatter::new(ctx.output_format);

    match command {
        DiscoverCommand::Goal { goal } => {
            formatter.warning(&format!(
                "Goal-driven discovery not yet implemented. Goal: {}",
                goal
            ));
            formatter.list_item("See: https://github.com/mandubian/ccos/issues/170");
        }
        DiscoverCommand::Server { name, filter } => {
            ctx.status(&format!("Discovering from server: {}", name));
            if let Some(f) = filter {
                ctx.status(&format!("Filter: {}", f));
            }
            formatter.warning("Server discovery not yet implemented");
            formatter.list_item("See: https://github.com/mandubian/ccos/issues/172");
        }
        DiscoverCommand::Search { query } => {
            formatter.warning(&format!(
                "Capability search not yet implemented. Query: {}",
                query
            ));
        }
        DiscoverCommand::List => {
            formatter.warning("Capability listing not yet implemented");
        }
        DiscoverCommand::Inspect { id } => {
            formatter.warning(&format!(
                "Capability inspection not yet implemented. ID: {}",
                id
            ));
        }
    }

    Ok(())
}

async fn execute_server(
    ctx: &mut CliContext,
    command: ServerCommand,
) -> rtfs::runtime::error::RuntimeResult<()> {
    let formatter = ccos::cli::OutputFormatter::new(ctx.output_format);

    match command {
        ServerCommand::List => {
            formatter.section("MCP Servers");
            formatter.list_item("Server management is done via discovery service");
            formatter.list_item("Use 'ccos discover list' to see discovered servers");
            formatter.list_item("Use 'ccos discover server <name>' to introspect a specific server");
        }
        ServerCommand::Add { url, name } => {
            formatter.warning(&format!(
                "Server add not yet implemented. URL: {}, Name: {:?}",
                url, name
            ));
            formatter.list_item("See: https://github.com/mandubian/ccos/issues/169");
        }
        ServerCommand::Remove { name } => {
            formatter.warning(&format!("Server remove not yet implemented. Name: {}", name));
        }
        ServerCommand::Health { name } => {
            formatter.warning(&format!(
                "Server health not yet implemented. Name: {:?}",
                name
            ));
            formatter.list_item("See: https://github.com/mandubian/ccos/issues/171");
        }
        ServerCommand::Dismiss { name, reason } => {
            formatter.warning(&format!(
                "Server dismiss not yet implemented. Name: {}, Reason: {:?}",
                name, reason
            ));
        }
        ServerCommand::Retry { name } => {
            formatter.warning(&format!("Server retry not yet implemented. Name: {}", name));
        }
    }

    Ok(())
}

async fn execute_approval(
    ctx: &mut CliContext,
    command: ApprovalCommand,
) -> rtfs::runtime::error::RuntimeResult<()> {
    let formatter = ccos::cli::OutputFormatter::new(ctx.output_format);

    match command {
        ApprovalCommand::Pending => {
            formatter.warning("Approval queue not yet implemented");
            formatter.list_item("See: https://github.com/mandubian/ccos/issues/169");
        }
        ApprovalCommand::Approve { id, reason } => {
            formatter.warning(&format!(
                "Approval not yet implemented. ID: {}, Reason: {:?}",
                id, reason
            ));
        }
        ApprovalCommand::Reject { id, reason } => {
            formatter.warning(&format!(
                "Rejection not yet implemented. ID: {}, Reason: {}",
                id, reason
            ));
        }
        ApprovalCommand::Timeout => {
            formatter.warning("Timeout listing not yet implemented");
        }
    }

    Ok(())
}

async fn execute_call(
    ctx: &mut CliContext,
    capability_id: &str,
    args: Vec<String>,
) -> rtfs::runtime::error::RuntimeResult<()> {
    let formatter = ccos::cli::OutputFormatter::new(ctx.output_format);

    formatter.warning(&format!(
        "Capability call not yet implemented. ID: {}, Args: {:?}",
        capability_id, args
    ));
    formatter.list_item("See: https://github.com/mandubian/ccos/issues/172");

    Ok(())
}

async fn execute_rtfs(
    ctx: &mut CliContext,
    command: RtfsCommand,
) -> rtfs::runtime::error::RuntimeResult<()> {
    let formatter = ccos::cli::OutputFormatter::new(ctx.output_format);

    match command {
        RtfsCommand::Eval { expr } => {
            formatter.warning(&format!("RTFS eval not yet implemented. Expr: {}", expr));
        }
        RtfsCommand::Run { file } => {
            formatter.warning(&format!("RTFS run not yet implemented. File: {:?}", file));
        }
        RtfsCommand::Repl => {
            formatter.warning("RTFS REPL not yet implemented");
            formatter.list_item("Use: cargo run --bin rtfs-repl");
        }
    }

    Ok(())
}

async fn execute_explore(
    _ctx: &mut CliContext,
    server: Option<String>,
) -> rtfs::runtime::error::RuntimeResult<()> {
    // For now, suggest using the existing capability_explorer
    eprintln!("Interactive explorer launching...");
    if let Some(s) = server {
        eprintln!("Starting with server: {}", s);
    }
    eprintln!();
    eprintln!("Note: For now, use the capability_explorer example:");
    eprintln!("  cargo run --example capability_explorer -- --config ../config/agent_config.toml");
    eprintln!();
    eprintln!("See: https://github.com/mandubian/ccos/issues/172");

    Ok(())
}
