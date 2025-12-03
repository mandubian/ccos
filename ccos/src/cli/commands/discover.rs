use crate::cli::CliContext;
use crate::cli::OutputFormatter;
use crate::ops::discover::DiscoverOptions;
use clap::Subcommand;
use rtfs::runtime::error::RuntimeResult;

#[derive(Subcommand)]
pub enum DiscoverCommand {
    /// Discover capabilities from a goal description
    Goal {
        /// Natural language goal description
        goal: String,

        /// Interactive mode - select which discoveries to queue
        #[arg(short, long)]
        interactive: bool,

        /// Limit results to top N (by relevance score)
        #[arg(long, value_name = "N")]
        top: Option<usize>,

        /// Minimum relevance score threshold (0.0-1.0)
        #[arg(long, value_name = "SCORE", default_value = "0.65")]
        threshold: f64,

        /// Use LLM for semantic analysis (requires API key)
        #[arg(long)]
        llm: bool,
    },

    /// Discover capabilities from a specific server
    Server {
        /// Server name
        name: String,

        /// Filter hint
        #[arg(short = 'f', long)]
        filter: Option<String>,
    },

    /// List all discovered capabilities
    List,

    /// Inspect a specific capability
    Inspect {
        /// Capability ID
        id: String,
    },
}

pub async fn execute(
    ctx: &mut CliContext,
    command: DiscoverCommand,
) -> RuntimeResult<()> {
    let formatter = OutputFormatter::new(ctx.output_format);

    match command {
        DiscoverCommand::Goal { goal, interactive, top, threshold, llm } => {
            ctx.status(&format!("Discovering capabilities for goal: {}", goal));

            let options = DiscoverOptions {
                interactive,
                top,
                threshold,
                llm,
            };

            let queued_ids = crate::ops::discover::discover_by_goal_with_options(goal, options).await?;

            if queued_ids.is_empty() {
                formatter.warning("No servers found matching criteria.");
            } else {
                formatter.success(&format!(
                    "Queued {} servers for approval.",
                    queued_ids.len()
                ));
                formatter.list_item("Use 'ccos approval pending' to review and approve.");
            }
        }
        DiscoverCommand::Server { name, filter } => {
            ctx.status(&format!("Discovering capabilities from server: {}", name));
            if let Some(f) = filter {
                ctx.status(&format!("Filter: {}", f));
            }
            formatter.warning("Server capability discovery not yet implemented");
            formatter.list_item("This will connect to the server and list available tools/capabilities.");
            formatter.list_item("See: https://github.com/mandubian/ccos/issues/172");
        }
        DiscoverCommand::List => {
            formatter.warning("Capability listing not yet implemented");
        }
        DiscoverCommand::Inspect { id } => {
            formatter.warning(&format!(
                "Capability inspection not yet implemented. ID: {}",
                id
            ));
        }
    }

    Ok(())
}
