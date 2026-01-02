// Turn processor: handles entity input and triggers system actions

use std::sync::{Arc, Mutex};

use super::types::*;
use crate::approval::queue::{DiscoverySource, ServerInfo};
use crate::arbiter::llm_provider::LlmProvider;
use crate::capability_marketplace::types::CapabilityMarketplace;
use crate::discovery::RegistrySearchResult;
use crate::intent_graph::IntentGraph;
use crate::mcp::core::MCPDiscoveryService;
use crate::mcp::types::DiscoveryOptions;
use crate::planner::modular_planner::PlanResult;
use crate::synthesis::introspection::api_introspector::APIIntrospector;
use crate::synthesis::introspection::LlmDocParser;

/// Processes entity input and triggers appropriate actions
pub struct TurnProcessor {
    /// Capability marketplace for resolution
    marketplace: Arc<CapabilityMarketplace>,
    /// Intent graph being constructed
    #[allow(dead_code)]
    intent_graph: Arc<Mutex<IntentGraph>>,
    /// MCP discovery service for finding servers
    discovery_service: Arc<MCPDiscoveryService>,
    /// Last discovery results for 'details' and 'connect' commands
    last_discovery_results: Arc<Mutex<Vec<RegistrySearchResult>>>,
    /// LLM provider for doc exploration and other tasks
    llm_provider: Option<Arc<dyn LlmProvider>>,
}

impl TurnProcessor {
    pub fn new(
        marketplace: Arc<CapabilityMarketplace>,
        intent_graph: Arc<Mutex<IntentGraph>>,
    ) -> Self {
        // Create discovery service with marketplace for auto-registration
        let discovery_service = MCPDiscoveryService::new().with_marketplace(marketplace.clone());

        Self {
            marketplace,
            intent_graph,
            discovery_service: Arc::new(discovery_service),
            last_discovery_results: Arc::new(Mutex::new(Vec::new())),
            llm_provider: None,
        }
    }

    /// Set LLM provider
    pub fn with_llm_provider(mut self, provider: Arc<dyn LlmProvider>) -> Self {
        self.llm_provider = Some(provider);
        self
    }

    /// Create with custom discovery service
    pub fn with_discovery_service(
        marketplace: Arc<CapabilityMarketplace>,
        intent_graph: Arc<Mutex<IntentGraph>>,
        discovery_service: Arc<MCPDiscoveryService>,
    ) -> Self {
        Self {
            marketplace,
            intent_graph,
            discovery_service,
            last_discovery_results: Arc::new(Mutex::new(Vec::new())),
            llm_provider: None,
        }
    }

    /// Process the entity's input intent and return actions + next message
    #[allow(unused_variables)]
    pub async fn process(
        &self,
        intent: &InputIntent,
        current_goal: &str,
        analysis: &GoalAnalysis,
        config: &DialogueConfig,
    ) -> Result<ProcessingResult, ProcessingError> {
        let mut actions = Vec::new();
        let mut next_message = None;
        let mut completed_plan = None;
        let mut should_continue = true;

        match intent {
            InputIntent::RefineGoal { new_goal } => {
                // Update the goal and re-analyze
                actions.push(TurnAction::IntentRefined {
                    intent_id: "root".to_string(),
                    old_description: current_goal.to_string(),
                    new_description: new_goal.clone(),
                });

                // We'll need to re-analyze with the new goal
                next_message = Some(format!(
                    "Got it! Updating goal to: \"{}\"\n\
                     Let me analyze what's needed...",
                    new_goal
                ));
            }

            InputIntent::Discover { domain } => {
                // Search for servers in this domain
                let (discovered, message) = self.discover_domain(domain).await?;

                for server in &discovered {
                    actions.push(TurnAction::ServersDiscovered {
                        domain: domain.clone(),
                        servers: vec![server.clone()],
                    });
                }

                next_message = Some(message);
            }

            InputIntent::ConnectServer { server_id } => {
                // Check if server_id is a number (index into last discovery results)
                let (resolved_id, server_info) = if let Ok(index) = server_id.parse::<usize>() {
                    let results = self.last_discovery_results.lock().unwrap();
                    if results.is_empty() {
                        return Ok(ProcessingResult {
                            actions: vec![],
                            next_message: Some(
                                "❌ No discovery results available.\n\n\
                                 Run 'discover <domain>' first, then 'connect <N>' to connect."
                                    .to_string(),
                            ),
                            completed_plan: None,
                            should_continue: true,
                            abandon_reason: None,
                        });
                    } else if index == 0 || index > results.len() {
                        return Ok(ProcessingResult {
                            actions: vec![],
                            next_message: Some(format!(
                                "❌ Invalid index: {}. Valid range is 1-{}.",
                                index,
                                results.len()
                            )),
                            completed_plan: None,
                            should_continue: true,
                            abandon_reason: None,
                        });
                    } else {
                        let result = &results[index - 1];
                        (result.server_info.name.clone(), Some(result.clone()))
                    }
                } else {
                    (server_id.clone(), None)
                };

                // Attempt to connect to the resolved server
                let (connected, message) = self
                    .connect_server_with_info(&resolved_id, server_info.as_ref())
                    .await?;

                if connected {
                    actions.push(TurnAction::ServerConnected {
                        server_id: resolved_id.clone(),
                        server_name: resolved_id.clone(),
                        capabilities_count: 0,
                    });
                }

                next_message = Some(message);
            }

            InputIntent::Synthesize { description } => {
                // Synthesis stub - will be implemented later
                let (result, message) = self.synthesize_capability(description).await?;

                if let Some(cap_id) = result {
                    actions.push(TurnAction::CapabilitySynthesized {
                        capability_id: cap_id,
                        description: description.clone(),
                        safety_status: "pending_review".to_string(),
                    });
                }

                next_message = Some(message);
            }

            InputIntent::Approval {
                request_id,
                approved,
            } => {
                actions.push(TurnAction::ApprovalDecided {
                    request_id: request_id.clone(),
                    approved: *approved,
                });

                next_message = Some(if *approved {
                    format!("Approved: {}. Proceeding...", request_id)
                } else {
                    format!(
                        "Rejected: {}. What would you like to do instead?",
                        request_id
                    )
                });
            }

            InputIntent::SelectOption { option_id } => {
                // Handle option selection based on current suggestions
                let (action, message) = self.handle_option_selection(option_id, analysis).await?;

                if let Some(a) = action {
                    actions.push(a);
                }

                next_message = Some(message);
            }

            InputIntent::Proceed => {
                // Try to generate a complete plan
                match self.try_generate_plan(current_goal, analysis).await {
                    Ok(plan) => {
                        actions.push(TurnAction::PlanFragmentGenerated {
                            rtfs_preview: plan.rtfs_plan.chars().take(200).collect(),
                            intent_ids_covered: plan.intent_ids.clone(),
                        });

                        completed_plan = Some(CompletedPlan {
                            rtfs_plan: plan.rtfs_plan.clone(),
                            intent_ids: plan.intent_ids.clone(),
                            plan_id: plan.plan_id.clone(),
                            conversation_summary: "Plan generated through dialogue".to_string(),
                        });

                        next_message =
                            Some("Plan generated successfully! Ready for execution.".to_string());
                        should_continue = false;
                    }
                    Err(e) => {
                        next_message = Some(format!(
                            "Cannot proceed yet: {}\n\
                             Missing capabilities need to be resolved first.",
                            e
                        ));
                    }
                }
            }

            InputIntent::Abandon { reason } => {
                should_continue = false;
                next_message = Some(format!(
                    "Dialogue ended. {}",
                    reason.as_deref().unwrap_or("No reason given.")
                ));

                return Ok(ProcessingResult {
                    actions,
                    next_message,
                    completed_plan: None,
                    should_continue: false,
                    abandon_reason: reason.clone(),
                });
            }

            InputIntent::Question { text } => {
                // Entity is asking a question - we need to answer
                let answer = self.answer_question(text, current_goal, analysis).await?;
                next_message = Some(answer);
            }

            InputIntent::ProvideInfo { key, value } => {
                // Entity provided additional information
                next_message = Some(format!("Noted: {} = {}. Updating analysis...", key, value));
            }

            InputIntent::Unclear { raw_input } => {
                next_message = Some(format!(
                    "I didn't understand: \"{}\"\n\n\
                     You can:\n\
                     - Type a number to select an option\n\
                     - Say 'discover <domain>' to find capabilities\n\
                     - Say 'connect <server>' to connect a server\n\
                     - Say 'details <N>' to see full info for result N\n\
                     - Say 'more' to see all results\n\
                     - Say 'proceed' to generate the plan\n\
                     - Say 'quit' to exit",
                    raw_input
                ));
            }

            InputIntent::Details { index } => {
                // Details command - show full server info using stored results
                let results = self.last_discovery_results.lock().unwrap();
                if results.is_empty() {
                    next_message = Some(
                        "📋 No discovery results available.\n\n\
                         Run 'discover <domain>' first, then 'details <N>' to see full info."
                            .to_string(),
                    );
                } else if *index == 0 || *index > results.len() {
                    next_message = Some(format!(
                        "❌ Invalid index: {}. Valid range is 1-{}.\n\n\
                         Use 'details <N>' where N is a result number.",
                        index,
                        results.len()
                    ));
                } else {
                    let result = &results[*index - 1];
                    let presenter = super::presenter::DialoguePresenter::new();
                    next_message = Some(presenter.format_server_details(result));
                }
            }

            InputIntent::ShowMore => {
                // Show more results - display all results
                let results = self.last_discovery_results.lock().unwrap();
                if results.is_empty() {
                    next_message = Some(
                        "📋 No discovery results to show.\n\n\
                         Run 'discover <domain>' first to search for servers."
                            .to_string(),
                    );
                } else {
                    let presenter = super::presenter::DialoguePresenter::new();
                    // format_discovery_results with show_all=true shows all results
                    next_message =
                        Some(presenter.format_discovery_results("results", &results, true));
                }
            }

            InputIntent::Back => {
                // Go back to previous view
                next_message = Some(
                    "◀️ Back to main view.\n\n\
                     What would you like to do next?"
                        .to_string(),
                );
            }

            InputIntent::Explore { index } => {
                // Explore documentation to find API links using LLM
                let results = self.last_discovery_results.lock().unwrap();
                if results.is_empty() {
                    next_message = Some(
                        "📋 No discovery results available.\n\n\
                         Run 'discover <domain>' first, then 'explore <N>' to find API links."
                            .to_string(),
                    );
                } else if *index == 0 || *index > results.len() {
                    next_message = Some(format!(
                        "❌ Invalid index: {}. Valid range is 1-{}.",
                        index,
                        results.len()
                    ));
                } else {
                    let result = &results[*index - 1];
                    let doc_url = result.server_info.endpoint.clone();
                    drop(results); // Release lock before async operation

                    if let Some(llm_provider) = &self.llm_provider {
                        // Present initial message
                        println!("🔍 Exploring documentation at: {}", doc_url);
                        println!(
                            "⏳ Analyzing page content for API links... This may take a moment."
                        );

                        // Run the parser
                        let parser = LlmDocParser::new();
                        match parser
                            .explore_documentation(&doc_url, llm_provider.as_ref())
                            .await
                        {
                            Ok(discovery) => {
                                let mut msg = format!("✅ Analysis complete for: {}\n\n", doc_url);

                                if discovery.is_api_documentation {
                                    msg.push_str("found valid API documentation page.\n");

                                    // It's a doc page, try to parse endpoints directly
                                    if let Some(provider) = &self.llm_provider {
                                        println!("📖 Parsing endpoints from documentation page...");
                                        match parser
                                            .parse_documentation(&doc_url, provider.as_ref())
                                            .await
                                        {
                                            Ok(parse_result) => {
                                                if !parse_result.endpoints.is_empty() {
                                                    msg.push_str(&format!(
                                                        "\n✨ **Discovered {} Endpoints**:\n",
                                                        parse_result.endpoints.len()
                                                    ));

                                                    // Show first few endpoints
                                                    for (i, ep) in parse_result
                                                        .endpoints
                                                        .iter()
                                                        .take(5)
                                                        .enumerate()
                                                    {
                                                        msg.push_str(&format!(
                                                            "  - {} {} - {}\n",
                                                            ep.method, ep.path, ep.description
                                                        ));
                                                    }
                                                    if parse_result.endpoints.len() > 5 {
                                                        msg.push_str(&format!(
                                                            "  ... and {} more\n",
                                                            parse_result.endpoints.len() - 5
                                                        ));
                                                    }

                                                    // Add as a connectable result
                                                    new_results.push(RegistrySearchResult {
                                                        source: DiscoverySource::WebSearch { url: doc_url.clone() },
                                                        server_info: ServerInfo {
                                                            name: parse_result.api_name.clone(),
                                                            description: Some(format!("Parsed {} endpoints from documentation", parse_result.endpoints.len())),
                                                            endpoint: doc_url.clone(),
                                                            auth_env_var: parse_result.auth.as_ref().map(|a| a.env_var_suggestion.clone()),
                                                            capabilities_path: None, // We don't save extracted capabilities to file yet
                                                            alternative_endpoints: vec![parse_result.base_url.clone()],
                                                        },
                                                        match_score: 0.9,
                                                        alternative_endpoints: vec![],
                                                    });

                                                    additional_msg
                                                        .push_str("\n\n✨ **Endpoints Parsed!**\n");
                                                    additional_msg.push_str("Found API definition on page. You can `connect` to use this API (requires configuration).");
                                                }
                                            }
                                            Err(e) => {
                                                println!("⚠️ Failed to parse endpoints: {}", e);
                                            }
                                        }
                                    }
                                }

                                let mut new_results = Vec::new();
                                let mut additional_msg = String::new();

                                if !discovery.openapi_specs.is_empty() {
                                    msg.push_str("\n📜 **OpenAPI Specifications**:\n");
                                    let introspector = if let Some(provider) = &self.llm_provider {
                                        APIIntrospector::with_llm_provider(provider.clone())
                                    } else {
                                        APIIntrospector::new()
                                    };

                                    for spec_url in &discovery.openapi_specs {
                                        if !spec_url.is_empty() {
                                            msg.push_str(&format!("  - {}\n", spec_url));

                                            // Attempt introspection
                                            println!("🔍 Introspecting OpenAPI spec: {}", spec_url);
                                            match introspector
                                                .introspect_from_openapi(spec_url, "web-api")
                                                .await
                                            {
                                                Ok(api_result) => {
                                                    println!(
                                                        "✅ Successfully introspected: {}",
                                                        api_result.api_title
                                                    );

                                                    // Convert to RegistrySearchResult
                                                    new_results.push(RegistrySearchResult {
                                                        source: DiscoverySource::WebSearch {
                                                            url: spec_url.clone(),
                                                        },
                                                        server_info: ServerInfo {
                                                            name: api_result.api_title.clone(),
                                                            description: Some(format!(
                                                                "Discovered from OpenAPI spec: {}",
                                                                spec_url
                                                            )),
                                                            endpoint: spec_url.clone(), // Use spec URL as endpoint for now
                                                            auth_env_var: None,
                                                            capabilities_path: None,
                                                            alternative_endpoints: vec![api_result
                                                                .base_url
                                                                .clone()],
                                                        },
                                                        match_score: 1.0,
                                                        alternative_endpoints: vec![],
                                                    });
                                                }
                                                Err(e) => {
                                                    println!(
                                                        "⚠️ Failed to introspect {}: {}",
                                                        spec_url, e
                                                    );
                                                }
                                            }
                                        }
                                    }
                                }

                                if !new_results.is_empty() {
                                    // Update last discovery results with found specs
                                    let mut last_results =
                                        self.last_discovery_results.lock().unwrap();
                                    *last_results = new_results.clone();

                                    additional_msg
                                        .push_str("\n\n🎉 **Introspection Successful!**\n");
                                    additional_msg.push_str(
                                        "Found usable API specifications. You can now:\n",
                                    );
                                    for (i, res) in last_results.iter().enumerate() {
                                        additional_msg.push_str(&format!(
                                            "  [{}] Connect to {}\n",
                                            i + 1,
                                            res.server_info.name
                                        ));
                                    }
                                    additional_msg.push_str(
                                        "\nRun `connect <N>` to register these capabilities.",
                                    );
                                }

                                if !discovery.api_links.is_empty() {
                                    msg.push_str("\n🔗 **API Links**:\n");
                                    for link in &discovery.api_links {
                                        msg.push_str(&format!(
                                            "  - [{}]({}) - {}\n",
                                            link.label, link.url, link.api_type
                                        ));

                                        // Add links to results so they can be explored recursively
                                        new_results.push(RegistrySearchResult {
                                            source: DiscoverySource::WebSearch {
                                                url: link.url.clone(),
                                            },
                                            server_info: ServerInfo {
                                                name: link.label.clone(),
                                                description: Some(format!(
                                                    "Discovered {} link: {}",
                                                    link.api_type, link.url
                                                )),
                                                endpoint: link.url.clone(),
                                                auth_env_var: None,
                                                capabilities_path: None,
                                                alternative_endpoints: vec![],
                                            },
                                            match_score: 0.8, // Slightly lower score for raw links
                                            alternative_endpoints: vec![],
                                        });
                                    }
                                }

                                if !new_results.is_empty() {
                                    // Update last discovery results with found specs and links
                                    let mut last_results =
                                        self.last_discovery_results.lock().unwrap();
                                    *last_results = new_results.clone();

                                    additional_msg.push_str("\n\n🎉 **Discovery Updated!**\n");
                                    additional_msg.push_str("Found usable items. You can:\n");
                                    for (i, res) in last_results.iter().enumerate() {
                                        let action = if res
                                            .server_info
                                            .description
                                            .as_ref()
                                            .map_or(false, |d| d.contains("OpenAPI spec"))
                                        {
                                            "Connect to"
                                        } else {
                                            "Explore"
                                        };
                                        additional_msg.push_str(&format!(
                                            "  [{}] {} {}\n",
                                            i + 1,
                                            action,
                                            res.server_info.name
                                        ));
                                    }
                                    additional_msg.push_str("\nRun `connect <N>` to use APIs, or `explore <N>` to crawl documentation links.");
                                }

                                msg.push_str(&additional_msg);

                                if discovery.api_links.is_empty()
                                    && discovery.openapi_specs.is_empty()
                                {
                                    msg.push_str(
                                        "⚠️ No explicit API links or specs found on this page.",
                                    );
                                }

                                next_message = Some(msg);
                            }
                            Err(e) => {
                                next_message =
                                    Some(format!("❌ Failed to explore documentation: {}", e));
                            }
                        }
                    } else {
                        next_message = Some(
                            "❌ LLM provider not configured. Cannot explore documentation."
                                .to_string(),
                        );
                    }
                }
            }
        }

        Ok(ProcessingResult {
            actions,
            next_message,
            completed_plan,
            should_continue,
            abandon_reason: None,
        })
    }

    // -------------------------------------------------------------------------
    // Discovery Implementation
    // -------------------------------------------------------------------------

    async fn discover_domain(
        &self,
        domain: &str,
    ) -> Result<(Vec<DiscoveredServer>, String), ProcessingError> {
        log::info!(
            "🔍 Discovering servers for domain: {} (comprehensive search)",
            domain
        );

        // Use RegistrySearcher for comprehensive multi-source discovery
        let searcher = crate::discovery::RegistrySearcher::new();

        match searcher.search(domain).await {
            Ok(results) => {
                // Store results for 'details' and 'connect' commands
                if let Ok(mut stored) = self.last_discovery_results.lock() {
                    *stored = results.clone();
                }

                // Use presenter for clean, consistent formatting
                let presenter = super::presenter::DialoguePresenter::new();
                let message = presenter.format_discovery_results(domain, &results, false);

                // Convert RegistrySearchResult to DiscoveredServer for state tracking
                let servers: Vec<DiscoveredServer> = results
                    .iter()
                    .map(|result| {
                        let source_label = match &result.source {
                            crate::approval::queue::DiscoverySource::McpRegistry { name } => {
                                format!("MCP Registry: {}", name)
                            }
                            crate::approval::queue::DiscoverySource::ApisGuru { api_name } => {
                                format!("APIs.guru: {}", api_name)
                            }
                            crate::approval::queue::DiscoverySource::WebSearch { url } => {
                                format!("Web: {}", url.chars().take(50).collect::<String>())
                            }
                            crate::approval::queue::DiscoverySource::LocalOverride { path } => {
                                format!("Local: {}", path.split('/').last().unwrap_or(path))
                            }
                            _ => "Other".to_string(),
                        };

                        DiscoveredServer {
                            id: result.server_info.name.clone(),
                            name: result.server_info.name.clone(),
                            description: result
                                .server_info
                                .description
                                .clone()
                                .or(Some(source_label)),
                            capabilities_preview: vec![],
                        }
                    })
                    .collect();

                Ok((servers, message))
            }
            Err(e) => {
                log::warn!("Comprehensive discovery failed for '{}': {}", domain, e);
                // Fall back to local config discovery
                self.discover_domain_from_local_config(domain).await
            }
        }
    }

    async fn discover_domain_from_local_config(
        &self,
        domain: &str,
    ) -> Result<(Vec<DiscoveredServer>, String), ProcessingError> {
        // Check marketplace for existing capabilities in this domain
        let capabilities = self.marketplace.list_capabilities().await;

        // Filter by domain
        let matching: Vec<_> = capabilities
            .iter()
            .filter(|cap| {
                cap.domains
                    .iter()
                    .any(|d| d.to_lowercase().contains(&domain.to_lowercase()))
                    || cap.id.to_lowercase().contains(&domain.to_lowercase())
            })
            .collect();

        if matching.is_empty() {
            // Create placeholder for external discovery
            let message = format!(
                "No local servers found for '{}' domain.\n\n\
                 Options:\n\
                 [1] Search MCP Registry online for '{}' servers\n\
                 [2] Configure custom '{}' MCP server manually\n\
                 [3] Try a different domain\n\n\
                 What would you like to do?",
                domain, domain, domain
            );
            Ok((vec![], message))
        } else {
            // Group capabilities by server/source
            let mut servers_map: std::collections::HashMap<String, Vec<String>> =
                std::collections::HashMap::new();
            for cap in matching {
                let server_id = cap.id.split('.').take(2).collect::<Vec<_>>().join(".");
                servers_map
                    .entry(server_id)
                    .or_default()
                    .push(cap.name.clone());
            }

            let servers: Vec<DiscoveredServer> = servers_map
                .into_iter()
                .map(|(id, caps)| DiscoveredServer {
                    id: id.clone(),
                    name: id.clone(),
                    description: Some(format!("{} capabilities", caps.len())),
                    capabilities_preview: caps.into_iter().take(5).collect(),
                })
                .collect();

            let message = format!(
                "Found {} capability source(s) for '{}' domain:\n\n{}\n\n\
                 Type 'connect <number>' to use a server, or 'proceed' if ready.",
                servers.len(),
                domain,
                servers
                    .iter()
                    .enumerate()
                    .map(|(i, s)| format!(
                        "[{}] {} - {}\n    Capabilities: {:?}",
                        i + 1,
                        s.name,
                        s.description.as_deref().unwrap_or(""),
                        s.capabilities_preview
                    ))
                    .collect::<Vec<_>>()
                    .join("\n\n")
            );

            Ok((servers, message))
        }
    }

    // -------------------------------------------------------------------------
    // Server Connection Implementation
    // -------------------------------------------------------------------------

    async fn connect_server(&self, server_id: &str) -> Result<(bool, String), ProcessingError> {
        log::info!("🔌 Connecting to server: {}", server_id);

        // Try to find server config in local configuration
        let config_discovery =
            crate::capability_marketplace::config_mcp_discovery::LocalConfigMcpDiscovery::new();

        // Check if this server is configured (search by name)
        let all_configs = config_discovery.get_all_server_configs();
        let server_config = all_configs.into_iter().find(|c| c.name == server_id);

        match server_config {
            Some(server_config) => {
                // Discover tools from this server
                let options = DiscoveryOptions::default();
                match self
                    .discovery_service
                    .discover_tools(&server_config, &options)
                    .await
                {
                    Ok(tools) => {
                        let cap_count = tools.len();

                        // Register discovered tools
                        for tool in &tools {
                            let manifest = self
                                .discovery_service
                                .tool_to_manifest(tool, &server_config);
                            if let Err(e) =
                                self.discovery_service.register_capability(&manifest).await
                            {
                                log::warn!(
                                    "Failed to register capability {}: {}",
                                    tool.tool_name,
                                    e
                                );
                            }
                        }

                        let message = format!(
                            "✅ Connected to '{}'!\n\n\
                             Discovered {} capabilities:\n{}\n\n\
                             These capabilities are now available for planning.\n\
                             Type 'proceed' to generate a plan or explore other options.",
                            server_id,
                            cap_count,
                            tools
                                .iter()
                                .take(10)
                                .map(|t| format!(
                                    "  • {} - {}",
                                    t.tool_name,
                                    t.description.as_deref().unwrap_or("No description")
                                ))
                                .collect::<Vec<_>>()
                                .join("\n")
                        );

                        Ok((true, message))
                    }
                    Err(e) => {
                        let message = format!(
                            "❌ Failed to connect to '{}': {}\n\n\
                             Please check:\n\
                             - Is the server running?\n\
                             - Are credentials configured correctly?\n\
                             - Is the server URL correct?",
                            server_id, e
                        );
                        Ok((false, message))
                    }
                }
            }
            None => {
                // Server not configured - offer to configure it
                let message = format!(
                    "Server '{}' is not configured.\n\n\
                     To add this server:\n\
                     1. Add it to .mcp.json or mcp_servers.json\n\
                     2. Include the server URL and any required authentication\n\n\
                     Or try discovering from the registry:\n\
                     > discover {}\n",
                    server_id, server_id
                );
                Ok((false, message))
            }
        }
    }

    /// Connect to a server with optional pre-discovered server info (from web search)
    async fn connect_server_with_info(
        &self,
        server_id: &str,
        server_info: Option<&RegistrySearchResult>,
    ) -> Result<(bool, String), ProcessingError> {
        // First try local config connection
        let config_discovery =
            crate::capability_marketplace::config_mcp_discovery::LocalConfigMcpDiscovery::new();
        let all_configs = config_discovery.get_all_server_configs();
        let server_config = all_configs.into_iter().find(|c| c.name == server_id);

        if server_config.is_some() {
            // Server is locally configured, use standard connect
            return self.connect_server(server_id).await;
        }

        // Not locally configured - check if we have server info from web search
        if let Some(info) = server_info {
            let presenter = super::presenter::DialoguePresenter::new();
            let details = presenter.format_server_details(info);

            // For web search results, we can't directly connect but we can show how to
            let message = format!(
                "🔗 Connection info for '{}':\n\n{}\n\n\
                 📋 To integrate this service:\n\
                 1. Visit the URL above and sign up for API access\n\
                 2. Get your API key/credentials\n\
                 3. Add to .mcp.json or environment variables\n\
                 4. Re-run discovery to connect\n\n\
                 💡 For MCP servers, add to .mcp.json:\n\
                 {{\n\
                   \"servers\": {{\n\
                     \"{}\": {{\n\
                       \"url\": \"{}\",\n\
                       \"auth\": {{ \"type\": \"bearer\", \"env\": \"YOUR_API_KEY\" }}\n\
                     }}\n\
                   }}\n\
                 }}",
                server_id, details, server_id, info.server_info.endpoint
            );
            Ok((false, message))
        } else {
            // No info available
            let message = format!(
                "Server '{}' is not configured.\n\n\
                 To add this server:\n\
                 1. Add it to .mcp.json or mcp_servers.json\n\
                 2. Include the server URL and any required authentication\n\n\
                 Or try discovering from the registry:\n\
                 > discover {}",
                server_id, server_id
            );
            Ok((false, message))
        }
    }

    // -------------------------------------------------------------------------
    // Synthesis Implementation
    // -------------------------------------------------------------------------

    async fn synthesize_capability(
        &self,
        description: &str,
    ) -> Result<(Option<String>, String), ProcessingError> {
        log::info!("🔧 Synthesizing capability: {}", description);

        // Generate capability ID from description
        let cap_id = format!(
            "synthesized.{}",
            description
                .to_lowercase()
                .chars()
                .filter(|c| c.is_alphanumeric() || *c == ' ')
                .collect::<String>()
                .replace(' ', "_")
                .chars()
                .take(30)
                .collect::<String>()
        );

        // TODO: Integrate with actual synthesis pipeline
        // For now, queue for synthesis and return pending status
        let message = format!(
            "🔧 Synthesizing capability: '{}'\n\n\
             This will create an RTFS adapter that:\n\
             - Wraps the required functionality\n\
             - Includes input/output validation\n\
             - Undergoes safety review before use\n\n\
             ⏳ Capability ID: {}\n\
             📋 Status: Queued for synthesis\n\n\
             The capability will be available after synthesis completes.\n\
             In the meantime, would you like to:\n\
             [1] Continue with other capabilities\n\
             [2] Refine the synthesis description\n\
             [3] Wait for synthesis to complete",
            description, cap_id
        );

        Ok((Some(cap_id), message))
    }

    // -------------------------------------------------------------------------
    // Option Selection Handler
    // -------------------------------------------------------------------------

    async fn handle_option_selection(
        &self,
        option_id: &str,
        analysis: &GoalAnalysis,
    ) -> Result<(Option<TurnAction>, String), ProcessingError> {
        // Map option to suggestion
        let idx: usize =
            if option_id.len() == 1 && option_id.chars().next().unwrap().is_alphabetic() {
                // Letter selection: a=0, b=1, etc.
                (option_id.to_lowercase().chars().next().unwrap() as usize) - ('a' as usize)
            } else if let Ok(num) = option_id.parse::<usize>() {
                // Number selection: 1=0, 2=1, etc.
                num.saturating_sub(1)
            } else {
                return Ok((
                    None,
                    format!(
                        "Invalid option: '{}'. Please select a valid option.",
                        option_id
                    ),
                ));
            };

        if idx >= analysis.suggestions.len() {
            return Ok((
                None,
                format!(
                    "Option {} is out of range. Valid options: 1-{}",
                    option_id,
                    analysis.suggestions.len()
                ),
            ));
        }

        let suggestion = &analysis.suggestions[idx];

        match suggestion {
            Suggestion::Discover { domain, .. } => {
                let (servers, message) = self.discover_domain(domain).await?;
                let action = TurnAction::ServersDiscovered {
                    domain: domain.clone(),
                    servers,
                };
                Ok((Some(action), message))
            }
            Suggestion::ConnectServer {
                server_id,
                server_name,
                provides,
            } => {
                let (connected, message) = self.connect_server(server_id).await?;
                let action = if connected {
                    Some(TurnAction::ServerConnected {
                        server_id: server_id.clone(),
                        server_name: server_name.clone(),
                        capabilities_count: provides.len(),
                    })
                } else {
                    None
                };
                Ok((action, message))
            }
            Suggestion::Synthesize { description, .. } => {
                let (cap_id, message) = self.synthesize_capability(description).await?;
                let action = cap_id.map(|id| TurnAction::CapabilitySynthesized {
                    capability_id: id,
                    description: description.clone(),
                    safety_status: "pending".to_string(),
                });
                Ok((action, message))
            }
            Suggestion::RefineGoal {
                alternative,
                reason,
            } => {
                let message = format!(
                    "Refining goal to: \"{}\"\n\
                     Reason: {}\n\n\
                     Proceeding with this refined goal?",
                    alternative, reason
                );
                let action = TurnAction::IntentRefined {
                    intent_id: "root".to_string(),
                    old_description: analysis.goal.clone(),
                    new_description: alternative.clone(),
                };
                Ok((Some(action), message))
            }
        }
    }

    // -------------------------------------------------------------------------
    // Plan Generation
    // -------------------------------------------------------------------------

    async fn try_generate_plan(
        &self,
        goal: &str,
        analysis: &GoalAnalysis,
    ) -> Result<PlanResult, ProcessingError> {
        if analysis.feasibility < 0.5 {
            return Err(ProcessingError::InsufficientCapabilities {
                feasibility: analysis.feasibility,
                missing: analysis.missing_domains.clone(),
            });
        }

        // Note: ModularPlanner.plan() requires &mut self, but TurnProcessor holds Arc<ModularPlanner>
        // Plan generation is handled by DialoguePlanner.generate_plan() which uses its own planner instance
        // This stub signals that plan generation should proceed through the main dialogue flow
        log::info!("📝 Plan generation requested for goal: {}", goal);

        // Return a placeholder that signals the dialogue planner to generate the actual plan
        Err(ProcessingError::PlanningFailed(
            "Plan generation is handled by DialoguePlanner - use 'proceed' to generate the plan"
                .to_string(),
        ))
    }

    // -------------------------------------------------------------------------
    // Question Answering
    // -------------------------------------------------------------------------

    async fn answer_question(
        &self,
        question: &str,
        current_goal: &str,
        analysis: &GoalAnalysis,
    ) -> Result<String, ProcessingError> {
        // Simple FAQ-style answers
        let q_lower = question.to_lowercase();

        if q_lower.contains("what") && q_lower.contains("missing") {
            return Ok(format!(
                "Missing capability domains:\n{}",
                analysis
                    .missing_domains
                    .iter()
                    .map(|d| format!("  - {}", d))
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
        }

        if q_lower.contains("what") && q_lower.contains("available") {
            return Ok(format!(
                "Available capability domains:\n{}",
                analysis
                    .available_domains
                    .iter()
                    .map(|d| format!("  - {}", d))
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
        }

        if q_lower.contains("feasibility") || q_lower.contains("possible") {
            return Ok(format!(
                "Current feasibility: {:.0}%\n\
                 This means we can achieve about {:.0}% of the goal with current capabilities.",
                analysis.feasibility * 100.0,
                analysis.feasibility * 100.0
            ));
        }

        // Default: echo back and ask for more context
        Ok(format!(
            "I heard: \"{}\"\n\
             Could you rephrase or provide more context?\n\
             Current goal: \"{}\"",
            question, current_goal
        ))
    }
}

/// Errors during turn processing
#[derive(Debug, thiserror::Error)]
pub enum ProcessingError {
    #[error("Insufficient capabilities: {feasibility:.0}% feasible, missing: {missing:?}")]
    InsufficientCapabilities {
        feasibility: f32,
        missing: Vec<String>,
    },

    #[error("Planning failed: {0}")]
    PlanningFailed(String),

    #[error("Discovery failed: {0}")]
    DiscoveryFailed(String),

    #[error("Connection failed: {0}")]
    ConnectionFailed(String),

    #[error("Synthesis failed: {0}")]
    SynthesisFailed(String),
}
