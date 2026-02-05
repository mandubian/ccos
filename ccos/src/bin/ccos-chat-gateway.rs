use std::path::PathBuf;

use clap::{Parser, Subcommand};

use ccos::chat::connector::{ActivationRules, LoopbackConnectorConfig};
use ccos::chat::gateway::ChatGateway;
use ccos::chat::quarantine::FileQuarantineStore;

#[derive(Parser)]
#[command(name = "ccos-chat-gateway")]
#[command(author = "CCOS Team")]
#[command(version)]
#[command(about = "CCOS Secure Chat Gateway (Phase 1 MVP)")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Serve(ServeArgs),
    Purge(PurgeArgs),
}

#[derive(Parser)]
struct ServeArgs {
    #[arg(long, default_value = "127.0.0.1:8822")]
    bind_addr: String,

    #[arg(long, default_value = "127.0.0.1:8833")]
    connector_bind_addr: String,

    #[arg(long)]
    connector_secret: String,

    #[arg(long)]
    outbound_url: Option<String>,

    #[arg(long, default_value = "storage/approvals")]
    approvals_dir: PathBuf,

    #[arg(long, default_value = "storage/quarantine")]
    quarantine_dir: PathBuf,

    #[arg(long, default_value = "86400")]
    quarantine_ttl_seconds: i64,

    #[arg(long, default_value = "CCOS_QUARANTINE_KEY")]
    quarantine_key_env: String,

    #[arg(long, default_value = "chat-mode-v0")]
    policy_pack_version: String,

    /// Comma-separated allowlist of outbound HTTP hosts for governed egress.
    ///
    /// If omitted, the gateway defaults to allowing only localhost/127.0.0.1.
    #[arg(long, value_delimiter = ',')]
    http_allow_hosts: Vec<String>,

    /// Comma-separated allowlist of outbound HTTP ports for governed egress.
    ///
    /// If omitted, all ports are allowed (subject to host allowlist).
    #[arg(long, value_delimiter = ',')]
    http_allow_ports: Vec<u16>,

    #[arg(long, value_delimiter = ',')]
    allow_senders: Vec<String>,

    #[arg(long, value_delimiter = ',')]
    allow_channels: Vec<String>,

    #[arg(long, value_delimiter = ',')]
    mentions: Vec<String>,

    #[arg(long, value_delimiter = ',')]
    keywords: Vec<String>,

    #[arg(long, default_value = "1000")]
    min_send_interval_ms: u64,
}

#[derive(Parser)]
struct PurgeArgs {
    #[arg(long, default_value = "storage/quarantine")]
    quarantine_dir: PathBuf,

    #[arg(long, default_value = "expired")]
    mode: String,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Serve(args) => serve_gateway(args).await,
        Commands::Purge(args) => purge_quarantine(args),
    };

    if let Err(e) = result {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}

async fn serve_gateway(args: ServeArgs) -> Result<(), String> {
    let activation = ActivationRules {
        allowed_senders: args.allow_senders,
        allowed_channels: args.allow_channels,
        required_mentions: args.mentions,
        required_keywords: args.keywords,
    };

    println!(
        "[Gateway] Starting with connector bind: {}",
        args.connector_bind_addr
    );
    println!("[Gateway] Outbound URL: {:?}", args.outbound_url);

    let connector = LoopbackConnectorConfig {
        bind_addr: args.connector_bind_addr,
        shared_secret: args.connector_secret,
        activation,
        outbound_url: args.outbound_url,
        default_ttl_seconds: args.quarantine_ttl_seconds,
        min_send_interval_ms: args.min_send_interval_ms,
    };

    let config = ccos::chat::gateway::ChatGatewayConfig {
        bind_addr: args.bind_addr,
        approvals_dir: args.approvals_dir,
        quarantine_dir: args.quarantine_dir,
        quarantine_default_ttl_seconds: args.quarantine_ttl_seconds,
        quarantine_key_env: args.quarantine_key_env,
        policy_pack_version: args.policy_pack_version,
        connector,
        http_allow_hosts: args.http_allow_hosts,
        http_allow_ports: args.http_allow_ports,
    };

    ChatGateway::start(config)
        .await
        .map_err(|e| format!("{}", e))
}

fn purge_quarantine(args: PurgeArgs) -> Result<(), String> {
    let removed = if args.mode == "all" {
        FileQuarantineStore::purge_all_in_dir(&args.quarantine_dir).map_err(|e| format!("{}", e))?
    } else {
        FileQuarantineStore::purge_expired_in_dir(&args.quarantine_dir)
            .map_err(|e| format!("{}", e))?
    };

    println!("Purged {} quarantine entries", removed);
    Ok(())
}
