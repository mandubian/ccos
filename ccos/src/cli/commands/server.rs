use crate::cli::CliContext;
use crate::cli::OutputFormatter;
use crate::discovery::{ApprovalQueue, DiscoverySource, PendingDiscovery, RegistrySearcher, RiskAssessment, RiskLevel, ServerInfo};
use crate::mcp::core::MCPDiscoveryService;
use crate::capability_marketplace::mcp_discovery::MCPServerConfig;
use crate::mcp::types::DiscoveryOptions;
use crate::synthesis::introspection::api_introspector::APIIntrospector;
use crate::synthesis::runtime::web_search_discovery::WebSearchDiscovery;
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
        /// Uses OPENAI_API_KEY or ANTHROPIC_API_KEY environment variables
        #[arg(long)]
        llm: bool,

        /// LLM model to use (default: gpt-4o-mini for OpenAI, claude-3-haiku-20240307 for Anthropic)
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
                                    add_server_to_queue(selected, &formatter, ctx, tools.len()).await?;
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
                                            add_server_to_queue(selected, &formatter, ctx, capabilities.len()).await?;
                                        }
                                        Err(e) => {
                                            formatter.warning(&format!("Failed to create capabilities: {}", e));
                                            // Still add to queue for manual import
                                            add_server_to_queue(selected, &formatter, ctx, introspection.endpoints.len()).await?;
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
                                    add_server_to_queue(selected, &formatter, ctx, 0).await?;
                                }
                            }
                        }
                    } else {
                        // Regular API - try to discover OpenAPI spec and introspect
                        ctx.status(&format!("Discovering API capabilities from: {}", selected.server_info.name));
                        
                        // Create introspector with optional LLM provider from config
                        let mut introspector = APIIntrospector::new();
                        if llm {
                            match ctx.create_llm_provider(llm_model.clone()).await {
                                Ok(provider) => {
                                    formatter.list_item(&format!("LLM fallback enabled via arbiter configuration"));
                                    introspector.set_llm_provider(provider);
                                }
                                Err(e) => {
                                    formatter.warning(&format!("Could not enable LLM fallback: {}", e));
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
                                            add_server_to_queue(selected, &formatter, ctx, capabilities.len()).await?;
                                        }
                                        Err(e) => {
                                            formatter.warning(&format!("Failed to create capabilities: {}", e));
                                            // Still add to queue
                                            add_server_to_queue(selected, &formatter, ctx, introspection.endpoints.len()).await?;
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                formatter.warning(&format!("Failed to discover API capabilities: {}", e));
                                formatter.list_item("Could not find OpenAPI specification at common locations.");
                                
                                // Try web search to find OpenAPI spec or API documentation
                                ctx.status("Searching web for OpenAPI specification...");
                                let mut web_searcher = WebSearchDiscovery::new("auto".to_string());
                                
                                // Build search query from domain and server name
                                let search_query = format!("{} {} OpenAPI spec", domain, selected.server_info.name);
                                
                                match web_searcher.search_for_api_specs(&search_query).await {
                                    Ok(search_results) => {
                                        if !search_results.is_empty() {
                                            formatter.list_item(&format!("Found {} potential OpenAPI spec(s) via web search:", search_results.len()));
                                            println!();
                                            
                                            // Show top results
                                            for (idx, result) in search_results.iter().take(5).enumerate() {
                                                formatter.kv("Result", &format!("{}. {}", idx + 1, result.title));
                                                formatter.kv("URL", &result.url);
                                                formatter.kv("Type", &result.result_type);
                                                println!();
                                            }
                                            
                                            // Try to use the first OpenAPI spec result
                                            let openapi_result = search_results.iter()
                                                .find(|r| r.result_type == "openapi_spec" || r.url.ends_with(".json") || r.url.ends_with(".yaml"));
                                            
                                            if let Some(result) = openapi_result {
                                                ctx.status(&format!("Trying OpenAPI spec from: {}", result.url));
                                                match introspector.introspect_from_openapi(&result.url, &domain).await {
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
                                                                    formatter.success(&format!("Created {} capabilities from web-discovered OpenAPI spec.", capabilities.len()));
                                                                    add_server_to_queue(selected, &formatter, ctx, capabilities.len()).await?;
                                                                }
                                                                Err(e) => {
                                                                    formatter.warning(&format!("Failed to create capabilities: {}", e));
                                                                }
                                                            }
                                                        }
                                                    }
                                                    Err(e) => {
                                                        formatter.warning(&format!("Failed to introspect from web-discovered URL: {}", e));
                                                        // Prompt for manual spec URL (inline)
                                                        formatter.list_item("Common OpenAPI spec locations to try:");
                                                        let common_spec_paths = vec![
                                                            format!("{}/openapi.json", base_url),
                                                            format!("{}/swagger.json", base_url),
                                                            format!("{}/api/openapi.json", base_url),
                                                            format!("{}/api/swagger.json", base_url),
                                                            format!("{}/v1/openapi.json", base_url),
                                                            format!("{}/docs/openapi.json", base_url),
                                                        ];
                                                        for path in &common_spec_paths {
                                                            formatter.list_item(&format!("  - {}", path));
                                                        }
                                                        println!();
                                                        
                                                        formatter.list_item("You can manually provide an OpenAPI spec URL:");
                                                        print!("Enter OpenAPI spec URL (or press Enter to skip): ");
                                                        io::stdout().flush().map_err(|e| RuntimeError::Generic(format!("Failed to flush stdout: {}", e)))?;
                                                        
                                                        let mut spec_url = String::new();
                                                        io::stdin().read_line(&mut spec_url).map_err(|e| RuntimeError::Generic(format!("Failed to read input: {}", e)))?;
                                                        let spec_url = spec_url.trim();
                                                        
                                                        if !spec_url.is_empty() {
                                                            ctx.status(&format!("Introspecting from provided OpenAPI spec: {}", spec_url));
                                                            match introspector.introspect_from_openapi(spec_url, &domain).await {
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
                                                                                add_server_to_queue(selected, &formatter, ctx, capabilities.len()).await?;
                                                                            }
                                                                            Err(e) => {
                                                                                formatter.warning(&format!("Failed to create capabilities: {}", e));
                                                                            }
                                                                        }
                                                                    }
                                                                }
                                                                Err(e) => {
                                                                    formatter.warning(&format!("Failed to introspect from provided URL: {}", e));
                                                                }
                                                            }
                                                        } else {
                                                            formatter.list_item("Skipped. You can add this server manually with 'ccos server add <url>' if you find the OpenAPI spec URL later.");
                                                        }
                                                    }
                                                }
                                            } else {
                                                formatter.list_item("No OpenAPI spec found in search results.");
                                                formatter.list_item("You can manually provide an OpenAPI spec URL with 'ccos server search <spec-url>'.");
                                            }
                                        } else {
                                            formatter.list_item("No OpenAPI specs found via web search.");
                                            formatter.list_item("You can manually provide an OpenAPI spec URL with 'ccos server search <spec-url>'.");
                                        }
                                    }
                                    Err(e) => {
                                        formatter.warning(&format!("Web search failed: {}", e));
                                        formatter.list_item("You can manually provide an OpenAPI spec URL with 'ccos server search <spec-url>'.");
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
                                            
                                            add_server_to_queue(selected, &formatter, ctx, tools.len()).await?;
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
                                                    add_server_to_queue(selected, &formatter, ctx, capabilities.len()).await?;
                                                }
                                                Err(e) => {
                                                    formatter.warning(&format!("Failed to create capabilities: {}", e));
                                                    add_server_to_queue(selected, &formatter, ctx, introspection.endpoints.len()).await?;
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
                                let introspector = APIIntrospector::new();
                                
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
                                                    add_server_to_queue(selected, &formatter, ctx, capabilities.len()).await?;
                                                }
                                                Err(e) => {
                                                    formatter.warning(&format!("Failed to create capabilities: {}", e));
                                                    add_server_to_queue(selected, &formatter, ctx, introspection.endpoints.len()).await?;
                                                }
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        formatter.warning(&format!("Failed to discover API: {}", e));
                                        
                                        // Try web search to find OpenAPI spec or API documentation
                                        ctx.status("Searching web for OpenAPI specification...");
                                        let mut web_searcher = WebSearchDiscovery::new("auto".to_string());
                                        
                                        // Build search query from domain and server name
                                        let search_query = format!("{} {} OpenAPI spec", domain, selected.server_info.name);
                                        
                                        match web_searcher.search_for_api_specs(&search_query).await {
                                            Ok(search_results) => {
                                                if !search_results.is_empty() {
                                                    formatter.list_item(&format!("Found {} potential OpenAPI spec(s) via web search:", search_results.len()));
                                                    println!();
                                                    
                                                    // Show top results
                                                    for (idx, result) in search_results.iter().take(5).enumerate() {
                                                        formatter.kv("Result", &format!("{}. {}", idx + 1, result.title));
                                                        formatter.kv("URL", &result.url);
                                                        formatter.kv("Type", &result.result_type);
                                                        println!();
                                                    }
                                                    
                                                    // Try to use the first OpenAPI spec result
                                                    let openapi_result = search_results.iter()
                                                        .find(|r| r.result_type == "openapi_spec" || r.url.ends_with(".json") || r.url.ends_with(".yaml"));
                                                    
                                                    if let Some(result) = openapi_result {
                                                        ctx.status(&format!("Trying OpenAPI spec from: {}", result.url));
                                                        match introspector.introspect_from_openapi(&result.url, &domain).await {
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
                                                                            formatter.success(&format!("Created {} capabilities from web-discovered OpenAPI spec.", capabilities.len()));
                                                                            add_server_to_queue(selected, &formatter, ctx, capabilities.len()).await?;
                                                                        }
                                                                        Err(e) => {
                                                                            formatter.warning(&format!("Failed to create capabilities: {}", e));
                                                                        }
                                                                    }
                                                                }
                                                            }
                                                            Err(e) => {
                                                                formatter.warning(&format!("Failed to introspect from web-discovered URL: {}", e));
                                                                formatter.list_item("You can manually provide an OpenAPI spec URL with 'ccos server search <spec-url>'.");
                                                            }
                                                        }
                                                    } else {
                                                        formatter.list_item("No OpenAPI spec found in search results.");
                                                        formatter.list_item("You can manually provide an OpenAPI spec URL with 'ccos server search <spec-url>'.");
                                                    }
                                                } else {
                                                    formatter.list_item("No OpenAPI specs found via web search.");
                                                    formatter.list_item("You can manually provide an OpenAPI spec URL with 'ccos server search <spec-url>'.");
                                                }
                                            }
                                            Err(e) => {
                                                formatter.warning(&format!("Web search failed: {}", e));
                                                formatter.list_item("You can manually provide an OpenAPI spec URL with 'ccos server search <spec-url>'.");
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
) -> RuntimeResult<()> {
        print!("\nAdd this server to approval queue? (y/n): ");
        io::stdout().flush().map_err(|e| RuntimeError::Generic(format!("Failed to flush stdout: {}", e)))?;
        
        let mut confirm = String::new();
        io::stdin().read_line(&mut confirm).map_err(|e| RuntimeError::Generic(format!("Failed to read input: {}", e)))?;
        let confirm = confirm.trim().to_lowercase();
        
        if confirm == "y" || confirm == "yes" {
            ctx.status("Adding server to approval queue...");
            let queue = ApprovalQueue::new(".");
            
            // Check if already in queue
            let existing = queue.list_approved()
                .ok()
                .unwrap_or_default()
                .iter()
                .any(|s| s.server_info.endpoint == selected.server_info.endpoint);
            
            if existing {
                formatter.warning("Server is already in the approval queue.");
            } else {
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
            }
        } else {
            formatter.list_item("Server not added to queue.");
    }

    Ok(())
}

