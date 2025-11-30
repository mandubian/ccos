use crate::cli::CliContext;
use crate::cli::OutputFormatter;
use crate::discovery::approval_queue::ApprovalQueue;
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
    let queue = ApprovalQueue::new("."); // TODO: Use configured data path

    match command {
        ApprovalCommand::Pending => {
            let pending = queue.list_pending()?;
            if pending.is_empty() {
                formatter.success("No pending approvals");
            } else {
                formatter.section("Pending Approvals");
                for item in pending {
                    formatter.kv("ID", &item.id);
                    formatter.kv("Source", &item.source.name());
                    formatter.kv("Server", &item.server_info.name);
                    formatter.kv("Endpoint", &item.server_info.endpoint);
                    formatter.kv("Risk Level", &format!("{:?}", item.risk_assessment.level));
                    if let Some(goal) = &item.requesting_goal {
                        formatter.kv("Goal", goal);
                    }
                    println!();
                }
            }
        }
        ApprovalCommand::Approve { id, reason } => {
            queue.approve(&id, reason.clone())?;
            formatter.success(&format!("Approved discovery: {}", id));
            if let Some(r) = reason {
                formatter.list_item(&format!("Reason: {}", r));
            }
        }
        ApprovalCommand::Reject { id, reason } => {
            queue.reject(&id, reason.clone())?;
            formatter.success(&format!("Rejected discovery: {}", id));
            formatter.list_item(&format!("Reason: {}", reason));
        }
        ApprovalCommand::Timeout => {
            let timeouts = queue.list_timeouts()?;
            if timeouts.is_empty() {
                formatter.success("No timed-out discoveries");
            } else {
                formatter.section("Timed-out Discoveries");
                for item in timeouts {
                    formatter.kv("ID", &item.id);
                    formatter.kv("Source", &item.source.name());
                    formatter.kv("Server", &item.server_info.name);
                    formatter.kv("Requested At", &item.requested_at.to_string());
                    formatter.kv("Expired At", &item.expires_at.to_string());
                    println!();
                }
            }
        }
    }

    Ok(())
}
