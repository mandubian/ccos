use crate::cli::CliContext;
use crate::cli::OutputFormatter;
use clap::Subcommand;
use rtfs::runtime::error::{RuntimeResult, RuntimeError};
use std::path::PathBuf;
use std::process::Command;

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
    let _formatter = OutputFormatter::new(ctx.output_format);

    match command {
        RtfsCommand::Eval { expr } => {
            // Using capability_explorer for RTFS eval temporarily to leverage its context
            let mut cmd = Command::new("cargo");
            cmd.arg("run")
               .arg("--example")
               .arg("capability_explorer")
               .arg("--quiet")
               .arg("--") // Separator for arguments passed to the example
               .arg("--rtfs")
               .arg(expr);
            
            let status = cmd.status().map_err(|e| {
                RuntimeError::Generic(format!("Failed to run RTFS eval: {}", e))
            })?;
            
            if !status.success() {
                return Err(RuntimeError::Generic("RTFS evaluation failed".to_string()));
            }
        }
        RtfsCommand::Run { file } => {
             // Using capability_explorer for RTFS run temporarily
            let mut cmd = Command::new("cargo");
            cmd.arg("run")
               .arg("--example")
               .arg("capability_explorer")
               .arg("--quiet")
               .arg("--") // Separator for arguments passed to the example
               .arg("--rtfs-file")
               .arg(file);
               
            let status = cmd.status().map_err(|e| {
                RuntimeError::Generic(format!("Failed to run RTFS file: {}", e))
            })?;
            
            if !status.success() {
                return Err(RuntimeError::Generic("RTFS file execution failed".to_string()));
            }
        }
        RtfsCommand::Repl => {
            ctx.status("Starting RTFS REPL...");
            let mut cmd = Command::new("cargo");
            cmd.arg("run")
               .arg("--bin")
               .arg("rtfs-ccos-repl");
               
            let status = cmd.status().map_err(|e| {
                RuntimeError::Generic(format!("Failed to start REPL: {}", e))
            })?;
            
            if !status.success() {
                return Err(RuntimeError::Generic("REPL exited with error".to_string()));
            }
        }
    }

    Ok(())
}

