use crate::cli::CliContext;
use crate::cli::OutputFormatter;
use crate::discovery::{ApprovalQueue, DiscoverySource, PendingDiscovery, RegistrySearcher, RiskAssessment, RiskLevel, ServerInfo};
use crate::mcp::core::MCPDiscoveryService;
use crate::capability_marketplace::mcp_discovery::MCPServerConfig;
use crate::mcp::types::DiscoveryOptions;
use crate::synthesis::introspection::api_introspector::APIIntrospector;
use chrono::Utc;
use clap::Subcommand;
use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use std::io::{self, Write};
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

    /// Search for servers in registry and overrides
    Search {
        /// Search query
        query: String,

        /// Filter servers that have a specific capability (requires connecting to servers)
        #[arg(long)]
        capability: Option<String>,

        /// Select a server by index (from the search results list) to introspect and add
        #[arg(long)]
        select: Option<usize>,

        /// Select a server by name to introspect and add
        #[arg(long)]
        select_by_name: Option<String>,

        /// Enable LLM-based documentation parsing as a fallback for API discovery
        /// Uses LLM provider from arbiter configuration (llm_profiles in agent_config.toml)
        #[arg(long)]
        llm: bool,

        /// LLM model to use (overrides the model from llm_profiles configuration)
        #[arg(long)]
        llm_model: Option<String>,
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
                    if let Some(ref auth_var) = server.server_info.auth_env_var {
                        let token_set = std::env::var(auth_var).is_ok();
                        if token_set {
                            formatter.kv("Auth", &format!("✓ {} (set)", auth_var));
                        } else {
                            formatter.kv("Auth", &format!("⚠ {} (not set)", auth_var));
                        }
                    }
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
                    auth_env_var: Some(ApprovalQueue::suggest_auth_env_var(&name_str)),
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
            let queue = ApprovalQueue::new(".");
            let reason_str = reason.unwrap_or_else(|| "Manual dismissal".to_string());
            
            match queue.dismiss_server(&name, reason_str.clone()) {
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
            let queue = ApprovalQueue::new(".");
            match queue.retry_server(&name) {
                Ok(_) => {
                    formatter.success(&format!("Retried server '{}'", name));
                    formatter.list_item("Server moved back to Approved list with reset health stats.");
                }
                Err(e) => {
                    formatter.warning(&format!("Failed to retry server: {}", e));
                    formatter.list_item("Check if the server is in the rejected/dismissed list.");
                }
            }
        }
        ServerCommand::Search { query, capability, select, select_by_name, llm, llm_model } => {
            ctx.status(&format!("Searching for servers: {}", query));
            if let Some(ref cap) = capability {
                ctx.status(&format!("Filtering by capability: {}", cap));
            }
            if llm {
                ctx.status("LLM fallback enabled for API documentation parsing");
            }
            
            let searcher = RegistrySearcher::new();
            let initial_results = searcher.search(&query).await?;
            
            // Filter by capability if specified
            let (results, matching_capabilities) = if let Some(ref cap_name) = capability {
                ctx.status("Connecting to servers to check capabilities...");
                let discovery_service = MCPDiscoveryService::new();
                let mut filtered_results = Vec::new();
                let mut matching_caps = std::collections::HashMap::new();
                
                for result in &initial_results {
                    // Only check servers with HTTP endpoints
                    if result.server_info.endpoint.is_empty() || !result.server_info.endpoint.starts_with("http") {
                        if ctx.verbose {
                            ctx.status(&format!("Skipping {} (no HTTP endpoint)", result.server_info.name));
                        }
                        continue;
                    }
                    
                    // Create server config
                    let server_config = MCPServerConfig {
                        name: result.server_info.name.clone(),
                        endpoint: result.server_info.endpoint.clone(),
                        auth_token: None, // Will use env vars if needed
                        timeout_seconds: 10, // Shorter timeout for search
                        protocol_version: "2024-11-05".to_string(),
                    };
                    
                    // Discover tools from server
                    let options = DiscoveryOptions {
                        introspect_output_schemas: false,
                        use_cache: true,
                        register_in_marketplace: false,
                        export_to_rtfs: false,
                        export_directory: None,
                        auth_headers: None,
                        ..Default::default()
                    };
                    
                    match discovery_service.discover_tools(&server_config, &options).await {
                        Ok(tools) => {
                            // Find matching capabilities
                            let matching: Vec<String> = tools.iter()
                                .filter_map(|tool| {
                                    let name_match = tool.tool_name.to_lowercase().contains(&cap_name.to_lowercase());
                                    let desc_match = tool.description.as_ref()
                                        .map(|d| d.to_lowercase().contains(&cap_name.to_lowercase()))
                                        .unwrap_or(false);
                                    
                                    if name_match || desc_match {
                                        Some(tool.tool_name.clone())
                                    } else {
                                        None
                                    }
                                })
                                .collect();
                            
                            if !matching.is_empty() {
                                if ctx.verbose {
                                    ctx.status(&format!("✓ {} has capability '{}'", result.server_info.name, cap_name));
                                }
                                matching_caps.insert(result.server_info.endpoint.clone(), matching);
                                filtered_results.push(result.clone());
                            } else if ctx.verbose {
                                ctx.status(&format!("✗ {} does not have capability '{}'", result.server_info.name, cap_name));
                            }
                        }
                        Err(e) => {
                            if ctx.verbose {
                                ctx.status(&format!("Failed to check {}: {}", result.server_info.name, e));
                            }
                        }
                    }
                }
                
                (filtered_results, Some(matching_caps))
            } else {
                (initial_results, None)
            };
            
            let count = results.len();
            
            if results.is_empty() {
                formatter.warning("No servers found.");
                if capability.is_some() {
                    formatter.list_item("Try a different capability name or check server endpoints.");
                }
            } else {
                formatter.section("Search Results");
                for (idx, result) in results.iter().enumerate() {
                    formatter.kv("Index", &format!("{}", idx + 1));
                    formatter.kv("Source", &result.source.name());
                    formatter.kv("Server", &result.server_info.name);
                    if !result.server_info.endpoint.is_empty() {
                        formatter.kv("Endpoint", &result.server_info.endpoint);
                    }
                    formatter.kv(
                        "Description",
                        &result.server_info.description.clone().unwrap_or_default(),
                    );
                    
                    // Show auth requirement if server needs authentication
                    if let Some(ref auth_var) = result.server_info.auth_env_var {
                        let token_set = std::env::var(auth_var).is_ok();
                        if token_set {
                            formatter.kv("Auth", &format!("✓ {} (set)", auth_var));
                        } else {
                            formatter.kv("Auth", &format!("⚠ {} (not set)", auth_var));
                        }
                    }
                    
                    // Show matching capabilities if filtering by capability
                    if let Some(ref matching_caps) = matching_capabilities {
                        if let Some(caps) = matching_caps.get(&result.server_info.endpoint) {
                            if !caps.is_empty() {
                                formatter.kv("Matching Capabilities", &caps.join(", "));
                            }
                        }
                    }
                    
                    println!();
                }
                
                // Handle server selection
                let selected_result = if let Some(idx) = select {
                    if idx == 0 || idx > results.len() {
                        formatter.warning(&format!("Invalid index: {}. Must be between 1 and {}", idx, results.len()));
                        return Ok(());
                    }
                    Some(&results[idx - 1])
                } else if let Some(ref name) = select_by_name {
                    results.iter().find(|r| r.server_info.name == *name || r.server_info.name.contains(name))
                } else {
                    None
                };
                
                if let Some(selected) = selected_result {
                    // Check if server has HTTP endpoint
                    if selected.server_info.endpoint.is_empty() || !selected.server_info.endpoint.starts_with("http") {
                        formatter.warning("Selected server does not have an HTTP endpoint. Cannot introspect capabilities.");
                        formatter.list_item("Only servers with HTTP/HTTPS endpoints can be introspected.");
                        return Ok(());
                    }
                    
                    // Check if server already exists in approved or pending queues
                    let queue = ApprovalQueue::new(".");
                    let pending_list = queue.list_pending().ok().unwrap_or_default();
                    let approved_list = queue.list_approved().ok().unwrap_or_default();
                    
                    let existing_pending = pending_list.iter()
                        .find(|s| s.server_info.endpoint == selected.server_info.endpoint);
                    let existing_approved = approved_list.iter()
                        .find(|s| s.server_info.endpoint == selected.server_info.endpoint);
                    
                    if let Some(existing) = existing_approved {
                        formatter.warning(&format!("Server '{}' is already approved.", selected.server_info.name));
                        if let Some(ref files) = existing.capability_files {
                            if !files.is_empty() {
                                formatter.list_item(&format!("Existing capabilities: {} file(s)", files.len()));
                            }
                        }
                        formatter.list_item("You can add new capabilities to this server, or use 'ccos explore' to work with existing ones.");
                        print!("\nContinue with discovery to add new capabilities? (y/n): ");
                        io::stdout().flush().map_err(|e| RuntimeError::Generic(format!("Failed to flush stdout: {}", e)))?;
                        
                        let mut confirm = String::new();
                        io::stdin().read_line(&mut confirm).map_err(|e| RuntimeError::Generic(format!("Failed to read input: {}", e)))?;
                        let confirm = confirm.trim().to_lowercase();
                        
                        if confirm != "y" && confirm != "yes" {
                            formatter.list_item("Skipping discovery.");
                            return Ok(());
                        }
                    } else if let Some(existing) = existing_pending {
                        formatter.warning(&format!("Server '{}' is already in the pending queue.", selected.server_info.name));
                        formatter.list_item(&format!("Existing entry ID: {}", existing.id));
                        formatter.list_item("You can add new capabilities to this server, which will be merged with the existing entry.");
                        print!("\nContinue with discovery? (y/n): ");
                        io::stdout().flush().map_err(|e| RuntimeError::Generic(format!("Failed to flush stdout: {}", e)))?;
                        
                        let mut confirm = String::new();
                        io::stdin().read_line(&mut confirm).map_err(|e| RuntimeError::Generic(format!("Failed to read input: {}", e)))?;
                        let confirm = confirm.trim().to_lowercase();
                        
                        if confirm != "y" && confirm != "yes" {
                            formatter.list_item("Skipping discovery.");
                            return Ok(());
                        }
                    }
                    
                    // Detect server type: MCP or OpenAPI
                    let endpoint_lower = selected.server_info.endpoint.to_lowercase();
                    let is_mcp_server = endpoint_lower.contains("/mcp")
                        || endpoint_lower.contains("mcp://")
                        || endpoint_lower.contains("modelcontextprotocol")
                        || endpoint_lower.contains("smithery.ai")
                        || selected.server_info.name.contains("/mcp")
                        || matches!(selected.source, DiscoverySource::McpRegistry { .. })
                        || matches!(selected.source, DiscoverySource::LocalOverride { .. });
                    
                    let is_openapi_spec = endpoint_lower.contains("openapi")
                        || endpoint_lower.contains("swagger")
                        || endpoint_lower.ends_with(".json") && (endpoint_lower.contains("api") || endpoint_lower.contains("spec"))
                        || endpoint_lower.ends_with(".yaml") && (endpoint_lower.contains("api") || endpoint_lower.contains("spec"))
                        || endpoint_lower.ends_with(".yml") && (endpoint_lower.contains("api") || endpoint_lower.contains("spec"));
                    
                    if is_mcp_server {
                        // Handle MCP server introspection
                        ctx.status(&format!("Introspecting MCP capabilities from: {}", selected.server_info.name));
                        let discovery_service = MCPDiscoveryService::new();
                        
                        let server_config = MCPServerConfig {
                            name: selected.server_info.name.clone(),
                            endpoint: selected.server_info.endpoint.clone(),
                            auth_token: None,
                            timeout_seconds: 30,
                            protocol_version: "2024-11-05".to_string(),
                        };
                        
                        let options = DiscoveryOptions {
                            introspect_output_schemas: false,
                            use_cache: true,
                            register_in_marketplace: false,
                            export_to_rtfs: false,
                            export_directory: None,
                            auth_headers: None,
                            ..Default::default()
                        };
                        
                        match discovery_service.discover_tools(&server_config, &options).await {
                            Ok(tools) => {
                                formatter.section(&format!("MCP Capabilities from {}", selected.server_info.name));
                                if tools.is_empty() {
                                    formatter.warning("No capabilities found on this MCP server.");
                                } else {
                                    for (idx, tool) in tools.iter().enumerate() {
                                        formatter.kv("Capability", &format!("{}. {}", idx + 1, tool.tool_name));
                                        if let Some(ref desc) = tool.description {
                                            formatter.kv("Description", desc);
                                        }
                                        println!();
                                    }
                                    
                                    // Add to queue
                                    add_server_to_queue(selected, &formatter, ctx, tools.len(), None).await?;
                                }
                            }
                            Err(e) => {
                                formatter.warning(&format!("Failed to introspect MCP capabilities: {}", e));
                                formatter.list_item("The server may be unreachable or require authentication.");
                            }
                        }
                    } else if is_openapi_spec {
                        // Handle OpenAPI spec introspection
                        ctx.status(&format!("Introspecting OpenAPI spec from: {}", selected.server_info.endpoint));
                        let introspector = APIIntrospector::new();
                        
                        // Extract domain from server name or endpoint
                        let domain = extract_domain_from_endpoint(&selected.server_info.endpoint)
                            .unwrap_or_else(|| "api".to_string());
                        
                        match introspector.introspect_from_openapi(&selected.server_info.endpoint, &domain).await {
                            Ok(introspection) => {
                                formatter.section(&format!("OpenAPI Capabilities from {}", selected.server_info.name));
                                formatter.kv("API Title", &introspection.api_title);
                                formatter.kv("API Version", &introspection.api_version);
                                formatter.kv("Base URL", &introspection.base_url);
                                formatter.kv("Endpoints Found", &format!("{}", introspection.endpoints.len()));
                                println!();
                                
                                if introspection.endpoints.is_empty() {
                                    formatter.warning("No endpoints found in this OpenAPI spec.");
                                } else {
                                    formatter.list_item("Discovered endpoints:");
                                    for (idx, endpoint) in introspection.endpoints.iter().enumerate() {
                                        formatter.kv("Endpoint", &format!("{}. {} {} {}", idx + 1, endpoint.method, endpoint.path, endpoint.name));
                                        if !endpoint.description.is_empty() {
                                            formatter.kv("Description", &endpoint.description);
                                        }
                                        println!();
                                    }
                                    
                                    // Create capabilities from introspection
                                    match introspector.create_capabilities_from_introspection(&introspection) {
                                        Ok(capabilities) => {
                                            formatter.success(&format!("Created {} capabilities from OpenAPI spec.", capabilities.len()));
                                            
                                            // Generate and save RTFS capability files
                                            let pending_dir = std::path::Path::new("capabilities/servers/pending");
                                            std::fs::create_dir_all(pending_dir).map_err(|e| {
                                                RuntimeError::Generic(format!("Failed to create pending directory: {}", e))
                                            })?;
                                            
                                            let server_id = selected.server_info.name.to_lowercase().replace(" ", "_").replace("/", "_");
                                            let server_pending_dir = pending_dir.join(&server_id);
                                            std::fs::create_dir_all(&server_pending_dir).map_err(|e| {
                                                RuntimeError::Generic(format!("Failed to create server pending directory: {}", e))
                                            })?;
                                            
                                            let mut saved_files = Vec::new();
                                            for capability in &capabilities {
                                                let impl_code = introspector.generate_http_implementation(capability, &introspection);
                                                match introspector.save_capability_to_rtfs(capability, &impl_code, &server_pending_dir) {
                                                    Ok(file_path) => {
                                                        saved_files.push(file_path);
                                                    }
                                                    Err(e) => {
                                                        formatter.warning(&format!("Failed to save capability {}: {}", capability.id, e));
                                                    }
                                                }
                                            }
                                            
                                            if !saved_files.is_empty() {
                                                formatter.list_item(&format!("Saved {} capability file(s) to: {}", saved_files.len(), server_pending_dir.display()));
                                            }
                                            
                                            add_server_to_queue(selected, &formatter, ctx, capabilities.len(), Some(saved_files)).await?;
                                        }
                                        Err(e) => {
                                            formatter.warning(&format!("Failed to create capabilities: {}", e));
                                            // Still add to queue for manual import
                                            add_server_to_queue(selected, &formatter, ctx, introspection.endpoints.len(), None).await?;
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                formatter.warning(&format!("Failed to introspect OpenAPI spec: {}", e));
                                formatter.list_item("The spec may be invalid or unreachable.");
                                // Still offer to add to queue
                                print!("\nAdd this OpenAPI spec to approval queue for later import? (y/n): ");
                                io::stdout().flush().map_err(|e| RuntimeError::Generic(format!("Failed to flush stdout: {}", e)))?;
                                
                                let mut confirm = String::new();
                                io::stdin().read_line(&mut confirm).map_err(|e| RuntimeError::Generic(format!("Failed to read input: {}", e)))?;
                                let confirm = confirm.trim().to_lowercase();
                                
                                if confirm == "y" || confirm == "yes" {
                                    add_server_to_queue(selected, &formatter, ctx, 0, None).await?;
                                }
                            }
                        }
                    } else {
                        // Regular API - try to discover OpenAPI spec and introspect
                        ctx.status(&format!("Discovering API capabilities from: {}", selected.server_info.name));
                        
                        // Create introspector with optional LLM provider from arbiter config
                        let mut introspector = APIIntrospector::new();
                        if llm {
                            match ctx.create_llm_provider(llm_model.clone()).await {
                                Ok(provider) => {
                                    formatter.list_item("LLM fallback enabled via arbiter configuration");
                                    introspector.set_llm_provider(provider);
                                }
                                Err(e) => {
                                    formatter.warning(&format!("Could not enable LLM fallback: {}", e));
                                    formatter.list_item("Make sure llm_profiles are configured in agent_config.toml");
                                }
                            }
                        }
                        
                        // Extract base URL
                        let base_url = if let Ok(parsed) = url::Url::parse(&selected.server_info.endpoint) {
                            format!("{}://{}", parsed.scheme(), parsed.host_str().unwrap_or(""))
                        } else {
                            selected.server_info.endpoint.clone()
                        };
                        
                        // Extract domain for introspection
                        let domain = extract_domain_from_endpoint(&base_url)
                            .unwrap_or_else(|| "api".to_string());
                        
                        // Try to introspect via discovery (will try common OpenAPI spec locations)
                        match introspector.introspect_from_discovery(&base_url, &domain).await {
                            Ok(introspection) => {
                                formatter.section(&format!("API Capabilities from {}", selected.server_info.name));
                                formatter.kv("API Title", &introspection.api_title);
                                formatter.kv("API Version", &introspection.api_version);
                                formatter.kv("Base URL", &introspection.base_url);
                                formatter.kv("Endpoints Found", &format!("{}", introspection.endpoints.len()));
                                println!();
                                
                                if introspection.endpoints.is_empty() {
                                    formatter.warning("No endpoints discovered.");
                                } else {
                                    formatter.list_item("Discovered endpoints:");
                                    for (idx, endpoint) in introspection.endpoints.iter().enumerate() {
                                        formatter.kv("Endpoint", &format!("{}. {} {} {}", idx + 1, endpoint.method, endpoint.path, endpoint.name));
                                        if !endpoint.description.is_empty() {
                                            formatter.kv("Description", &endpoint.description);
                                        }
                                        println!();
                                    }
                                    
                                // Create capabilities from introspection
                                match introspector.create_capabilities_from_introspection(&introspection) {
                                    Ok(capabilities) => {
                                        formatter.success(&format!("Created {} capabilities from API discovery.", capabilities.len()));
                                        
                                        // Generate and save RTFS capability files
                                        let pending_dir = std::path::Path::new("capabilities/servers/pending");
                                        std::fs::create_dir_all(pending_dir).map_err(|e| {
                                            RuntimeError::Generic(format!("Failed to create pending directory: {}", e))
                                        })?;
                                        
                                        let server_id = selected.server_info.name.to_lowercase().replace(" ", "_").replace("/", "_");
                                        let server_pending_dir = pending_dir.join(&server_id);
                                        std::fs::create_dir_all(&server_pending_dir).map_err(|e| {
                                            RuntimeError::Generic(format!("Failed to create server pending directory: {}", e))
                                        })?;
                                        
                                        let mut saved_files = Vec::new();
                                        for capability in &capabilities {
                                            let impl_code = introspector.generate_http_implementation(capability, &introspection);
                                            match introspector.save_capability_to_rtfs(capability, &impl_code, &server_pending_dir) {
                                                Ok(file_path) => {
                                                    saved_files.push(file_path);
                                                }
                                                Err(e) => {
                                                    formatter.warning(&format!("Failed to save capability {}: {}", capability.id, e));
                                                }
                                            }
                                        }
                                        
                                        if !saved_files.is_empty() {
                                            formatter.list_item(&format!("Saved {} capability file(s) to: {}", saved_files.len(), server_pending_dir.display()));
                                        }
                                        
                                                    add_server_to_queue(selected, &formatter, ctx, capabilities.len(), None).await?;
                                    }
                                    Err(e) => {
                                        formatter.warning(&format!("Failed to create capabilities: {}", e));
                                        // Still add to queue
                                        add_server_to_queue(selected, &formatter, ctx, introspection.endpoints.len(), None).await?;
                                    }
                                }
                                }
                            }
                            Err(e) => {
                                formatter.warning(&format!("Failed to discover API capabilities: {}", e));
                                formatter.list_item("Could not find OpenAPI specification at common locations.");
                                
                                // If LLM is enabled, ask user for documentation URL(s)
                                if llm {
                                    println!();
                                    formatter.list_item("💡 You can provide documentation URLs for LLM parsing.");
                                    formatter.list_item("💡 You can add multiple APIs for the same provider by entering URLs one by one.");
                                    
                                    // Set up server directory once (before the loop)
                                    let pending_dir = std::path::Path::new("capabilities/servers/pending");
                                    std::fs::create_dir_all(pending_dir).map_err(|e| {
                                        RuntimeError::Generic(format!("Failed to create pending directory: {}", e))
                                    })?;
                                    
                                    let server_id = selected.server_info.name.to_lowercase().replace(" ", "_").replace("/", "_");
                                    let server_pending_dir = pending_dir.join(&server_id);
                                    std::fs::create_dir_all(&server_pending_dir).map_err(|e| {
                                        RuntimeError::Generic(format!("Failed to create server pending directory: {}", e))
                                    })?;
                                    
                                    let parser = crate::synthesis::introspection::llm_doc_parser::LlmDocParser::new();
                                    if let Some(llm_provider) = introspector.llm_provider() {
                                        let mut total_capabilities = 0;
                                        let mut total_saved_files = Vec::new();
                                        let mut has_successful_parse = false;
                                        
                                        // Loop to collect multiple API documentation URLs
                                        loop {
                                            println!();
                                            if total_capabilities == 0 {
                                                print!("Enter documentation URL (or press Enter to skip): ");
                                            } else {
                                                formatter.success(&format!("✓ Added {} capabilities so far ({} API module(s))", total_capabilities, total_saved_files.len()));
                                                print!("📋 Add another API documentation URL for this provider (or press Enter to finish): ");
                                            }
                                            io::stdout().flush().map_err(|e| RuntimeError::Generic(format!("Failed to flush stdout: {}", e)))?;
                                            
                                            let mut doc_url = String::new();
                                            io::stdin().read_line(&mut doc_url).map_err(|e| RuntimeError::Generic(format!("Failed to read input: {}", e)))?;
                                            let doc_url = doc_url.trim();
                                            
                                            if doc_url.is_empty() {
                                                break; // User is done adding URLs
                                            }
                                            
                                            ctx.status(&format!("Parsing documentation from: {}", doc_url));
                                            
                                            match parser.parse_from_url(doc_url, &domain, llm_provider.as_ref()).await {
                                                Ok(introspection) => {
                                                    formatter.section(&format!("API Capabilities from {}", selected.server_info.name));
                                                    formatter.kv("API Title", &introspection.api_title);
                                                    formatter.kv("API Version", &introspection.api_version);
                                                    formatter.kv("Base URL", &introspection.base_url);
                                                    formatter.kv("Endpoints Found", &format!("{}", introspection.endpoints.len()));
                                                    println!();
                                                    
                                                    if introspection.endpoints.is_empty() {
                                                        formatter.warning("No endpoints discovered from documentation.");
                                                        formatter.list_item("Skipping this URL, you can try another.");
                                                        continue;
                                                    } else {
                                                        formatter.list_item("Discovered endpoints:");
                                                        for (idx, endpoint) in introspection.endpoints.iter().enumerate() {
                                                            formatter.kv("Endpoint", &format!("{}. {} {} {}", idx + 1, endpoint.method, endpoint.path, endpoint.name));
                                                            if !endpoint.description.is_empty() {
                                                                formatter.kv("Description", &endpoint.description);
                                                            }
                                                            println!();
                                                        }
                                                        
                                                        // Create capabilities from introspection
                                                        match introspector.create_capabilities_from_introspection(&introspection) {
                                                            Ok(capabilities) => {
                                                                formatter.success(&format!("Created {} capabilities from documentation.", capabilities.len()));
                                                                
                                                                // Generate and save RTFS capability files
                                                                let mut saved_files = Vec::new();
                                                                for capability in &capabilities {
                                                                    let impl_code = introspector.generate_http_implementation(capability, &introspection);
                                                                    match introspector.save_capability_to_rtfs(capability, &impl_code, &server_pending_dir) {
                                                                        Ok(file_path) => {
                                                                            saved_files.push(file_path);
                                                                        }
                                                                        Err(e) => {
                                                                            formatter.warning(&format!("Failed to save capability {}: {}", capability.id, e));
                                                                        }
                                                                    }
                                                                }
                                                                
                                                                if !saved_files.is_empty() {
                                                                    formatter.list_item(&format!("Saved {} capability file(s) to: {}", saved_files.len(), server_pending_dir.display()));
                                                                    total_saved_files.extend(saved_files);
                                                                    total_capabilities += capabilities.len();
                                                                    has_successful_parse = true;
                                                                }
                                                            }
                                                            Err(e) => {
                                                                formatter.warning(&format!("Failed to create capabilities: {}", e));
                                                                formatter.list_item("Skipping this URL, you can try another.");
                                                            }
                                                        }
                                                    }
                                                }
                                                Err(e) => {
                                                    formatter.warning(&format!("Failed to parse documentation: {}", e));
                                                    formatter.list_item("The documentation may be in an unsupported format or unreachable.");
                                                    formatter.list_item("Skipping this URL, you can try another.");
                                                }
                                            }
                                        }
                                        
                                        // After the loop, add server to queue with total capability count
                                        if has_successful_parse {
                                            if total_saved_files.len() > 1 {
                                                formatter.success(&format!("✓ Server '{}' ready with {} capabilities from {} API modules", selected.server_info.name, total_capabilities, total_saved_files.len()));
                                            } else {
                                                formatter.success(&format!("✓ Server '{}' ready with {} capabilities", selected.server_info.name, total_capabilities));
                                            }
                                            add_server_to_queue(selected, &formatter, ctx, total_capabilities, Some(total_saved_files)).await?;
                                        } else {
                                            // No successful parses, offer to add to queue anyway
                                            println!();
                                            formatter.list_item("No capabilities were successfully parsed.");
                                            print!("Add this server to approval queue for later import? (y/n): ");
                                            io::stdout().flush().map_err(|e| RuntimeError::Generic(format!("Failed to flush stdout: {}", e)))?;
                                            
                                            let mut confirm = String::new();
                                            io::stdin().read_line(&mut confirm).map_err(|e| RuntimeError::Generic(format!("Failed to read input: {}", e)))?;
                                            let confirm = confirm.trim().to_lowercase();
                                            
                                            if confirm == "y" || confirm == "yes" {
                                                add_server_to_queue(selected, &formatter, ctx, 0, None).await?;
                                            }
                                        }
                                    } else {
                                        formatter.warning("LLM provider not available despite --llm flag.");
                                        // Still offer to add to queue
                                        print!("\nAdd this server to approval queue for later import? (y/n): ");
                                        io::stdout().flush().map_err(|e| RuntimeError::Generic(format!("Failed to flush stdout: {}", e)))?;
                                        
                                        let mut confirm = String::new();
                                        io::stdin().read_line(&mut confirm).map_err(|e| RuntimeError::Generic(format!("Failed to read input: {}", e)))?;
                                        let confirm = confirm.trim().to_lowercase();
                                        
                                        if confirm == "y" || confirm == "yes" {
                                            add_server_to_queue(selected, &formatter, ctx, 0, None).await?;
                                        }
                                    }
                                } else {
                                    formatter.list_item("Use --llm flag to enable LLM-based documentation parsing.");
                                    // Still offer to add to queue
                                    print!("\nAdd this server to approval queue for later import? (y/n): ");
                                    io::stdout().flush().map_err(|e| RuntimeError::Generic(format!("Failed to flush stdout: {}", e)))?;
                                    
                                    let mut confirm = String::new();
                                    io::stdin().read_line(&mut confirm).map_err(|e| RuntimeError::Generic(format!("Failed to read input: {}", e)))?;
                                    let confirm = confirm.trim().to_lowercase();
                                    
                                    if confirm == "y" || confirm == "yes" {
                                        add_server_to_queue(selected, &formatter, ctx, 0, None).await?;
                                    }
                                }
                            }
                        }
                    }
                } else {
                    // Interactive mode: prompt for selection if stdin is a TTY
                    if atty::is(atty::Stream::Stdin) {
                        print!("\nSelect a server (enter number or name, or 'q' to quit): ");
                        io::stdout().flush().map_err(|e| RuntimeError::Generic(format!("Failed to flush stdout: {}", e)))?;
                        
                        let mut input = String::new();
                        io::stdin().read_line(&mut input).map_err(|e| RuntimeError::Generic(format!("Failed to read input: {}", e)))?;
                        let input = input.trim();
                        
                        if input.is_empty() || input.eq_ignore_ascii_case("q") || input.eq_ignore_ascii_case("quit") {
                            formatter.list_item("Selection cancelled.");
                            return Ok(());
                        }
                        
                        // Try to parse as number first
                        let selected_result = if let Ok(idx) = input.parse::<usize>() {
                            if idx == 0 || idx > results.len() {
                                formatter.warning(&format!("Invalid index: {}. Must be between 1 and {}", idx, results.len()));
                                return Ok(());
                            }
                            Some(&results[idx - 1])
                        } else {
                            // Try to match by name
                            results.iter().find(|r| {
                                r.server_info.name.eq_ignore_ascii_case(input) || 
                                r.server_info.name.to_lowercase().contains(&input.to_lowercase())
                            })
                        };
                        
                        if let Some(selected) = selected_result {
                            // Check if server has HTTP endpoint
                            if selected.server_info.endpoint.is_empty() || !selected.server_info.endpoint.starts_with("http") {
                                formatter.warning("Selected server does not have an HTTP endpoint. Cannot introspect capabilities.");
                                formatter.list_item("Only servers with HTTP/HTTPS endpoints can be introspected.");
                                return Ok(());
                            }
                            
                            // Check if server already exists in approved or pending queues
                            let queue = ApprovalQueue::new(".");
                            let pending_list = queue.list_pending().ok().unwrap_or_default();
                            let approved_list = queue.list_approved().ok().unwrap_or_default();
                            
                            let existing_pending = pending_list.iter()
                                .find(|s| s.server_info.endpoint == selected.server_info.endpoint);
                            let existing_approved = approved_list.iter()
                                .find(|s| s.server_info.endpoint == selected.server_info.endpoint);
                            
                            if let Some(existing) = existing_approved {
                                formatter.warning(&format!("Server '{}' is already approved.", selected.server_info.name));
                                if let Some(ref files) = existing.capability_files {
                                    if !files.is_empty() {
                                        formatter.list_item(&format!("Existing capabilities: {} file(s)", files.len()));
                                    }
                                }
                                formatter.list_item("You can add new capabilities to this server, or use 'ccos explore' to work with existing ones.");
                                print!("\nContinue with discovery to add new capabilities? (y/n): ");
                                io::stdout().flush().map_err(|e| RuntimeError::Generic(format!("Failed to flush stdout: {}", e)))?;
                                
                                let mut confirm = String::new();
                                io::stdin().read_line(&mut confirm).map_err(|e| RuntimeError::Generic(format!("Failed to read input: {}", e)))?;
                                let confirm = confirm.trim().to_lowercase();
                                
                                if confirm != "y" && confirm != "yes" {
                                    formatter.list_item("Skipping discovery.");
                                    return Ok(());
                                }
                            } else if let Some(existing) = existing_pending {
                                formatter.warning(&format!("Server '{}' is already in the pending queue.", selected.server_info.name));
                                formatter.list_item(&format!("Existing entry ID: {}", existing.id));
                                formatter.list_item("You can add new capabilities to this server, which will be merged with the existing entry.");
                                print!("\nContinue with discovery? (y/n): ");
                                io::stdout().flush().map_err(|e| RuntimeError::Generic(format!("Failed to flush stdout: {}", e)))?;
                                
                                let mut confirm = String::new();
                                io::stdin().read_line(&mut confirm).map_err(|e| RuntimeError::Generic(format!("Failed to read input: {}", e)))?;
                                let confirm = confirm.trim().to_lowercase();
                                
                                if confirm != "y" && confirm != "yes" {
                                    formatter.list_item("Skipping discovery.");
                                    return Ok(());
                                }
                            }
                            
                            // Detect server type: MCP or OpenAPI/API
                            let endpoint_lower = selected.server_info.endpoint.to_lowercase();
                            let is_mcp_server = endpoint_lower.contains("/mcp")
                                || endpoint_lower.contains("mcp://")
                                || endpoint_lower.contains("modelcontextprotocol")
                                || endpoint_lower.contains("smithery.ai")
                                || selected.server_info.name.contains("/mcp")
                                || matches!(selected.source, DiscoverySource::McpRegistry { .. })
                                || matches!(selected.source, DiscoverySource::LocalOverride { .. });
                            
                            let is_openapi_spec = endpoint_lower.contains("openapi")
                                || endpoint_lower.contains("swagger")
                                || endpoint_lower.ends_with(".json") && (endpoint_lower.contains("api") || endpoint_lower.contains("spec"))
                                || endpoint_lower.ends_with(".yaml") && (endpoint_lower.contains("api") || endpoint_lower.contains("spec"))
                                || endpoint_lower.ends_with(".yml") && (endpoint_lower.contains("api") || endpoint_lower.contains("spec"));
                            
                            if is_mcp_server {
                                // Handle MCP server introspection
                                ctx.status(&format!("Introspecting MCP capabilities from: {}", selected.server_info.name));
                                let discovery_service = MCPDiscoveryService::new();
                                
                                let server_config = MCPServerConfig {
                                    name: selected.server_info.name.clone(),
                                    endpoint: selected.server_info.endpoint.clone(),
                                    auth_token: None,
                                    timeout_seconds: 30,
                                    protocol_version: "2024-11-05".to_string(),
                                };
                                
                                let options = DiscoveryOptions {
                                    introspect_output_schemas: false,
                                    use_cache: true,
                                    register_in_marketplace: false,
                                    export_to_rtfs: false,
                                    export_directory: None,
                                    auth_headers: None,
                                    ..Default::default()
                                };
                                
                                match discovery_service.discover_tools(&server_config, &options).await {
                                    Ok(tools) => {
                                        formatter.section(&format!("MCP Capabilities from {}", selected.server_info.name));
                                        if tools.is_empty() {
                                            formatter.warning("No capabilities found on this MCP server.");
                                        } else {
                                            for (idx, tool) in tools.iter().enumerate() {
                                                formatter.kv("Capability", &format!("{}. {}", idx + 1, tool.tool_name));
                                                if let Some(ref desc) = tool.description {
                                                    formatter.kv("Description", desc);
                                                }
                                                println!();
                                            }
                                            
                                            add_server_to_queue(selected, &formatter, ctx, tools.len(), None).await?;
                                        }
                                    }
                                    Err(e) => {
                                        formatter.warning(&format!("Failed to introspect MCP capabilities: {}", e));
                                        formatter.list_item("The server may be unreachable or require authentication.");
                                    }
                                }
                            } else if is_openapi_spec {
                                // Handle OpenAPI spec introspection
                                ctx.status(&format!("Introspecting OpenAPI spec from: {}", selected.server_info.endpoint));
                                let introspector = APIIntrospector::new();
                                
                                let domain = extract_domain_from_endpoint(&selected.server_info.endpoint)
                                    .unwrap_or_else(|| "api".to_string());
                                
                                match introspector.introspect_from_openapi(&selected.server_info.endpoint, &domain).await {
                                    Ok(introspection) => {
                                        formatter.section(&format!("OpenAPI Capabilities from {}", selected.server_info.name));
                                        formatter.kv("API Title", &introspection.api_title);
                                        formatter.kv("Endpoints Found", &format!("{}", introspection.endpoints.len()));
                                        println!();
                                        
                                        if !introspection.endpoints.is_empty() {
                                            for (idx, endpoint) in introspection.endpoints.iter().take(10).enumerate() {
                                                formatter.kv("Endpoint", &format!("{}. {} {} {}", idx + 1, endpoint.method, endpoint.path, endpoint.name));
                                            }
                                            if introspection.endpoints.len() > 10 {
                                                formatter.list_item(&format!("... and {} more endpoints", introspection.endpoints.len() - 10));
                                            }
                                            println!();
                                            
                                            match introspector.create_capabilities_from_introspection(&introspection) {
                                                Ok(capabilities) => {
                                                    formatter.success(&format!("Created {} capabilities.", capabilities.len()));
                                                    add_server_to_queue(selected, &formatter, ctx, capabilities.len(), None).await?;
                                                }
                                                Err(e) => {
                                                    formatter.warning(&format!("Failed to create capabilities: {}", e));
                                                    add_server_to_queue(selected, &formatter, ctx, introspection.endpoints.len(), None).await?;
                                                }
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        formatter.warning(&format!("Failed to introspect OpenAPI spec: {}", e));
                                    }
                                }
                            } else {
                                // Regular API - try to discover OpenAPI spec
                                ctx.status(&format!("Discovering API capabilities from: {}", selected.server_info.name));
                                
                                // Create introspector with optional LLM provider from arbiter config
                                let mut introspector = APIIntrospector::new();
                                if llm {
                                    match ctx.create_llm_provider(llm_model.clone()).await {
                                        Ok(provider) => {
                                            formatter.list_item("LLM fallback enabled via arbiter configuration");
                                            introspector.set_llm_provider(provider);
                                        }
                                        Err(e) => {
                                            formatter.warning(&format!("Could not enable LLM fallback: {}", e));
                                            formatter.list_item("Make sure llm_profiles are configured in agent_config.toml");
                                        }
                                    }
                                }
                                
                                let base_url = if let Ok(parsed) = url::Url::parse(&selected.server_info.endpoint) {
                                    format!("{}://{}", parsed.scheme(), parsed.host_str().unwrap_or(""))
                                } else {
                                    selected.server_info.endpoint.clone()
                                };
                                
                                let domain = extract_domain_from_endpoint(&base_url)
                                    .unwrap_or_else(|| "api".to_string());
                                
                                match introspector.introspect_from_discovery(&base_url, &domain).await {
                                    Ok(introspection) => {
                                        formatter.section(&format!("API Capabilities from {}", selected.server_info.name));
                                        formatter.kv("API Title", &introspection.api_title);
                                        formatter.kv("Endpoints Found", &format!("{}", introspection.endpoints.len()));
                                        println!();
                                        
                                        if !introspection.endpoints.is_empty() {
                                            for (idx, endpoint) in introspection.endpoints.iter().take(10).enumerate() {
                                                formatter.kv("Endpoint", &format!("{}. {} {} {}", idx + 1, endpoint.method, endpoint.path, endpoint.name));
                                            }
                                            if introspection.endpoints.len() > 10 {
                                                formatter.list_item(&format!("... and {} more endpoints", introspection.endpoints.len() - 10));
                                            }
                                            println!();
                                            
                                            match introspector.create_capabilities_from_introspection(&introspection) {
                                                Ok(capabilities) => {
                                                    formatter.success(&format!("Created {} capabilities.", capabilities.len()));
                                                    add_server_to_queue(selected, &formatter, ctx, capabilities.len(), None).await?;
                                                }
                                                Err(e) => {
                                                    formatter.warning(&format!("Failed to create capabilities: {}", e));
                                                    add_server_to_queue(selected, &formatter, ctx, introspection.endpoints.len(), None).await?;
                                                }
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        formatter.warning(&format!("Failed to discover API: {}", e));
                                        formatter.list_item("Could not find OpenAPI specification at common locations.");
                                        
                                        // If LLM is enabled, ask user for documentation URL(s)
                                        if llm {
                                            println!();
                                            formatter.list_item("💡 You can provide documentation URLs for LLM parsing.");
                                            formatter.list_item("💡 You can add multiple APIs for the same provider by entering URLs one by one.");
                                            
                                            // Set up server directory once (before the loop)
                                            let pending_dir = std::path::Path::new("capabilities/servers/pending");
                                            std::fs::create_dir_all(pending_dir).map_err(|e| {
                                                RuntimeError::Generic(format!("Failed to create pending directory: {}", e))
                                            })?;
                                            
                                            let server_id = selected.server_info.name.to_lowercase().replace(" ", "_").replace("/", "_");
                                            let server_pending_dir = pending_dir.join(&server_id);
                                            std::fs::create_dir_all(&server_pending_dir).map_err(|e| {
                                                RuntimeError::Generic(format!("Failed to create server pending directory: {}", e))
                                            })?;
                                            
                                            let parser = crate::synthesis::introspection::llm_doc_parser::LlmDocParser::new();
                                            if let Some(llm_provider) = introspector.llm_provider() {
                                                let mut total_capabilities = 0;
                                                let mut total_saved_files = Vec::new();
                                                let mut has_successful_parse = false;
                                                
                                                // Loop to collect multiple API documentation URLs
                                                loop {
                                                    println!();
                                                    if total_capabilities == 0 {
                                                        print!("Enter documentation URL (or press Enter to skip): ");
                                                    } else {
                                                        formatter.success(&format!("✓ Added {} capabilities so far ({} API module(s))", total_capabilities, total_saved_files.len()));
                                                        print!("📋 Add another API documentation URL for this provider (or press Enter to finish): ");
                                                    }
                                                    io::stdout().flush().map_err(|e| RuntimeError::Generic(format!("Failed to flush stdout: {}", e)))?;
                                                    
                                                    let mut doc_url = String::new();
                                                    io::stdin().read_line(&mut doc_url).map_err(|e| RuntimeError::Generic(format!("Failed to read input: {}", e)))?;
                                                    let doc_url = doc_url.trim();
                                                    
                                                    if doc_url.is_empty() {
                                                        break; // User is done adding URLs
                                                    }
                                                    
                                                    ctx.status(&format!("Parsing documentation from: {}", doc_url));
                                                    
                                                    match parser.parse_from_url(doc_url, &domain, llm_provider.as_ref()).await {
                                                        Ok(introspection) => {
                                                            formatter.section(&format!("API Capabilities from {}", selected.server_info.name));
                                                            formatter.kv("API Title", &introspection.api_title);
                                                            formatter.kv("API Version", &introspection.api_version);
                                                            formatter.kv("Base URL", &introspection.base_url);
                                                            formatter.kv("Endpoints Found", &format!("{}", introspection.endpoints.len()));
                                                            println!();
                                                            
                                                            if introspection.endpoints.is_empty() {
                                                                formatter.warning("No endpoints discovered from documentation.");
                                                                formatter.list_item("Skipping this URL, you can try another.");
                                                                continue;
                                                            } else {
                                                                formatter.list_item("Discovered endpoints:");
                                                                for (idx, endpoint) in introspection.endpoints.iter().enumerate() {
                                                                    formatter.kv("Endpoint", &format!("{}. {} {} {}", idx + 1, endpoint.method, endpoint.path, endpoint.name));
                                                                    if !endpoint.description.is_empty() {
                                                                        formatter.kv("Description", &endpoint.description);
                                                                    }
                                                                    println!();
                                                                }
                                                                
                                                                // Create capabilities from introspection
                                                                match introspector.create_capabilities_from_introspection(&introspection) {
                                                                    Ok(capabilities) => {
                                                                        formatter.success(&format!("Created {} capabilities from documentation.", capabilities.len()));
                                                                        
                                                                        // Generate and save RTFS capability files
                                                                        let mut saved_files = Vec::new();
                                                                        for capability in &capabilities {
                                                                            let impl_code = introspector.generate_http_implementation(capability, &introspection);
                                                                            match introspector.save_capability_to_rtfs(capability, &impl_code, &server_pending_dir) {
                                                                                Ok(file_path) => {
                                                                                    saved_files.push(file_path);
                                                                                }
                                                                                Err(e) => {
                                                                                    formatter.warning(&format!("Failed to save capability {}: {}", capability.id, e));
                                                                                }
                                                                            }
                                                                        }
                                                                        
                                                                        if !saved_files.is_empty() {
                                                                            formatter.list_item(&format!("Saved {} capability file(s) to: {}", saved_files.len(), server_pending_dir.display()));
                                                                            total_saved_files.extend(saved_files);
                                                                            total_capabilities += capabilities.len();
                                                                            has_successful_parse = true;
                                                                        }
                                                                    }
                                                                    Err(e) => {
                                                                        formatter.warning(&format!("Failed to create capabilities: {}", e));
                                                                        formatter.list_item("Skipping this URL, you can try another.");
                                                                    }
                                                                }
                                                            }
                                                        }
                                                        Err(e) => {
                                                            formatter.warning(&format!("Failed to parse documentation: {}", e));
                                                            formatter.list_item("The documentation may be in an unsupported format or unreachable.");
                                                            formatter.list_item("Skipping this URL, you can try another.");
                                                        }
                                                    }
                                                }
                                                
                                                // After the loop, add server to queue with total capability count
                                                if has_successful_parse {
                                                    if total_saved_files.len() > 1 {
                                                        formatter.success(&format!("✓ Server '{}' ready with {} capabilities from {} API modules", selected.server_info.name, total_capabilities, total_saved_files.len()));
                                                    } else {
                                                        formatter.success(&format!("✓ Server '{}' ready with {} capabilities", selected.server_info.name, total_capabilities));
                                                    }
                                                    add_server_to_queue(selected, &formatter, ctx, total_capabilities, Some(total_saved_files)).await?;
                                                } else {
                                                    // No successful parses, offer to add to queue anyway
                                                    println!();
                                                    formatter.list_item("No capabilities were successfully parsed.");
                                                    print!("Add this server to approval queue for later import? (y/n): ");
                                                    io::stdout().flush().map_err(|e| RuntimeError::Generic(format!("Failed to flush stdout: {}", e)))?;
                                                    
                                                    let mut confirm = String::new();
                                                    io::stdin().read_line(&mut confirm).map_err(|e| RuntimeError::Generic(format!("Failed to read input: {}", e)))?;
                                                    let confirm = confirm.trim().to_lowercase();
                                                    
                                                    if confirm == "y" || confirm == "yes" {
                                                        add_server_to_queue(selected, &formatter, ctx, 0, None).await?;
                                                    }
                                                }
                                            } else {
                                                formatter.warning("LLM provider not available despite --llm flag.");
                                                // Still offer to add to queue
                                                print!("\nAdd this server to approval queue for later import? (y/n): ");
                                                io::stdout().flush().map_err(|e| RuntimeError::Generic(format!("Failed to flush stdout: {}", e)))?;
                                                
                                                let mut confirm = String::new();
                                                io::stdin().read_line(&mut confirm).map_err(|e| RuntimeError::Generic(format!("Failed to read input: {}", e)))?;
                                                let confirm = confirm.trim().to_lowercase();
                                                
                                                if confirm == "y" || confirm == "yes" {
                                                    add_server_to_queue(selected, &formatter, ctx, 0, None).await?;
                                                }
                                            }
                                        } else {
                                            formatter.list_item("Use --llm flag to enable LLM-based documentation parsing.");
                                            // Still offer to add to queue
                                            print!("\nAdd this server to approval queue for later import? (y/n): ");
                                            io::stdout().flush().map_err(|e| RuntimeError::Generic(format!("Failed to flush stdout: {}", e)))?;
                                            
                                            let mut confirm = String::new();
                                            io::stdin().read_line(&mut confirm).map_err(|e| RuntimeError::Generic(format!("Failed to read input: {}", e)))?;
                                            let confirm = confirm.trim().to_lowercase();
                                            
                                            if confirm == "y" || confirm == "yes" {
                                                add_server_to_queue(selected, &formatter, ctx, 0, None).await?;
                                            }
                                        }
                                    }
                                }
                            }
                        } else {
                            formatter.warning(&format!("No server found matching '{}'", input));
                            formatter.list_item(&format!("Please enter a valid index (1-{}) or server name.", count));
                        }
                    } else {
                        // Non-interactive mode (stdin is not a TTY)
                        formatter.list_item(&format!("Found {} server(s).", count));
                        formatter.list_item("Use 'ccos server search <query> --select <index>' to select a server by index (1-based).");
                        formatter.list_item("Or use 'ccos server search <query> --select-by-name <name>' to select by name.");
                        formatter.list_item("Selected servers will have their capabilities introspected and can be added to the approval queue.");
                    }
                }
            }
        }
    }

    Ok(())
}

/// Extract domain from endpoint URL
fn extract_domain_from_endpoint(endpoint: &str) -> Option<String> {
    if let Ok(parsed) = url::Url::parse(endpoint) {
        if let Some(host) = parsed.host_str() {
            // Remove port if present
            let domain = host.split(':').next().unwrap_or(host);
            // Remove 'www.' prefix if present
            let domain = domain.strip_prefix("www.").unwrap_or(domain);
            return Some(domain.to_string());
        }
    }
    None
}

/// Add server to approval queue
async fn add_server_to_queue(
    selected: &crate::discovery::RegistrySearchResult,
    formatter: &OutputFormatter,
    ctx: &CliContext,
    capability_count: usize,
    saved_files: Option<Vec<std::path::PathBuf>>,
) -> RuntimeResult<()> {
    let queue = ApprovalQueue::new(".");
    
    // Check if server already exists in pending or approved
    let pending_list = queue.list_pending().ok().unwrap_or_default();
    let approved_list = queue.list_approved().ok().unwrap_or_default();
    
    let existing_pending = pending_list.iter()
        .find(|s| s.server_info.endpoint == selected.server_info.endpoint);
    let existing_approved = approved_list.iter()
        .find(|s| s.server_info.endpoint == selected.server_info.endpoint);
    
    if let Some(existing) = existing_approved {
        // Server is already approved - update it with new capabilities
        formatter.warning(&format!("Server '{}' is already approved.", selected.server_info.name));
        if let Some(ref files) = existing.capability_files {
            if !files.is_empty() {
                formatter.list_item(&format!("Existing capabilities: {} file(s)", files.len()));
            }
        }
        
        if let Some(ref new_files) = saved_files {
            if !new_files.is_empty() {
                ctx.status("Updating approved server with new capabilities...");
                
                // Move files from pending to approved directory
                let pending_dir = std::path::Path::new("capabilities/servers/pending");
                let approved_dir = std::path::Path::new("capabilities/servers/approved");
                std::fs::create_dir_all(approved_dir).map_err(|e| {
                    RuntimeError::Generic(format!("Failed to create approved directory: {}", e))
                })?;
                
                let server_id = selected.server_info.name.to_lowercase().replace(" ", "_").replace("/", "_");
                let approved_server_dir = approved_dir.join(&server_id);
                std::fs::create_dir_all(&approved_server_dir).map_err(|e| {
                    RuntimeError::Generic(format!("Failed to create approved server directory: {}", e))
                })?;
                
                let pending_server_dir = pending_dir.join(&server_id);
                
                let mut new_capability_files = Vec::new();
                
                // Move each file from pending to approved
                for file_path in new_files {
                    // Extract relative path from pending directory
                    if let Ok(rel_from_pending) = file_path.strip_prefix(&pending_server_dir) {
                        // Preserve directory structure in approved directory
                        let dest_path = approved_server_dir.join(rel_from_pending);
                        
                        // Create parent directories if needed
                        if let Some(parent) = dest_path.parent() {
                            std::fs::create_dir_all(parent).map_err(|e| {
                                RuntimeError::Generic(format!("Failed to create directory: {}", e))
                            })?;
                        }
                        
                        // Copy file to approved location
                        std::fs::copy(file_path, &dest_path).map_err(|e| {
                            RuntimeError::Generic(format!("Failed to copy file: {}", e))
                        })?;
                        
                        // Calculate relative path for capability_files (from approved root)
                        if let Ok(rel_path) = dest_path.strip_prefix("capabilities/servers/approved") {
                            // Remove leading slash if present
                            let rel_str = rel_path.to_string_lossy().to_string();
                            let rel_str = rel_str.strip_prefix('/').unwrap_or(&rel_str);
                            new_capability_files.push(rel_str.to_string());
                        }
                    } else {
                        // Fallback: try to extract just the filename and subdirectory
                        if let Some(file_name) = file_path.file_name() {
                            // Try to find subdirectory by checking parent
                            if let Some(parent) = file_path.parent() {
                                if let Some(parent_name) = parent.file_name() {
                                    let subdir = approved_server_dir.join(parent_name);
                                    std::fs::create_dir_all(&subdir).map_err(|e| {
                                        RuntimeError::Generic(format!("Failed to create subdirectory: {}", e))
                                    })?;
                                    let dest_path = subdir.join(file_name);
                                    
                                    std::fs::copy(file_path, &dest_path).map_err(|e| {
                                        RuntimeError::Generic(format!("Failed to copy file: {}", e))
                                    })?;
                                    
                                    if let Ok(rel_path) = dest_path.strip_prefix("capabilities/servers/approved") {
                                        let rel_str = rel_path.to_string_lossy().to_string();
                                        let rel_str = rel_str.strip_prefix('/').unwrap_or(&rel_str);
                                        new_capability_files.push(rel_str.to_string());
                                    }
                                }
                            }
                        }
                    }
                }
                
                // Update approved entry with new capability files
                match queue.add_capability_files_to_approved(&selected.server_info.endpoint, new_capability_files.clone()) {
                    Ok(_) => {
                        formatter.success(&format!("Added {} new capability file(s) to approved server.", new_capability_files.len()));
                        formatter.list_item(&format!("Server '{}' now has {} total capability file(s).", selected.server_info.name, 
                            existing.capability_files.as_ref().map(|f| f.len()).unwrap_or(0) + new_capability_files.len()));
                    }
                    Err(e) => {
                        formatter.warning(&format!("Failed to update approved server: {}", e));
                        formatter.list_item("Files were moved but entry was not updated. You may need to manually update approved.json");
                    }
                }
                
                return Ok(());
            }
        }
        
        // No new files to add
        formatter.list_item("No new capabilities to add.");
        return Ok(());
    }
    
    if let Some(existing) = existing_pending {
        // Server already in pending queue - ask to merge or replace
        formatter.warning(&format!("Server '{}' is already in the pending queue.", selected.server_info.name));
        formatter.list_item(&format!("Existing entry ID: {}", existing.id));
        
        println!();
        formatter.list_item("Options:");
        formatter.list_item("  • 'merge' or 'm' - Add new capabilities to existing entry (recommended)");
        formatter.list_item("  • 'replace' or 'r' - Replace existing entry with new one");
        formatter.list_item("  • 'n' or Enter - Cancel, keep existing entry");
        print!("\nHow do you want to proceed? (merge/replace/cancel): ");
        io::stdout().flush().map_err(|e| RuntimeError::Generic(format!("Failed to flush stdout: {}", e)))?;
        
        let mut choice = String::new();
        io::stdin().read_line(&mut choice).map_err(|e| RuntimeError::Generic(format!("Failed to read input: {}", e)))?;
        let choice = choice.trim().to_lowercase();
        
        match choice.as_str() {
            "merge" | "m" => {
                // Merge: Files are already saved, just acknowledge
                formatter.success(&format!("Added {} new capabilities to existing server entry.", capability_count));
                formatter.list_item(&format!("Use 'ccos approval approve {}' to approve the updated server.", existing.id));
                return Ok(());
            }
            "replace" | "r" => {
                // Replace: Remove old entry and files, then add new
                ctx.status("Replacing existing server entry...");
                
                // Remove old entry from pending
                match queue.remove_pending(&existing.id) {
                    Ok(Some(_)) => {
                        formatter.list_item("Removed existing entry from pending queue.");
                    }
                    Ok(None) => {
                        formatter.warning("Existing entry not found in queue (may have been removed).");
                    }
                    Err(e) => {
                        formatter.warning(&format!("Failed to remove existing entry: {}", e));
                        formatter.list_item("Will add new entry anyway.");
                    }
                }
                
                // Optionally remove old files (user might want to keep them, but for replace we remove)
                let server_id = selected.server_info.name.to_lowercase().replace(" ", "_").replace("/", "_");
                let old_pending_dir = std::path::Path::new("capabilities/servers/pending").join(&server_id);
                if old_pending_dir.exists() {
                    if let Err(e) = std::fs::remove_dir_all(&old_pending_dir) {
                        formatter.warning(&format!("Could not remove old files: {}", e));
                        formatter.list_item("Old files may still exist, but new entry will be created.");
                    } else {
                        formatter.list_item("Removed old capability files.");
                    }
                }
                
                // Fall through to add new entry
            }
            _ => {
                formatter.list_item("Keeping existing entry. New capabilities were saved but not added to queue.");
                return Ok(());
            }
        }
    }
    
    // Ask for confirmation if not already handled above
    if existing_pending.is_none() && existing_approved.is_none() {
        print!("\nAdd this server to approval queue? (y/n): ");
        io::stdout().flush().map_err(|e| RuntimeError::Generic(format!("Failed to flush stdout: {}", e)))?;
        
        let mut confirm = String::new();
        io::stdin().read_line(&mut confirm).map_err(|e| RuntimeError::Generic(format!("Failed to read input: {}", e)))?;
        let confirm = confirm.trim().to_lowercase();
        
        if confirm != "y" && confirm != "yes" {
            formatter.list_item("Server not added to queue.");
            return Ok(());
        }
    }
    
    // Add new entry to queue
    ctx.status("Adding server to approval queue...");
    let discovery = PendingDiscovery {
        id: format!("search-{}", Uuid::new_v4()),
        source: selected.source.clone(),
        server_info: selected.server_info.clone(),
        domain_match: vec![],
        risk_assessment: RiskAssessment {
            level: RiskLevel::Low,
            reasons: vec!["discovered_via_search".to_string()],
        },
        requested_at: Utc::now(),
        expires_at: Utc::now() + chrono::Duration::hours(24 * 30),
        requesting_goal: None,
    };
    
    match queue.add(discovery.clone()) {
        Ok(_) => {
            formatter.success(&format!("Added '{}' to approval queue.", selected.server_info.name));
            if capability_count > 0 {
                formatter.list_item(&format!("Found {} capabilities. Use 'ccos approval approve {}' to approve it.", capability_count, discovery.id));
            } else {
                formatter.list_item(&format!("Use 'ccos approval approve {}' to approve it.", discovery.id));
            }
        }
        Err(e) => {
            formatter.warning(&format!("Failed to add server to queue: {}", e));
        }
    }

    Ok(())
}

