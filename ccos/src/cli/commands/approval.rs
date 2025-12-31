use crate::cli::CliContext;
use crate::cli::OutputFormatter;
use clap::Subcommand;
#[cfg(feature = "tui")]
use dialoguer::{Confirm, Select};
use rtfs::runtime::error::RuntimeResult;

#[derive(Subcommand)]
pub enum ApprovalCommand {
    /// List pending approvals
    Pending,

    /// Approve a pending request
    Approve {
        /// Request ID
        id: String,

        /// Approval reason
        #[arg(short, long)]
        reason: Option<String>,

        /// Force merge without prompting (for scripts, discovery only)
        #[arg(long)]
        force_merge: bool,

        /// Skip if server already approved (for scripts, discovery only)
        #[arg(long)]
        skip_existing: bool,
    },

    /// Reject a pending request
    Reject {
        /// Request ID
        id: String,

        /// Rejection reason
        #[arg(short, long)]
        reason: String,
    },

    /// List timed-out requests
    Timeout,
}

pub async fn execute(ctx: &mut CliContext, command: ApprovalCommand) -> RuntimeResult<()> {
    let formatter = OutputFormatter::new(ctx.output_format);

    match command {
        ApprovalCommand::Pending => {
            let storage_path = ctx.resolve_workspace_path(&ctx.config.storage.approvals_dir);
            let result = crate::ops::approval::list_pending(storage_path).await?;
            if result.items.is_empty() {
                formatter.success("No pending approvals");
            } else {
                formatter.section("Pending Approvals");
                for item in result.items {
                    formatter.kv("ID", &item.id);
                    formatter.kv("Type", &format!("{:?}", item.approval_type));
                    formatter.kv("Source", &item.source);
                    formatter.kv("Title", &item.title);
                    formatter.kv("Description", &item.description);
                    formatter.kv("Risk Level", &item.risk_level);
                    if let Some(goal) = &item.goal {
                        formatter.kv("Goal", goal);
                    }
                    println!();
                }
            }
        }
        ApprovalCommand::Approve {
            id,
            reason,
            force_merge,
            skip_existing,
        } => {
            let storage_path = ctx.resolve_workspace_path(&ctx.config.storage.approvals_dir);
            // Check for conflict with existing approved server (only relevant for discovery)
            if let Some(conflict) =
                crate::ops::approval::check_approval_conflict(storage_path.clone(), id.clone())
                    .await?
            {
                println!();
                formatter.warning(&format!(
                    "Server \"{}\" already exists in approved list",
                    conflict.existing_name
                ));
                println!(
                    "  Current: v{}, {} capability files, approved on {}",
                    conflict.existing_version,
                    conflict.existing_tool_count,
                    &conflict.existing_approved_at[..10] // Just the date
                );
                println!(
                    "  New: \"{}\" ({})",
                    conflict.pending_name, conflict.pending_endpoint
                );
                println!();

                if skip_existing {
                    formatter.info("Skipping (--skip-existing flag)");
                    return Ok(());
                }

                if !force_merge {
                    let options = vec![
                        "Merge - Add new tools, keep usage stats, increment version",
                        "Skip - Keep existing, discard this pending item",
                        "Cancel - Do nothing",
                    ];

                    #[cfg(feature = "tui")]
                    let selection = Select::new()
                        .with_prompt("What would you like to do?")
                        .items(&options)
                        .default(0)
                        .interact()
                        .map_err(|e| {
                            rtfs::runtime::error::RuntimeError::Generic(format!(
                                "Selection error: {}",
                                e
                            ))
                        })?;

                    #[cfg(not(feature = "tui"))]
                    let selection = return Err(rtfs::runtime::error::RuntimeError::Generic(
                        "Interactive approval not available in minimal build. Use --force-merge or --skip-existing.".to_string(),
                    ));

                    match selection {
                        0 => {
                            // Merge - proceed with approval (existing logic handles merge)
                            crate::ops::approval::approve_request(
                                storage_path.clone(),
                                id.clone(),
                                reason.clone(),
                            )
                            .await?;
                            formatter.success(&format!(
                                "Merged into existing server (now v{})",
                                conflict.existing_version + 1
                            ));
                        }
                        1 => {
                            // Skip - remove from pending without approving
                            crate::ops::approval::skip_pending(storage_path.clone(), id.clone())
                                .await?;
                            formatter
                                .info("Skipped - pending item removed, existing server unchanged");
                        }
                        _ => {
                            formatter.info("Cancelled");
                            return Ok(());
                        }
                    }
                } else {
                    // Force merge
                    crate::ops::approval::approve_request(
                        storage_path.clone(),
                        id.clone(),
                        reason.clone(),
                    )
                    .await?;
                    formatter.success(&format!(
                        "Force-merged into existing server (now v{})",
                        conflict.existing_version + 1
                    ));
                }
            } else {
                // No conflict or not a discovery request - normal approval
                crate::ops::approval::approve_request(
                    storage_path.clone(),
                    id.clone(),
                    reason.clone(),
                )
                .await?;
                formatter.success(&format!("Approved request: {}", id));
                if let Some(r) = reason {
                    formatter.list_item(&format!("Reason: {}", r));
                }
            }
        }
        ApprovalCommand::Reject { id, reason } => {
            let storage_path = ctx.resolve_workspace_path(&ctx.config.storage.approvals_dir);
            crate::ops::approval::reject_request(storage_path, id.clone(), reason.clone()).await?;
            formatter.success(&format!("Rejected request: {}", id));
            formatter.list_item(&format!("Reason: {}", reason));
        }
        ApprovalCommand::Timeout => {
            let storage_path = ctx.resolve_workspace_path(&ctx.config.storage.approvals_dir);
            let result = crate::ops::approval::list_timeout(storage_path).await?;
            if result.items.is_empty() {
                formatter.success("No timed-out requests");
            } else {
                formatter.section("Timed-out Requests");
                for item in result.items {
                    formatter.kv("ID", &item.id);
                    formatter.kv("Type", &format!("{:?}", item.approval_type));
                    formatter.kv("Source", &item.source);
                    formatter.kv("Title", &item.title);
                    formatter.kv("Requested At", &item.requested_at);
                    println!();
                }
            }
        }
    }

    Ok(())
}
