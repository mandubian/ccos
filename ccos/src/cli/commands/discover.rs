use crate::cli::CliContext;
use crate::cli::OutputFormatter;
use crate::discovery::{ApprovalQueue, GoalDiscoveryAgent, RegistrySearcher};
use clap::Subcommand;
use rtfs::runtime::error::RuntimeResult;

#[derive(Subcommand)]
pub enum DiscoverCommand {
    /// Discover capabilities from a goal description
    Goal {
        /// Natural language goal description
        goal: String,
    },

    /// Discover capabilities from a specific server
    Server {
        /// Server name
        name: String,

        /// Filter hint
        #[arg(short = 'f', long)]
        filter: Option<String>,
    },

    /// Search discovered capabilities
    Search {
        /// Search query
        query: String,
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
        DiscoverCommand::Goal { goal } => {
            ctx.status(&format!("Discovering capabilities for goal: {}", goal));

            let queue = ApprovalQueue::new("."); // TODO: use configured path
            let agent = GoalDiscoveryAgent::new(queue);

            let queued_ids = agent.process_goal(&goal).await?;

            if queued_ids.is_empty() {
                formatter.warning("No capabilities found.");
            } else {
                formatter.success(&format!(
                    "Found and queued {} potential capabilities.",
                    queued_ids.len()
                ));
                formatter.list_item("Use 'ccos approval pending' to review and approve.");
            }
        }
        DiscoverCommand::Server { name, filter } => {
            ctx.status(&format!("Discovering from server: {}", name));
            if let Some(f) = filter {
                ctx.status(&format!("Filter: {}", f));
            }
            formatter.warning("Server discovery not yet implemented");
            formatter.list_item("See: https://github.com/mandubian/ccos/issues/172");
        }
        DiscoverCommand::Search { query } => {
            ctx.status(&format!("Searching registry for: {}", query));
            let searcher = RegistrySearcher::new();
            let results = searcher.search(&query).await?;

            if results.is_empty() {
                formatter.warning("No results found.");
            } else {
                formatter.section("Search Results");
                for result in results {
                    formatter.kv("Source", &result.source.name());
                    formatter.kv("Server", &result.server_info.name);
                    formatter.kv(
                        "Description",
                        &result.server_info.description.clone().unwrap_or_default(),
                    );
                    println!();
                }
            }
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

