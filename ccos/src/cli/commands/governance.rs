use crate::cli::CliContext;
use clap::Subcommand;
use rtfs::runtime::error::RuntimeResult;

#[derive(Subcommand)]
pub enum GovernanceCommand {
    /// Check if action is allowed
    Check {
        /// Action to check
        action: String,
    },

    /// View audit trail
    Audit,

    /// View/edit constitution
    Constitution,
}

pub async fn execute(
    _ctx: &mut CliContext,
    command: GovernanceCommand,
) -> RuntimeResult<()> {
    match command {
        GovernanceCommand::Check { action } => {
            let allowed = crate::ops::governance::check_action(action).await?;
            if allowed {
                println!("Action allowed.");
            } else {
                println!("Action denied.");
            }
        }
        GovernanceCommand::Audit => {
            let trail = crate::ops::governance::view_audit().await?;
            println!("{}", trail);
        }
        GovernanceCommand::Constitution => {
            let content = crate::ops::governance::view_constitution().await?;
            println!("{}", content);
        }
    }
    Ok(())
}
