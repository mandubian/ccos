use crate::cli::CliContext;
use clap::Args;
use rtfs::runtime::error::RuntimeResult;
use std::process::Command;

#[derive(Args)]
pub struct ExploreArgs {
    /// Start with a specific server selected
    #[arg(short, long)]
    pub server: Option<String>,
}

pub async fn execute(_ctx: &mut CliContext, args: ExploreArgs) -> RuntimeResult<()> {
    // Launch capability_explorer example as a subprocess for now
    // In future this should be integrated directly

    let mut cmd = Command::new("cargo");
    cmd.arg("run")
        .arg("--example")
        .arg("capability_explorer")
        .arg("--");

    if let Some(s) = args.server {
        cmd.arg("--server");
        cmd.arg(s);
    }

    // Pass through config if we knew where it was, but here we use default
    cmd.arg("--config");
    cmd.arg("config/agent_config.toml");

    let status = cmd.status().map_err(|e| {
        rtfs::runtime::error::RuntimeError::Generic(format!("Failed to launch explorer: {}", e))
    })?;

    if !status.success() {
        return Err(rtfs::runtime::error::RuntimeError::Generic(
            "Explorer exited with error".to_string(),
        ));
    }

    Ok(())
}
