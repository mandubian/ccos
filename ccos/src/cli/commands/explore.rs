use crate::cli::CliContext;
use clap::Args;
use rtfs::runtime::error::RuntimeResult;

#[derive(Args)]
pub struct ExploreArgs {
    /// Start with a specific server selected
    #[arg(short, long)]
    pub server: Option<String>,
}

pub async fn execute(
    _ctx: &mut CliContext,
    args: ExploreArgs,
) -> RuntimeResult<()> {
    // For now, suggest using the existing capability_explorer
    eprintln!("Interactive explorer launching...");
    if let Some(s) = args.server {
        eprintln!("Starting with server: {}", s);
    }
    eprintln!();
    eprintln!("Note: For now, use the capability_explorer example:");
    eprintln!("  cargo run --example capability_explorer -- --config ../config/agent_config.toml");
    eprintln!();
    eprintln!("See: https://github.com/mandubian/ccos/issues/172");

    Ok(())
}

