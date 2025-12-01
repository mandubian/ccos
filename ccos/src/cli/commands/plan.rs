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
    command: PlanCommand,
) -> RuntimeResult<()> {
    match command {
        PlanCommand::Create { goal } => {
            let result = crate::ops::plan::create_plan(goal).await?;
            println!("{}", result);
        }
        PlanCommand::Execute { plan } => {
            let result = crate::ops::plan::execute_plan(plan).await?;
            println!("{}", result);
        }
        PlanCommand::Validate { plan } => {
            let valid = crate::ops::plan::validate_plan(plan).await?;
            if valid {
                println!("Plan is valid.");
            } else {
                println!("Plan is invalid.");
            }
        }
    }
    Ok(())
}
