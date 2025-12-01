// Server command implementation
use crate::cli::output::OutputFormatter;
use crate::cli::CliContext;
use clap::Subcommand;
use rtfs::runtime::error::RuntimeResult;

#[derive(Subcommand)]
pub enum ServerCommand {
    List,
    Add {
        url: String,
        name: Option<String>,
    },
    Remove {
        name: String,
    },
    Health {
        name: Option<String>,
    },
    Search {
        query: String,
        capability: Option<String>,
        select: Option<usize>,
        select_by_name: Option<String>,
        llm: bool,
        llm_model: Option<String>,
    },
    Dismiss {
        name: String,
        reason: Option<String>,
    },
    Retry {
        name: String,
    },
}

pub async fn execute(ctx: &mut CliContext, command: ServerCommand) -> RuntimeResult<()> {
    let formatter = OutputFormatter::new(ctx.output_format);
    command.execute_impl(ctx, &formatter).await
}

impl ServerCommand {
    pub async fn execute_impl(&self, ctx: &CliContext, formatter: &OutputFormatter) -> RuntimeResult<()> {
        match self {
        ServerCommand::List => {
                let result = crate::ops::server::list_servers().await?;

                if result.servers.is_empty() {
                formatter.warning("No approved servers.");
            } else {
                formatter.section("Approved Servers");
                    for server in result.servers {
                    formatter.kv("ID", &server.id);
                        formatter.kv("Name", &server.name);
                        formatter.kv("Endpoint", &server.endpoint);
                        if let Some(src) = &server.source {
                            formatter.kv("Source", src);
                        }
                        if let Some(auth) = &server.auth_status {
                            formatter.kv("Auth", auth);
                        }
                        if server.status == "failing" {
                        formatter.kv("Status", "FAILING (Should Dismiss)");
                    } else {
                        formatter.kv("Status", "Healthy");
                    }
                    println!();
                }
            }
        }
        ServerCommand::Add { url, name } => {
                let name_str = name.clone().unwrap_or_else(|| "manual-server".to_string());
                let id = crate::ops::server::add_server(url.clone(), name.clone()).await?;

            formatter.success(&format!("Added server '{}' to approval queue.", name_str));
                formatter.kv("ID", &id);
                formatter.list_item("Use 'ccos approval list' to view status.");
        }
        ServerCommand::Remove { name } => {
                crate::ops::server::remove_server(name.clone()).await?;
                formatter.success("Server removed successfully.");
        }
        ServerCommand::Health { name } => {
                let health_info = crate::ops::server::server_health(name.clone()).await?;

                if health_info.is_empty() {
                formatter.warning("No matching servers found.");
            } else {
                    for server in health_info {
                        formatter.section(&format!("Health: {}", server.name));
                    formatter.kv("ID", &server.id);
                        if let Some(score) = server.health_score {
                             formatter.kv("Error Rate", &format!("{:.2}%", score * 100.0));
                        }
                        formatter.kv("Status", &server.status);
                        if let Some(auth) = &server.auth_status {
                            formatter.kv("Auth", auth);
                        }
                        println!();
                    }
            }
        }
        ServerCommand::Search { query, capability, select, select_by_name, llm, llm_model } => {
                if *llm {
                     // ctx.status is not available on &CliContext (it might be on &mut CliContext or OutputFormatter?)
                     // checking CliContext definition might be needed.
                     // But formatter has info/status-like methods.
                     formatter.info("LLM fallback enabled for API documentation parsing");
                }
                if let Some(cap) = &capability {
                     formatter.info(&format!("Filtering by capability: {}", cap));
                     formatter.info("Connecting to servers to check capabilities...");
                }

                let results = crate::ops::server::search_servers(query.clone(), capability.clone(), *llm, llm_model.clone()).await?;
            
            if results.is_empty() {
                formatter.warning("No servers found.");
                if capability.is_some() {
                    formatter.list_item("Try a different capability name or check server endpoints.");
                }
            } else {
                formatter.section("Search Results");
                    for (idx, server) in results.iter().enumerate() {
                    formatter.kv("Index", &format!("{}", idx + 1));
                        if let Some(src) = &server.source {
                             formatter.kv("Source", src);
                        }
                        formatter.kv("Server", &server.name);
                        formatter.kv("Endpoint", &server.endpoint);
                        if let Some(desc) = &server.description {
                            formatter.kv("Description", desc);
                        }
                        if let Some(auth) = &server.auth_status {
                            formatter.kv("Auth", auth);
                        }
                        if let Some(caps) = &server.matching_capabilities {
                            if !caps.is_empty() {
                                formatter.kv("Matching Capabilities", &caps.join(", "));
                            }
                        }
                    println!();
                }
                
                    // Interactive selection
                let selected_result = if let Some(idx) = select {
                        if *idx == 0 || *idx > results.len() {
                        formatter.warning(&format!("Invalid index: {}. Must be between 1 and {}", idx, results.len()));
                        return Ok(());
                    }
                    Some(&results[idx - 1])
                } else if let Some(ref name) = select_by_name {
                        results.iter().find(|r| r.name == *name || r.name.contains(name))
                } else {
                    None
                };
                
                if let Some(selected) = selected_result {
                        if selected.endpoint.is_empty() || !selected.endpoint.starts_with("http") {
                        formatter.warning("Selected server does not have an HTTP endpoint. Cannot introspect capabilities.");
                        formatter.list_item("Only servers with HTTP/HTTPS endpoints can be introspected.");
                        return Ok(());
                    }
                    
                        match crate::ops::server::add_server(selected.endpoint.clone(), Some(selected.name.clone())).await {
                            Ok(id) => {
                                 formatter.success(&format!("Added server '{}' to approval queue.", selected.name));
                                 formatter.kv("ID", &id);
                                 formatter.list_item("Use 'ccos approval list' to view status.");
                            },
                            Err(e) => {
                                 formatter.warning(&format!("Failed to add server: {}", e));
                            }
                        }
                    }
                }
            }
            ServerCommand::Dismiss { name, reason } => {
                 let queue = crate::discovery::ApprovalQueue::new(".");
                 let reason_str = reason.clone().unwrap_or_else(|| "Manual dismissal".to_string());
                
                 match queue.dismiss_server(name, reason_str.clone()) {
                     Ok(_) => {
                         formatter.success(&format!("Dismissed server '{}'", name));
                         formatter.kv("Reason", &reason_str);
                                        }
                                        Err(e) => {
                         formatter.warning(&format!("Failed to dismiss server: {}", e));
                     }
                 }
            }
            ServerCommand::Retry { name } => {
                let queue = crate::discovery::ApprovalQueue::new(".");
                match queue.retry_server(name) {
                    Ok(_) => {
                        formatter.success(&format!("Retried server '{}'", name));
                        formatter.list_item("Server moved back to Approved list with reset health stats.");
                            }
                            Err(e) => {
                        formatter.warning(&format!("Failed to retry server: {}", e));
                    }
                }
            }
        }
    Ok(())
}
}
