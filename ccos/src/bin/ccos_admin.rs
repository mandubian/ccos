//! CCOS Administration CLI
//!
//! Provides administrative controls for the Cognitive Computing Operating System.
//! Manages both capability approvals and general approval queue.

use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

use ccos::approval::queue::ApprovalAuthority;
use ccos::approval::storage_file::FileApprovalStorage;
use ccos::approval::types::{ApprovalFilter, ApprovalStatus as GeneralApprovalStatus};
use ccos::approval::unified_queue::UnifiedApprovalQueue;
use ccos::capabilities::registry::CapabilityRegistry;
use ccos::capability_marketplace::types::ApprovalStatus as CapabilityApprovalStatus;
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
    /// Manage capability marketplace approvals
    #[command(subcommand)]
    Capability(CapabilityCommands),

    /// Manage general approval queue (HumanActionRequest, ServerDiscovery, etc.)
    #[command(subcommand)]
    Approval(ApprovalCommands),

    /// Clean up pending approvals by criteria
    ///
    /// WARNING: This only deletes PENDING approvals. Approved, rejected, or expired
    /// approvals are never deleted by this command. Use `approval delete <ID>` to
    /// remove specific approvals regardless of status.
    Clean {
        /// Type of pending approval to clean
        ///
        /// Available types:
        /// - all                          : All pending approvals (default)
        /// - HumanActionRequest           : Human intervention requests
        /// - ServerDiscovery              : MCP server discovery requests
        /// - SecretWrite                  : Secret storage requests
        /// - SecretRequired               : Capability secret requirements
        /// - BudgetExtension              : Budget extension requests
        /// - EffectApproval               : Effect-based capability execution
        /// - SynthesisApproval            : Synthesized capability approval
        /// - LlmPromptApproval            : LLM prompt approval
        /// - ChatPolicyException          : Chat policy exceptions
        /// - ChatPublicDeclassification   : Public declassification approval
        #[arg(short, long, default_value = "all", value_name = "TYPE")]
        approval_type: String,

        /// Clean approvals older than N hours
        #[arg(short, long, value_name = "HOURS")]
        older_than_hours: Option<i64>,

        /// Dry run - show what would be deleted without actually deleting
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Subcommand)]
enum CapabilityCommands {
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

#[derive(Subcommand)]
enum ApprovalCommands {
    /// List pending approvals with optional type filter
    ListPending {
        /// Filter by approval type
        ///
        /// Available types:
        /// - HumanActionRequest           : Human intervention requests
        /// - ServerDiscovery              : MCP server discovery requests
        /// - SecretWrite                  : Secret storage requests
        /// - SecretRequired               : Capability secret requirements
        /// - BudgetExtension              : Budget extension requests
        /// - EffectApproval               : Effect-based capability execution
        /// - SynthesisApproval            : Synthesized capability approval
        /// - LlmPromptApproval            : LLM prompt approval
        /// - ChatPolicyException          : Chat policy exceptions
        /// - ChatPublicDeclassification   : Public declassification approval
        #[arg(short, long, value_name = "TYPE")]
        approval_type: Option<String>,

        /// Show full details for each approval
        #[arg(long)]
        verbose: bool,
    },

    /// List all approvals (pending, approved, rejected)
    ListAll {
        /// Filter by status
        #[arg(short, long, value_enum)]
        status: Option<ApprovalStatusFilter>,

        /// Filter by approval type
        ///
        /// Available types:
        /// - HumanActionRequest           : Human intervention requests
        /// - ServerDiscovery              : MCP server discovery requests
        /// - SecretWrite                  : Secret storage requests
        /// - SecretRequired               : Capability secret requirements
        /// - BudgetExtension              : Budget extension requests
        /// - EffectApproval               : Effect-based capability execution
        /// - SynthesisApproval            : Synthesized capability approval
        /// - LlmPromptApproval            : LLM prompt approval
        /// - ChatPolicyException          : Chat policy exceptions
        /// - ChatPublicDeclassification   : Public declassification approval
        #[arg(short, long, value_name = "TYPE")]
        approval_type: Option<String>,
    },

    /// Approve a request by ID
    Approve {
        /// The ID of the approval to approve
        id: String,

        /// Optional reason for approval
        #[arg(short, long)]
        reason: Option<String>,
    },

    /// Reject a request by ID
    Reject {
        /// The ID of the approval to reject
        id: String,

        /// Reason for rejection (required)
        #[arg(short, long)]
        reason: String,
    },

    /// Show detailed information about a specific approval
    Show {
        /// The ID of the approval to show
        id: String,
    },

    /// Delete an approval by ID
    Delete {
        /// The ID of the approval to delete
        id: String,

        /// Skip confirmation prompt
        #[arg(short, long)]
        force: bool,
    },
}

#[derive(ValueEnum, Clone, Debug)]
enum ApprovalStatusFilter {
    Pending,
    Approved,
    Rejected,
    Expired,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    // 1. Setup Workspace
    setup_workspace(&cli.config)?;
    let workspace_root = get_workspace_root();

    // 2. Load Config to find paths
    let agent_config = ccos::examples_common::builder::load_agent_config(&cli.config).ok();

    let approvals_dir = agent_config
        .as_ref()
        .map(|cfg| ccos::utils::fs::resolve_workspace_path(&cfg.storage.approvals_dir))
        .unwrap_or_else(|| workspace_root.join("storage/approvals"));

    let capabilities_dir = agent_config
        .as_ref()
        .map(|cfg| ccos::utils::fs::resolve_workspace_path(&cfg.storage.capabilities_dir))
        .unwrap_or_else(|| workspace_root.join("capabilities"));

    let approval_store_path = approvals_dir.join("capability_approvals.json");

    // 3. Initialize Capability Marketplace
    let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
    let marketplace = CapabilityMarketplace::new(registry);

    marketplace
        .configure_approval_store(&approval_store_path)
        .await
        .map_err(|e| format!("Failed to configure approval store: {}", e))?;

    let caps_root = capabilities_dir;
    if caps_root.exists() {
        let _ = marketplace
            .import_capabilities_from_rtfs_dir_recursive(caps_root)
            .await;
    }

    // 4. Initialize General Approval Queue
    let storage = FileApprovalStorage::new(approvals_dir.clone())
        .map_err(|e| format!("Failed to initialize approval storage: {}", e))?;
    let approval_queue = UnifiedApprovalQueue::new(Arc::new(storage));

    // 5. Handle Commands
    match cli.command {
        Commands::Capability(cmd) => {
            handle_capability_commands(cmd, marketplace).await?;
        }
        Commands::Approval(cmd) => {
            handle_approval_commands(cmd, approval_queue).await?;
        }
        Commands::Clean {
            approval_type,
            older_than_hours,
            dry_run,
        } => {
            handle_clean_command(approval_queue, approval_type, older_than_hours, dry_run).await?;
        }
    }

    Ok(())
}

async fn handle_capability_commands(
    cmd: CapabilityCommands,
    marketplace: CapabilityMarketplace,
) -> Result<(), Box<dyn std::error::Error>> {
    match cmd {
        CapabilityCommands::ListPending => {
            let caps = marketplace.list_capabilities().await;
            let mut pending_count = 0;

            println!(
                "{:<50} | {:<15} | {}",
                "Capability ID", "Status", "Description"
            );
            println!("{:-<50}-+-{:-<15}-+-{:-<50}", "", "", "");

            for cap in caps {
                if let Some(status) = marketplace.get_effective_approval_status(&cap.id).await {
                    if matches!(status, CapabilityApprovalStatus::Pending) {
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
        CapabilityCommands::Approve { id } => {
            if marketplace.get_manifest(&id).await.is_some() {
                marketplace
                    .update_approval_status(id.clone(), CapabilityApprovalStatus::Approved)
                    .await?;
                println!("✅ Approved capability '{}'", id);
            } else {
                eprintln!("❌ Error: Capability '{}' not found.", id);
            }
        }
        CapabilityCommands::Reject { id } => {
            if marketplace.get_manifest(&id).await.is_some() {
                marketplace
                    .update_approval_status(id.clone(), CapabilityApprovalStatus::Revoked)
                    .await?;
                println!("✅ Revoked capability '{}'", id);
            } else {
                eprintln!("❌ Error: Capability '{}' not found.", id);
            }
        }
        CapabilityCommands::ListAll => {
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
                    .unwrap_or(CapabilityApprovalStatus::Pending);

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

async fn handle_approval_commands<S: ApprovalStorage>(
    cmd: ApprovalCommands,
    queue: UnifiedApprovalQueue<S>,
) -> Result<(), Box<dyn std::error::Error>> {
    match cmd {
        ApprovalCommands::ListPending {
            approval_type,
            verbose,
        } => {
            let filter = if let Some(cat) = approval_type {
                ApprovalFilter {
                    category_type: Some(cat),
                    status_pending: Some(true),
                    limit: None,
                }
            } else {
                ApprovalFilter::pending()
            };

            let approvals = queue.list(filter).await?;

            if approvals.is_empty() {
                println!("No pending approvals found.");
            } else {
                println!("\n📋 Pending Approvals ({} found):\n", approvals.len());
                for app in &approvals {
                    print_approval_summary(app);
                    if verbose {
                        print_approval_details(app);
                        println!();
                    }
                }
            }
        }
        ApprovalCommands::ListAll {
            status,
            approval_type,
        } => {
            let filter = ApprovalFilter {
                category_type: approval_type,
                status_pending: match status {
                    Some(ApprovalStatusFilter::Pending) => Some(true),
                    _ => None,
                },
                limit: None,
            };

            let approvals = queue.list(filter).await?;

            // Filter by status if specified
            let filtered: Vec<_> = if let Some(s) = status {
                let target_status = match s {
                    ApprovalStatusFilter::Approved => GeneralApprovalStatus::Approved {
                        by: ApprovalAuthority::Auto,
                        reason: None,
                        at: chrono::Utc::now(),
                    },
                    ApprovalStatusFilter::Rejected => GeneralApprovalStatus::Rejected {
                        by: ApprovalAuthority::Auto,
                        reason: "filter".to_string(),
                        at: chrono::Utc::now(),
                    },
                    ApprovalStatusFilter::Expired => GeneralApprovalStatus::Expired {
                        at: chrono::Utc::now(),
                    },
                    ApprovalStatusFilter::Pending => GeneralApprovalStatus::Pending,
                };
                approvals
                    .into_iter()
                    .filter(|a| {
                        std::mem::discriminant(&a.status) == std::mem::discriminant(&target_status)
                    })
                    .collect()
            } else {
                approvals
            };

            if filtered.is_empty() {
                println!("No approvals found matching criteria.");
            } else {
                println!("\n📋 Approvals ({} found):\n", filtered.len());
                for app in &filtered {
                    print_approval_summary(app);
                }
            }
        }
        ApprovalCommands::Approve { id, reason } => {
            match queue
                .approve(
                    &id,
                    ApprovalAuthority::User("admin".to_string()),
                    reason.or_else(|| Some("Approved via ccos_admin CLI".to_string())),
                )
                .await
            {
                Ok(_) => println!("✅ Approved approval '{}'", id),
                Err(e) => eprintln!("❌ Failed to approve '{}': {}", id, e),
            }
        }
        ApprovalCommands::Reject { id, reason } => {
            match queue
                .reject(&id, ApprovalAuthority::User("admin".to_string()), reason)
                .await
            {
                Ok(_) => println!("✅ Rejected approval '{}'", id),
                Err(e) => eprintln!("❌ Failed to reject '{}': {}", id, e),
            }
        }
        ApprovalCommands::Show { id } => match queue.get(&id).await? {
            Some(app) => {
                println!("\n📄 Approval Details:\n");
                print_approval_summary(&app);
                print_approval_details(&app);
            }
            None => eprintln!("❌ Approval '{}' not found.", id),
        },
        ApprovalCommands::Delete { id, force } => {
            if !force {
                print!(
                    "⚠️  Are you sure you want to delete approval '{}'? [y/N]: ",
                    id
                );
                use std::io::{self, Write};
                io::stdout().flush()?;
                let mut input = String::new();
                io::stdin().read_line(&mut input)?;
                if !input.trim().eq_ignore_ascii_case("y") {
                    println!("Cancelled.");
                    return Ok(());
                }
            }

            match queue.remove(&id).await {
                Ok(true) => println!("✅ Deleted approval '{}'", id),
                Ok(false) => eprintln!("❌ Approval '{}' not found.", id),
                Err(e) => eprintln!("❌ Failed to delete '{}': {}", id, e),
            }
        }
    }
    Ok(())
}

async fn handle_clean_command<S: ApprovalStorage>(
    queue: UnifiedApprovalQueue<S>,
    approval_type: String,
    older_than_hours: Option<i64>,
    dry_run: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let filter = if approval_type == "all" {
        ApprovalFilter::pending()
    } else {
        ApprovalFilter {
            category_type: Some(approval_type.clone()),
            status_pending: Some(true),
            limit: None,
        }
    };

    let approvals = queue.list(filter).await?;
    let now = chrono::Utc::now();

    let to_delete: Vec<_> = approvals
        .into_iter()
        .filter(|app| {
            if let Some(hours) = older_than_hours {
                let age = now.signed_duration_since(app.requested_at);
                age.num_hours() >= hours
            } else {
                true
            }
        })
        .collect();

    if to_delete.is_empty() {
        println!("No approvals match the cleanup criteria.");
        return Ok(());
    }

    println!(
        "Found {} approvals to {}clean:\n",
        to_delete.len(),
        if dry_run { "[DRY RUN] " } else { "" }
    );

    for app in &to_delete {
        let age_hours = now.signed_duration_since(app.requested_at).num_hours();
        println!(
            "  - {} (type: {}, age: {}h)",
            app.id,
            format_category(&app.category),
            age_hours
        );
    }

    if dry_run {
        println!("\n(Dry run - no changes made)");
    } else {
        print!("\nProceed with deletion? [y/N]: ");
        use std::io::{self, Write};
        io::stdout().flush()?;
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;

        if input.trim().eq_ignore_ascii_case("y") {
            let mut deleted = 0;
            let mut failed = 0;

            for app in to_delete {
                match queue.remove(&app.id).await {
                    Ok(true) => {
                        println!("  ✅ Deleted {}", app.id);
                        deleted += 1;
                    }
                    Ok(false) => {
                        println!("  ⚠️  {} not found", app.id);
                    }
                    Err(e) => {
                        println!("  ❌ Failed to delete {}: {}", app.id, e);
                        failed += 1;
                    }
                }
            }

            println!("\n✅ Cleaned {} approvals ({} failed)", deleted, failed);
        } else {
            println!("Cancelled.");
        }
    }

    Ok(())
}

fn print_approval_summary(app: &ccos::approval::types::ApprovalRequest) {
    let status_str = match &app.status {
        GeneralApprovalStatus::Pending => "⏳ PENDING",
        GeneralApprovalStatus::Approved { .. } => "✅ APPROVED",
        GeneralApprovalStatus::Rejected { .. } => "❌ REJECTED",
        GeneralApprovalStatus::Expired { .. } => "⌛ EXPIRED",
        GeneralApprovalStatus::Superseded { .. } => "🔄 SUPERSEDED",
    };

    let cat_type = format_category(&app.category);
    println!(
        "  {} | {} | {} (risk: {:?})",
        app.id, status_str, cat_type, app.risk_assessment.level
    );
}

fn print_approval_details(app: &ccos::approval::types::ApprovalRequest) {
    let age = chrono::Utc::now().signed_duration_since(app.requested_at);
    println!(
        "    Requested: {} ({} ago)",
        app.requested_at,
        format_duration(age)
    );
    println!("    Expires: {}", app.expires_at);

    match &app.status {
        GeneralApprovalStatus::Approved { by, reason, at } => {
            println!("    Approved by: {:?} at {}", by, at);
            if let Some(r) = reason {
                println!("    Reason: {}", r);
            }
        }
        GeneralApprovalStatus::Rejected { by, reason, at } => {
            println!("    Rejected by: {:?} at {}", by, at);
            println!("    Reason: {}", reason);
        }
        _ => {}
    }

    // Print category-specific details
    use ccos::approval::types::ApprovalCategory;
    match &app.category {
        ApprovalCategory::HumanActionRequest {
            title, skill_id, ..
        } => {
            println!("    Title: {}", title);
            println!("    Skill: {}", skill_id);
        }
        ApprovalCategory::ServerDiscovery { server_info, .. } => {
            println!("    Server: {}", server_info.name);
            println!("    Endpoint: {}", server_info.endpoint);
        }
        ApprovalCategory::SecretWrite {
            key, description, ..
        } => {
            println!("    Secret Key: {}", key);
            println!("    Description: {}", description);
        }
        ApprovalCategory::PackageApproval { package, runtime } => {
            println!("    Package: {}", package);
            println!("    Runtime: {}", runtime);
        }
        _ => {}
    }

    if !app.metadata.is_empty() {
        println!("    Metadata: {:?}", app.metadata);
    }
}

fn format_category(cat: &ccos::approval::types::ApprovalCategory) -> String {
    use ccos::approval::types::ApprovalCategory;
    match cat {
        ApprovalCategory::ServerDiscovery { .. } => "ServerDiscovery".to_string(),
        ApprovalCategory::HumanActionRequest { .. } => "HumanActionRequest".to_string(),
        ApprovalCategory::SecretWrite { .. } => "SecretWrite".to_string(),
        ApprovalCategory::EffectApproval { .. } => "EffectApproval".to_string(),
        ApprovalCategory::SynthesisApproval { .. } => "SynthesisApproval".to_string(),
        ApprovalCategory::LlmPromptApproval { .. } => "LlmPromptApproval".to_string(),
        ApprovalCategory::BudgetExtension { .. } => "BudgetExtension".to_string(),
        ApprovalCategory::SecretRequired { .. } => "SecretRequired".to_string(),
        ApprovalCategory::ChatPolicyException { .. } => "ChatPolicyException".to_string(),
        ApprovalCategory::ChatPublicDeclassification { .. } => {
            "ChatPublicDeclassification".to_string()
        }
        ApprovalCategory::HttpHostApproval { .. } => "HttpHostApproval".to_string(),
        ApprovalCategory::PackageApproval { .. } => "PackageApproval".to_string(),
        ApprovalCategory::SandboxNetwork { .. } => "SandboxNetwork".to_string(),
    }
}

fn format_duration(duration: chrono::Duration) -> String {
    let hours = duration.num_hours();
    let minutes = duration.num_minutes() % 60;

    if hours > 24 {
        let days = hours / 24;
        let remaining_hours = hours % 24;
        format!("{}d {}h", days, remaining_hours)
    } else if hours > 0 {
        format!("{}h {}m", hours, minutes)
    } else {
        format!("{}m", duration.num_minutes())
    }
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

use ccos::approval::types::ApprovalStorage;
