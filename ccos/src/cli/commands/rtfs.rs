use crate::cli::CliContext;
use crate::cli::OutputFormatter;
use clap::Subcommand;
use rtfs::runtime::error::RuntimeResult;
use std::path::PathBuf;

#[derive(Subcommand)]
pub enum RtfsCommand {
    /// Evaluate an RTFS expression
    Eval {
        /// RTFS expression to evaluate
        expr: String,
    },

    /// Run an RTFS file
    Run {
        /// Path to RTFS file
        file: PathBuf,
    },

    /// Start interactive REPL
    Repl,
}

pub async fn execute(
    ctx: &mut CliContext,
    command: RtfsCommand,
) -> RuntimeResult<()> {
    let formatter = OutputFormatter::new(ctx.output_format);

    match command {
        RtfsCommand::Eval { expr } => {
            formatter.warning(&format!("RTFS eval not yet implemented. Expr: {}", expr));
        }
        RtfsCommand::Run { file } => {
            formatter.warning(&format!("RTFS run not yet implemented. File: {:?}", file));
        }
        RtfsCommand::Repl => {
            formatter.warning("RTFS REPL not yet implemented");
            formatter.list_item("Use: cargo run --bin rtfs-repl");
        }
    }

    Ok(())
}

