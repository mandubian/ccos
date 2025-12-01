use crate::cli::CliContext;
use crate::cli::OutputFormatter;
use clap::Subcommand;
use rtfs::runtime::error::RuntimeResult;

#[derive(Subcommand)]
pub enum ApprovalCommand {
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

pub async fn execute(
    ctx: &mut CliContext,
    command: ApprovalCommand,
) -> RuntimeResult<()> {
    let formatter = OutputFormatter::new(ctx.output_format);

    match command {
        ApprovalCommand::Pending => {
            let result = crate::ops::approval::list_pending().await?;
            if result.items.is_empty() {
                formatter.success("No pending approvals");
            } else {
                formatter.section("Pending Approvals");
                for item in result.items {
                    formatter.kv("ID", &item.id);
                    formatter.kv("Source", &item.source);
                    formatter.kv("Server", &item.server_name);
                    formatter.kv("Endpoint", &item.endpoint);
                    formatter.kv("Risk Level", &item.risk_level);
                    if let Some(goal) = &item.goal {
                        formatter.kv("Goal", goal);
                    }
                    println!();
                }
            }
        }
        ApprovalCommand::Approve { id, reason } => {
            crate::ops::approval::approve_discovery(id.clone(), reason.clone()).await?;
            formatter.success(&format!("Approved discovery: {}", id));
            if let Some(r) = reason {
                formatter.list_item(&format!("Reason: {}", r));
            }
        }
        ApprovalCommand::Reject { id, reason } => {
            crate::ops::approval::reject_discovery(id.clone(), reason.clone()).await?;
            formatter.success(&format!("Rejected discovery: {}", id));
            formatter.list_item(&format!("Reason: {}", reason));
        }
        ApprovalCommand::Timeout => {
            let result = crate::ops::approval::list_timeout().await?;
            if result.items.is_empty() {
                formatter.success("No timed-out discoveries");
            } else {
                formatter.section("Timed-out Discoveries");
                for item in result.items {
                    formatter.kv("ID", &item.id);
                    formatter.kv("Source", &item.source);
                    formatter.kv("Server", &item.server_name);
                    formatter.kv("Requested At", &item.requested_at);
                    println!();
                }
            }
        }
    }

    Ok(())
}
