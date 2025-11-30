use crate::cli::CliContext;
use crate::cli::OutputFormatter;
use crate::discovery::{ApprovalQueue, DiscoverySource, PendingDiscovery, RiskAssessment, RiskLevel, ServerInfo};
use chrono::Utc;
use clap::Subcommand;
use rtfs::runtime::error::RuntimeResult;
use uuid::Uuid;

#[derive(Subcommand)]
pub enum ServerCommand {
    /// List configured servers
    List,

    /// Add a new server
    Add {
        /// Server URL
        url: String,

        /// Server name
        #[arg(short, long)]
        name: Option<String>,
    },

    /// Remove a server
    Remove {
        /// Server name or ID
        name: String,
    },

    /// Show server health status
    Health {
        /// Specific server (all if not specified)
        name: Option<String>,
    },

    /// Dismiss a failing server
    Dismiss {
        /// Server name
        name: String,

        /// Reason for dismissal
        #[arg(short, long)]
        reason: Option<String>,
    },

    /// Retry a dismissed server
    Retry {
        /// Server name
        name: String,
    },
}

pub async fn execute(
    ctx: &mut CliContext,
    command: ServerCommand,
) -> RuntimeResult<()> {
    let formatter = OutputFormatter::new(ctx.output_format);

    match command {
        ServerCommand::List => {
            let queue = ApprovalQueue::new("."); // TODO: use configured path
            let approved = queue.list_approved()?;

            if approved.is_empty() {
                formatter.warning("No approved servers.");
            } else {
                formatter.section("Approved Servers");
                for server in approved {
                    formatter.kv("ID", &server.id);
                    formatter.kv("Name", &server.server_info.name);
                    formatter.kv("Endpoint", &server.server_info.endpoint);
                    formatter.kv("Source", &server.source.name());
                    if server.should_dismiss() {
                        formatter.kv("Status", "FAILING (Should Dismiss)");
                    } else {
                        formatter.kv("Status", "Healthy");
                    }
                    println!();
                }
            }
        }
        ServerCommand::Add { url, name } => {
            let queue = ApprovalQueue::new(".");
            let name_str = name.unwrap_or_else(|| "manual-server".to_string());

            let discovery = PendingDiscovery {
                id: format!("manual-{}", Uuid::new_v4()),
                source: DiscoverySource::Manual {
                    user: "cli".to_string(),
                },
                server_info: ServerInfo {
                    name: name_str.clone(),
                    endpoint: url.clone(),
                    description: Some("Manually added via CLI".to_string()),
                },
                domain_match: vec![],
                risk_assessment: RiskAssessment {
                    level: RiskLevel::Low,
                    reasons: vec!["manual_add".to_string()],
                },
                requested_at: Utc::now(),
                expires_at: Utc::now() + chrono::Duration::hours(24 * 30),
                requesting_goal: None,
            };

            queue.add(discovery.clone())?;
            formatter.success(&format!("Added server '{}' to approval queue.", name_str));
            formatter.kv("ID", &discovery.id);
        }
        ServerCommand::Remove { name } => {
            formatter.warning(&format!("Server remove not yet implemented. Name: {}", name));
        }
        ServerCommand::Health { name } => {
            let queue = ApprovalQueue::new(".");
            let approved = queue.list_approved()?;

            let target_servers: Vec<_> = if let Some(n) = name {
                approved
                    .into_iter()
                    .filter(|s| s.server_info.name == n || s.id == n)
                    .collect()
            } else {
                approved
            };

            if target_servers.is_empty() {
                formatter.warning("No matching servers found.");
            } else {
                for server in target_servers {
                    formatter.section(&format!("Health: {}", server.server_info.name));
                    formatter.kv("ID", &server.id);
                    formatter.kv("Total Calls", &server.total_calls.to_string());
                    formatter.kv("Total Errors", &server.total_errors.to_string());
                    formatter.kv(
                        "Consecutive Failures",
                        &server.consecutive_failures.to_string(),
                    );
                    formatter.kv("Error Rate", &format!("{:.2}%", server.error_rate() * 100.0));
                    formatter.kv(
                        "Dismissal Check",
                        if server.should_dismiss() {
                            "FAIL"
                        } else {
                            "PASS"
                        },
                    );
                    println!();
                }
            }
        }
        ServerCommand::Dismiss { name, reason } => {
            formatter.warning(&format!(
                "Server dismiss not yet implemented. Name: {}, Reason: {:?}",
                name, reason
            ));
        }
        ServerCommand::Retry { name } => {
            formatter.warning(&format!("Server retry not yet implemented. Name: {}", name));
        }
    }

    Ok(())
}

