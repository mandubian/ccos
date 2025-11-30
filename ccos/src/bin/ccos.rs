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
//!
//! # Server management (future)
//! ccos server search "github"  # Search for servers
//! ccos server list              # List configured servers
//!
//! # Server management (future)
//! ccos server list
//! ccos server health
//!
//! # Approval queue (future)
//! ccos approval pending
//! ccos approval approve <id>
//! ```

use ccos::cli::commands::{
    approval::ApprovalCommand, call::CallArgs, config::ConfigCommand, discover::DiscoverCommand,
    explore::ExploreArgs, governance::GovernanceCommand, plan::PlanCommand, rtfs::RtfsCommand,
    server::ServerCommand,
};
use ccos::cli::{commands, CliContext, OutputFormat};
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
    Call(CallArgs),

    /// RTFS operations
    Rtfs {
        #[command(subcommand)]
        command: RtfsCommand,
    },

    /// Interactive capability explorer (TUI)
    Explore(ExploreArgs),

    /// Planning
    Plan {
        #[command(subcommand)]
        command: PlanCommand,
    },

    /// Governance operations
    Governance {
        #[command(subcommand)]
        command: GovernanceCommand,
    },
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
        Commands::Config { command } => commands::config::execute(&ctx, command).await,
        Commands::Discover { command } => commands::discover::execute(&mut ctx, command).await,
        Commands::Server { command } => commands::server::execute(&mut ctx, command).await,
        Commands::Approval { command } => commands::approval::execute(&mut ctx, command).await,
        Commands::Call(args) => commands::call::execute(&mut ctx, args).await,
        Commands::Rtfs { command } => commands::rtfs::execute(&mut ctx, command).await,
        Commands::Explore(args) => commands::explore::execute(&mut ctx, args).await,
        Commands::Plan { command } => commands::plan::execute(&mut ctx, command).await,
        Commands::Governance { command } => commands::governance::execute(&mut ctx, command).await,
    };

    if let Err(e) = result {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}
