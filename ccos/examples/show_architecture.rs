use clap::Parser;
use std::error::Error;

use ccos::planner_viz_common::{load_agent_config, print_architecture_summary};

#[derive(Parser, Debug)]
#[command(
    author,
    version,
    about = "Display the CCOS smart assistant architecture summary from an agent config"
)]
struct Args {
    /// Path to the agent configuration file (TOML or JSON)
    #[arg(long, default_value = "config/agent_config.toml")]
    config: String,

    /// Optional LLM profile name to highlight in the summary
    #[arg(long)]
    profile: Option<String>,
}

fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();
    let config = load_agent_config(&args.config)?;
    print_architecture_summary(&config, args.profile.as_deref());
    Ok(())
}
