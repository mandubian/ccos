//! CCOS Administration CLI
//!
//! Provides administrative controls for the Cognitive Computing Operating System.
//! Primarily used for managing capability approvals and governance.

use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

use ccos::capabilities::registry::CapabilityRegistry;
use ccos::capability_marketplace::types::ApprovalStatus;
use ccos::capability_marketplace::CapabilityMarketplace;
use ccos::utils::fs::{get_workspace_root, set_workspace_root};

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Path to agent_config.toml (optional, defaults to config/agent_config.toml)
    #[arg(short, long, default_value = "config/agent_config.toml")]
    config: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// List capabilities pending approval
    ListPending,

    /// Approve a capability by ID
    Approve {
        /// The ID of the capability to approve
        id: String,
    },

    /// Reject (revoke) a capability by ID
    Reject {
        /// The ID of the capability to reject
        id: String,
    },

    /// List all capabilities and their statuses
    ListAll,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    // 1. Setup Workspace
    setup_workspace(&cli.config)?;
    let workspace_root = get_workspace_root();

    // 2. Load Config to find approvals path
    // We reuse the logic from ccos-mcp somewhat
    let agent_config = ccos::examples_common::builder::load_agent_config(&cli.config).ok();

    let approvals_dir = agent_config
        .as_ref()
        .map(|cfg| ccos::utils::fs::resolve_workspace_path(&cfg.storage.approvals_dir))
        .unwrap_or_else(|| workspace_root.join(".ccos/approvals"));
    let capabilities_dir = agent_config
        .as_ref()
        .map(|cfg| ccos::utils::fs::resolve_workspace_path(&cfg.storage.capabilities_dir))
        .unwrap_or_else(|| workspace_root.join("capabilities"));

    let approval_store_path = approvals_dir.join("capability_approvals.json");

    // 3. Initialize Minimal Marketplace
    let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
    let marketplace = CapabilityMarketplace::new(registry);

    // Load approvals
    marketplace
        .configure_approval_store(&approval_store_path)
        .await
        .map_err(|e| format!("Failed to configure approval store: {}", e))?;

    // Load capabilities from disk (RTFS files)
    // We scan standard directories
    let caps_root = capabilities_dir;
    if caps_root.exists() {
        let _ = marketplace
            .import_capabilities_from_rtfs_dir_recursive(caps_root)
            .await;
    } else {
        eprintln!(
            "Warning: 'capabilities' directory not found at {}",
            caps_root.display()
        );
    }

    // 4. Handle Commands
    match cli.command {
        Commands::ListPending => {
            let caps = marketplace.list_capabilities().await;
            let mut pending_count = 0;

            println!(
                "{:<50} | {:<15} | {}",
                "Capability ID", "Status", "Description"
            );
            println!("{:-<50}-+-{:-<15}-+-{:-<50}", "", "", "");

            for cap in caps {
                if let Some(status) = marketplace.get_effective_approval_status(&cap.id).await {
                    if matches!(status, ApprovalStatus::Pending) {
                        println!(
                            "{:<50} | {:<15} | {}",
                            cap.id,
                            format!("{:?}", status),
                            cap.description.chars().take(50).collect::<String>()
                        );
                        pending_count += 1;
                    }
                }
            }

            if pending_count == 0 {
                println!("No pending capabilities found.");
            } else {
                println!("\nFound {} pending capabilities.", pending_count);
            }
        }
        Commands::Approve { id } => {
            if marketplace.get_manifest(&id).await.is_some() {
                marketplace
                    .update_approval_status(id.clone(), ApprovalStatus::Approved)
                    .await?;
                println!("Approved capability '{}'", id);
            } else {
                eprintln!("Error: Capability '{}' not found.", id);
            }
        }
        Commands::Reject { id } => {
            if marketplace.get_manifest(&id).await.is_some() {
                marketplace
                    .update_approval_status(id.clone(), ApprovalStatus::Revoked)
                    .await?;
                println!("Revoked capability '{}'", id);
            } else {
                eprintln!("Error: Capability '{}' not found.", id);
            }
        }
        Commands::ListAll => {
            let caps = marketplace.list_capabilities().await;

            println!(
                "{:<50} | {:<15} | {}",
                "Capability ID", "Status", "Description"
            );
            println!("{:-<50}-+-{:-<15}-+-{:-<50}", "", "", "");

            for cap in &caps {
                let status = marketplace
                    .get_effective_approval_status(&cap.id)
                    .await
                    .unwrap_or(ApprovalStatus::Pending); // Should not happen for listed caps

                println!(
                    "{:<50} | {:<15} | {}",
                    cap.id,
                    format!("{:?}", status),
                    cap.description.chars().take(50).collect::<String>()
                );
            }
            println!("\nTotal capabilities: {}", caps.len());
        }
    }

    Ok(())
}

fn setup_workspace(config_path_str: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let config_path = PathBuf::from(config_path_str);
    let abs_config_path = if config_path.is_absolute() {
        config_path
    } else {
        std::env::current_dir()?.join(&config_path)
    };

    if let Some(config_parent) = abs_config_path.parent() {
        if let Some(workspace_root) = config_parent.parent() {
            set_workspace_root(workspace_root.to_path_buf());
            return Ok(workspace_root.to_path_buf());
        }
    }
    // Fallback
    let cwd = std::env::current_dir()?;
    set_workspace_root(cwd.clone());
    Ok(cwd)
}
