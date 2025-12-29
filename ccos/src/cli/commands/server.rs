// Server command implementation
use crate::cli::output::OutputFormatter;
use crate::cli::CliContext;
use crate::utils::fs::find_workspace_root;
use clap::Subcommand;
use rtfs::runtime::error::RuntimeResult;

#[derive(Subcommand)]
pub enum ServerCommand {
    /// List approved servers
    List,
    /// Add a server to approval queue
    Add { url: String, name: Option<String> },
    /// Remove a server
    Remove { name: String },
    /// Check server health
    Health { name: Option<String> },
    /// Search for servers
    Search {
        query: String,
        capability: Option<String>,
        select: Option<usize>,
        select_by_name: Option<String>,
        llm: bool,
        llm_model: Option<String>,
    },
    /// Dismiss a failing server
    Dismiss {
        name: String,
        reason: Option<String>,
    },
    /// Retry a dismissed server
    Retry { name: String },
    /// Introspect a server to discover its tools/capabilities
    Introspect {
        /// Server name or endpoint URL
        server: String,
    },
}

pub async fn execute(ctx: &mut CliContext, command: ServerCommand) -> RuntimeResult<()> {
    let formatter = OutputFormatter::new(ctx.output_format);
    command.execute_impl(ctx, &formatter).await
}

impl ServerCommand {
    pub async fn execute_impl(
        &self,
        ctx: &CliContext,
        formatter: &OutputFormatter,
    ) -> RuntimeResult<()> {
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
            ServerCommand::Search {
                query,
                capability,
                select,
                select_by_name,
                llm,
                llm_model,
            } => {
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

                let results = crate::ops::server::search_servers(
                    query.clone(),
                    capability.clone(),
                    *llm,
                    llm_model.clone(),
                )
                .await?;

                if results.is_empty() {
                    formatter.warning("No servers found.");
                    if capability.is_some() {
                        formatter.list_item(
                            "Try a different capability name or check server endpoints.",
                        );
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
                            formatter.warning(&format!(
                                "Invalid index: {}. Must be between 1 and {}",
                                idx,
                                results.len()
                            ));
                            return Ok(());
                        }
                        Some(&results[idx - 1])
                    } else if let Some(ref name) = select_by_name {
                        results
                            .iter()
                            .find(|r| r.name == *name || r.name.contains(name))
                    } else {
                        None
                    };

                    if let Some(selected) = selected_result {
                        if selected.endpoint.is_empty() || !selected.endpoint.starts_with("http") {
                            formatter.warning("Selected server does not have an HTTP endpoint. Cannot introspect capabilities.");
                            formatter.list_item(
                                "Only servers with HTTP/HTTPS endpoints can be introspected.",
                            );
                            return Ok(());
                        }

                        match crate::ops::server::add_server(
                            selected.endpoint.clone(),
                            Some(selected.name.clone()),
                        )
                        .await
                        {
                            Ok(id) => {
                                formatter.success(&format!(
                                    "Added server '{}' to approval queue.",
                                    selected.name
                                ));
                                formatter.kv("ID", &id);
                                formatter.list_item("Use 'ccos approval list' to view status.");
                            }
                            Err(e) => {
                                formatter.warning(&format!("Failed to add server: {}", e));
                            }
                        }
                    }
                }
            }
            ServerCommand::Dismiss { name, reason } => {
                let workspace_root = find_workspace_root();
                let storage_path = workspace_root.join("capabilities/servers/approvals");
                let storage = std::sync::Arc::new(
                    crate::approval::storage_file::FileApprovalStorage::new(storage_path)?,
                );
                let queue = crate::approval::UnifiedApprovalQueue::new(storage);
                let reason_str = reason
                    .clone()
                    .unwrap_or_else(|| "Manual dismissal".to_string());

                match queue.dismiss_server(name, reason_str.clone()).await {
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
                let workspace_root = find_workspace_root();
                let storage_path = workspace_root.join("capabilities/servers/approvals");
                let storage = std::sync::Arc::new(
                    crate::approval::storage_file::FileApprovalStorage::new(storage_path)?,
                );
                let queue = crate::approval::UnifiedApprovalQueue::new(storage);
                match queue.retry_server(name).await {
                    Ok(_) => {
                        formatter.success(&format!("Retried server '{}'", name));
                        formatter.list_item(
                            "Server moved back to Approved list with reset health stats.",
                        );
                    }
                    Err(e) => {
                        formatter.warning(&format!("Failed to retry server: {}", e));
                    }
                }
            }
            ServerCommand::Introspect { server } => {
                formatter.info(&format!("Introspecting server: {}", server));

                match crate::ops::server::introspect_server(server.clone()).await {
                    Ok(result) => {
                        formatter.section(&format!("Server: {}", result.server_name));
                        formatter.kv("Endpoint", &result.server_url);
                        formatter.kv("Protocol", &result.protocol_version);
                        formatter.kv("Tools Found", &result.tools.len().to_string());
                        println!();

                        if result.tools.is_empty() {
                            formatter.warning("No tools discovered from this server.");
                        } else {
                            formatter.section("Available Tools");
                            for tool in &result.tools {
                                println!("  📦 {}", tool.tool_name);
                                if let Some(desc) = &tool.description {
                                    println!("     {}", desc.chars().take(80).collect::<String>());
                                }
                                if let Some(schema_json) = &tool.input_schema_json {
                                    // Show parameter names from JSON schema
                                    if let Some(props) = schema_json.get("properties") {
                                        if let Some(obj) = props.as_object() {
                                            let params: Vec<&str> =
                                                obj.keys().map(|s| s.as_str()).collect();
                                            println!("     Parameters: {}", params.join(", "));
                                        }
                                    }
                                }
                                println!();
                            }
                        }
                    }
                    Err(e) => {
                        formatter.warning(&format!("Failed to introspect server: {}", e));
                        formatter.list_item(
                            "Make sure the server is accessible and supports MCP protocol.",
                        );
                    }
                }
            }
        }
        Ok(())
    }
}
