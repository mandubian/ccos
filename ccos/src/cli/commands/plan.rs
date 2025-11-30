use crate::cli::CliContext;
use clap::Subcommand;
use rtfs::runtime::error::RuntimeResult;

#[derive(Subcommand)]
pub enum PlanCommand {
    /// Create plan from goal
    Create {
        /// Goal description
        goal: String,
    },

    /// Execute a plan
    Execute {
        /// Plan ID or path
        plan: String,
    },

    /// Validate plan syntax
    Validate {
        /// Plan ID or path
        plan: String,
    },
}

pub async fn execute(
    _ctx: &mut CliContext,
    _command: PlanCommand,
) -> RuntimeResult<()> {
    // Placeholder
    println!("Plan command not yet implemented");
    Ok(())
}

