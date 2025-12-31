//! Discovery operations - pure logic functions for server discovery

use crate::discovery::config::DiscoveryConfig;
use crate::discovery::{ApprovalQueue, GoalDiscoveryAgent};
use crate::utils::fs::find_workspace_root;
#[cfg(feature = "tui")]
use dialoguer::{theme::ColorfulTheme, Confirm, MultiSelect, Select};
use rtfs::runtime::error::RuntimeResult;

/// Options for goal-driven server discovery
#[derive(Debug, Clone)]
pub struct DiscoverOptions {
    /// Interactive mode - let user select which servers to queue
    pub interactive: bool,
    /// Limit to top N results
    pub top: Option<usize>,
    /// Minimum relevance score threshold
    pub threshold: f64,
    /// Use LLM for semantic analysis
    pub llm: bool,
}

impl Default for DiscoverOptions {
    fn default() -> Self {
        Self {
            interactive: false,
            top: None,
            threshold: 0.65,
            llm: false,
        }
    }
}

/// Goal-driven server discovery (simple API - uses defaults)
pub async fn discover_by_goal(goal: String) -> RuntimeResult<Vec<String>> {
    discover_by_goal_with_options(goal, DiscoverOptions::default()).await
}

/// Goal-driven server discovery with options
pub async fn discover_by_goal_with_options(
    goal: String,
    options: DiscoverOptions,
) -> RuntimeResult<Vec<String>> {
    // Create config with custom threshold
    let mut config = DiscoveryConfig::from_env();
    config.match_threshold = options.threshold;

    let workspace_root = find_workspace_root();
    let queue = ApprovalQueue::new(&workspace_root);
    let agent = GoalDiscoveryAgent::new_with_config(queue, config);

    // Show mode being used
    if options.llm {
        println!("🧠 Using LLM-enhanced discovery (intent analysis + semantic ranking)");
    }

    // Get scored results (before queuing)
    let scored_results = agent.search_and_score(&goal, options.llm).await?;

    if scored_results.is_empty() {
        println!("🔍 No matching servers found.");

        if options.interactive {
            // Fallback: ask user if they want to add a server manually
            println!();
            #[cfg(feature = "tui")]
            let add_manual = dialoguer::Confirm::with_theme(&dialoguer::theme::ColorfulTheme::default())
                .with_prompt("Do you know a specific server URL you want to add?")
                .default(false)
                .interact()
                .unwrap_or(false);

            #[cfg(not(feature = "tui"))]
            let add_manual = false;

            if add_manual {
                return handle_manual_url_entry(&agent, &goal, options.llm).await;
            }
        }

        return Ok(vec![]);
    }

    // Apply top N limit if specified
    // In non-interactive mode, default to top 3 to avoid spamming the approval queue
    let effective_top = options
        .top
        .or(if !options.interactive { Some(3) } else { None });
    let total_found = scored_results.len();

    let results: Vec<_> = if let Some(n) = effective_top {
        if total_found > n && !options.interactive {
            println!(
                "⚠️  Non-interactive mode: limiting to top {} results (found {}). Use --interactive or --top N to see all.",
                n, total_found
            );
        }
        scored_results.into_iter().take(n).collect()
    } else {
        scored_results
    };

    println!(
        "🔍 Processing {} server candidates (threshold: {:.2})",
        results.len(),
        options.threshold
    );

    // Interactive selection if enabled
    if options.interactive {
        // Build selection items with scores
        let items: Vec<String> = results
            .iter()
            .map(|(result, score)| {
                let desc = result
                    .server_info
                    .description
                    .as_deref()
                    .unwrap_or("")
                    .chars()
                    .take(60)
                    .collect::<String>();
                format!("[{:.2}] {} - {}", score, result.server_info.name, desc)
            })
            .collect();

        if items.is_empty() {
            return Ok(vec![]);
        }

        // Show instructions before the multi-select
        println!("\n📋 All servers pre-selected. Use SPACE to toggle, ENTER to confirm.\n");

        // Show interactive multi-select (all pre-selected - user deselects unwanted)
        #[cfg(feature = "tui")]
        let selections = dialoguer::MultiSelect::with_theme(&dialoguer::theme::ColorfulTheme::default())
            .with_prompt("Select servers to queue")
            .items(&items)
            .defaults(&vec![true; items.len()]) // Pre-select all
            .interact()
            .map_err(|e| {
                rtfs::runtime::error::RuntimeError::Generic(format!("Selection cancelled: {}", e))
            })?;

        #[cfg(not(feature = "tui"))]
        return Err(rtfs::runtime::error::RuntimeError::Generic(
            "Interactive mode not available".to_string(),
        ));

        if selections.is_empty() {
            println!("❌ No servers selected.");

            // Fallback: ask user if they want to add a server manually
            println!();
            #[cfg(feature = "tui")]
            let add_manual = dialoguer::Confirm::with_theme(&dialoguer::theme::ColorfulTheme::default())
                .with_prompt("Do you know a specific server URL you want to add instead?")
                .default(false)
                .interact()
                .unwrap_or(false);

            #[cfg(not(feature = "tui"))]
            let add_manual = false;

            if add_manual {
                return handle_manual_url_entry(&agent, &goal, options.llm).await;
            }

            return Ok(vec![]);
        }

        println!("✓ Selected {} of {} servers", selections.len(), items.len());

        // Filter to selected items
        let selected: Vec<_> = selections.into_iter().map(|i| results[i].clone()).collect();

        // Check for conflicts with already approved or pending servers and prompt user
        let workspace_root = find_workspace_root();
        let queue = crate::discovery::ApprovalQueue::new(&workspace_root);
        let mut servers_to_queue = Vec::new();

        for (result, score) in &selected {
            let approved = queue.list_approved()?;
            let pending = queue.list_pending()?;

            // Check approved servers first
            let approved_conflict = approved.iter().find(|existing| {
                existing.server_info.name == result.server_info.name
                    || (!result.server_info.endpoint.is_empty()
                        && existing.server_info.endpoint == result.server_info.endpoint)
            });

            if let Some(existing) = approved_conflict {
                println!();
                println!(
                    "⚠️  Server \"{}\" already exists in approved list",
                    existing.server_info.name
                );
                println!(
                    "   Current: v{}, approved on {}",
                    existing.version,
                    &existing.approved_at.to_rfc3339()[..10]
                );
                println!(
                    "   New discovery: \"{}\" ({})",
                    result.server_info.name, result.server_info.endpoint
                );
                println!();

                let options = vec![
                    "Add to pending for re-approval (merge on approval)",
                    "Skip - Keep existing, don't add to pending",
                ];

                #[cfg(feature = "tui")]
                let selection = dialoguer::Select::with_theme(&dialoguer::theme::ColorfulTheme::default())
                    .with_prompt("What would you like to do?")
                    .items(&options)
                    .default(0)
                    .interact()
                    .map_err(|e| {
                        rtfs::runtime::error::RuntimeError::Generic(format!(
                            "Selection error: {}",
                            e
                        ))
                    })?;

                #[cfg(not(feature = "tui"))]
                let selection = 1; // Default to Skip in non-interactive

                if selection == 0 {
                    // Add to pending
                    servers_to_queue.push((result.clone(), *score));
                } else {
                    println!("   ✓ Skipped - keeping existing approved server");
                }
                continue;
            }

            // Check pending servers
            let pending_conflict = pending.iter().find(|existing| {
                existing.server_info.name == result.server_info.name
                    || (!result.server_info.endpoint.is_empty()
                        && existing.server_info.endpoint == result.server_info.endpoint)
            });

            if let Some(existing) = pending_conflict {
                println!();
                println!(
                    "⚠️  Server \"{}\" already exists in pending list",
                    existing.server_info.name
                );
                println!(
                    "   Current: queued on {}, expires on {}",
                    &existing.requested_at.to_rfc3339()[..10],
                    &existing.expires_at.to_rfc3339()[..10]
                );
                println!(
                    "   New discovery: \"{}\" ({})",
                    result.server_info.name, result.server_info.endpoint
                );
                println!();

                let options = vec![
                    "Merge - Update existing pending entry (keeps existing ID)",
                    "Replace - Remove old entry and add new one",
                    "Skip - Keep existing pending entry",
                ];

                #[cfg(feature = "tui")]
                let selection = dialoguer::Select::with_theme(&dialoguer::theme::ColorfulTheme::default())
                    .with_prompt("What would you like to do?")
                    .items(&options)
                    .default(0)
                    .interact()
                    .map_err(|e| {
                        rtfs::runtime::error::RuntimeError::Generic(format!(
                            "Selection error: {}",
                            e
                        ))
                    })?;

                #[cfg(not(feature = "tui"))]
                let selection = 2; // Default to Skip in non-interactive

                if selection == 0 {
                    // Merge - add will automatically merge
                    servers_to_queue.push((result.clone(), *score));
                } else if selection == 1 {
                    // Replace - remove old entry first
                    queue.remove_pending(&existing.id)?;
                    servers_to_queue.push((result.clone(), *score));
                } else {
                    println!("   ✓ Skipped - keeping existing pending entry");
                }
            } else {
                // No conflict, add to pending
                servers_to_queue.push((result.clone(), *score));
            }
        }

        if servers_to_queue.is_empty() {
            println!("⚠️  No servers to queue (all skipped or already approved)");
            return Ok(vec![]);
        }

        // Queue servers to pending FIRST (before introspection)
        let mut queued_servers = Vec::new();
        for (result, score) in &servers_to_queue {
            let id = agent.queue_result(&goal, result.clone(), *score)?;
            queued_servers.push((id, result.clone()));
        }

        // Ask if user wants to introspect tools
        println!();
        #[cfg(feature = "tui")]
        let introspect = dialoguer::Confirm::with_theme(&dialoguer::theme::ColorfulTheme::default())
            .with_prompt("Do you want to introspect queued servers to discover their tools?")
            .default(true)
            .interact()
            .unwrap_or(false);

        #[cfg(not(feature = "tui"))]
        let introspect = false; // Default to false in non-interactive

        if introspect {
            println!("\n🔍 Introspecting queued servers...\n");
            for (pending_id, result) in &queued_servers {
                let name = &result.server_info.name;

                // Collect all endpoints to try (primary + alternatives)
                let mut endpoints_to_try = vec![result.server_info.endpoint.clone()];
                endpoints_to_try.extend(result.server_info.alternative_endpoints.clone());

                // Filter to only HTTP endpoints
                endpoints_to_try.retain(|e| !e.is_empty() && e.starts_with("http"));

                if endpoints_to_try.is_empty() {
                    println!("⚠️  {} - No HTTP endpoint, skipping", name);
                    continue;
                }

                // Show which endpoints we'll try
                if endpoints_to_try.len() > 1 {
                    println!(
                        "📡 Connecting to {} ({} endpoint(s) available)...",
                        name,
                        endpoints_to_try.len()
                    );
                } else {
                    println!("📡 Connecting to {}...", name);
                }

                let auth_env_var = result.server_info.auth_env_var.as_deref();
                let mut introspection_success = false;

                // Try each endpoint until one succeeds
                for (idx, endpoint) in endpoints_to_try.iter().enumerate() {
                    if idx > 0 {
                        println!(
                            "   🔄 Trying alternative endpoint {} of {}...",
                            idx + 1,
                            endpoints_to_try.len()
                        );
                    }

                    match crate::ops::server::introspect_server_by_url(endpoint, name, auth_env_var)
                        .await
                    {
                        Ok(introspection) => {
                            if introspection.tools.is_empty() {
                                println!("   ⚠️  No tools found");
                            } else {
                                println!("   ✅ Found {} tools:", introspection.tools.len());
                                for tool in introspection.tools.iter().take(10) {
                                    let desc = tool
                                        .description
                                        .as_deref()
                                        .map(|d| d.chars().take(50).collect::<String>())
                                        .unwrap_or_default();
                                    println!("      • {} - {}", tool.tool_name, desc);
                                }
                                if introspection.tools.len() > 10 {
                                    println!(
                                        "      ... and {} more",
                                        introspection.tools.len() - 10
                                    );
                                }

                                // Save tools to RTFS file and link to pending entry
                                if let Err(e) = crate::ops::server::save_discovered_tools(
                                    &introspection,
                                    &result.server_info,
                                    Some(pending_id),
                                )
                                .await
                                {
                                    println!("   ⚠️  Failed to save capabilities: {}", e);
                                } else {
                                    println!("   💾 Capabilities saved to RTFS file");
                                }
                            }
                            introspection_success = true;
                            break; // Success, no need to try other endpoints
                        }
                        Err(e) => {
                            let error_msg = e.to_string();

                            // If this is the last endpoint, show the error
                            if idx == endpoints_to_try.len() - 1 {
                                // Check if it's an auth failure
                                if error_msg.contains("401")
                                    || error_msg.contains("Unauthorized")
                                    || error_msg.contains("not set")
                                {
                                    println!("   ⚠️  Authentication required");

                                    // Show expected env var
                                    if let Some(env_var) = auth_env_var {
                                        println!(
                                            "   📝 Expected environment variable: {}",
                                            env_var
                                        );
                                        println!(
                                            "   💡 Set it with: export {}=<your-token>",
                                            env_var
                                        );

                                        // GitHub-specific hint
                                        if name.to_lowercase().contains("github") {
                                            println!("   💡 For GitHub, you can also use: GITHUB_TOKEN or GITHUB_PAT");
                                        }

                                        // Ask if user wants to update the token and retry
                                        println!();
                                        #[cfg(feature = "tui")]
                                        let retry = dialoguer::Confirm::with_theme(
                                            &dialoguer::theme::ColorfulTheme::default(),
                                        )
                                        .with_prompt(&format!(
                                            "Do you want to set {} and retry?",
                                            env_var
                                        ))
                                        .default(false)
                                        .interact()
                                        .unwrap_or(false);

                                        #[cfg(not(feature = "tui"))]
                                        let retry = false;

                                        if retry {
                                            // Prompt for token (hidden input)
                                            #[cfg(feature = "tui")]
                                            let token = dialoguer::Password::with_theme(
                                                &dialoguer::theme::ColorfulTheme::default(),
                                            )
                                            .with_prompt(&format!(
                                                "Enter token for {} (input hidden)",
                                                env_var
                                            ))
                                            .interact()
                                            .ok();

                                            #[cfg(not(feature = "tui"))]
                                            let token: Option<String> = None;

                                            if let Some(token) = token {
                                                // Validate env_var name before setting (set_var can panic on invalid names)
                                                let token_set = if env_var.is_empty()
                                                    || env_var.contains('=')
                                                    || env_var.contains('\0')
                                                {
                                                    println!("   ⚠️  Invalid environment variable name: {}", env_var);
                                                    false
                                                } else if token.contains('\0') {
                                                    println!(
                                                        "   ⚠️  Token contains invalid characters"
                                                    );
                                                    false
                                                } else {
                                                    // Safe to set: env_var is validated and token is from user input
                                                    // Note: set_var can panic on invalid input, but we've validated both
                                                    // In single-threaded CLI context, this is safe
                                                    unsafe {
                                                        std::env::set_var(env_var, &token);
                                                    }
                                                    println!("   ✓ Token set. Retrying...");
                                                    true
                                                };

                                                // Only retry if token was successfully set
                                                if token_set {
                                                    // Retry with new token
                                                    match crate::ops::server::introspect_server_by_url(endpoint, name, Some(env_var)).await {
                                                    Ok(introspection) => {
                                                        if introspection.tools.is_empty() {
                                                            println!("   ⚠️  No tools found");
                                                        } else {
                                                            println!("   ✅ Found {} tools:", introspection.tools.len());
                                                            for tool in introspection.tools.iter().take(10) {
                                                                let desc = tool.description.as_deref()
                                                                    .map(|d| d.chars().take(50).collect::<String>())
                                                                    .unwrap_or_default();
                                                                println!("      • {} - {}", tool.tool_name, desc);
                                                            }
                                                            if introspection.tools.len() > 10 {
                                                                println!("      ... and {} more", introspection.tools.len() - 10);
                                                            }

                                                            // Save tools to RTFS file and link to pending entry
                                                            if let Err(e) = crate::ops::server::save_discovered_tools(
                                                                &introspection,
                                                                &result.server_info,
                                                                Some(pending_id),
                                                            ).await {
                                                                println!("   ⚠️  Failed to save capabilities: {}", e);
                                                            } else {
                                                                println!("   💾 Capabilities saved to RTFS file");
                                                            }
                                                            introspection_success = true;
                                                        }
                                                    }
                                                    Err(e2) => {
                                                        println!("   ❌ Still failed: {}", e2);
                                                        println!("   💡 Troubleshooting:");

                                                        // Check if it's GitHub Copilot endpoint
                                                        if endpoint.contains("githubcopilot.com") {
                                                            println!("      ⚠️  This is GitHub Copilot MCP (not regular GitHub)");
                                                            println!("      • Requires a GitHub Copilot API token (not regular PAT)");
                                                            println!("      • Get token from: https://github.com/settings/tokens?type=beta");
                                                            println!("      • Token must have 'copilot' scope/permissions");
                                                        } else {
                                                            println!("      • Verify token is valid and not expired");
                                                            println!("      • Check token has required permissions/scopes");
                                                            if name.to_lowercase().contains("github") {
                                                                println!("      • For GitHub: Use a Personal Access Token (PAT) with appropriate scopes");
                                                            }
                                                        }
                                                        println!("      • Token should be just the token value (we add 'Bearer' prefix automatically)");
                                                    }
                                                }
                                                }
                                            }
                                        }
                                    }
                                } else {
                                    println!("   ❌ {}", error_msg);
                                }
                            } else {
                                // Not the last endpoint, just log and continue trying
                                if endpoints_to_try.len() > 1 {
                                    println!(
                                        "   ⚠️  Failed: {} (trying next endpoint...)",
                                        error_msg
                                    );
                                } else {
                                    println!("   ❌ Failed: {}", error_msg);
                                }
                            }
                        }
                    }
                }

                // If all endpoints failed, show a summary
                if !introspection_success && endpoints_to_try.len() > 1 {
                    println!(
                        "   ❌ All {} endpoint(s) failed for {}",
                        endpoints_to_try.len(),
                        name
                    );
                }
                println!();
            }
        }

        // Return queued IDs (already queued before introspection)
        return Ok(queued_servers.iter().map(|(id, _)| id.clone()).collect());
    }

    // Non-interactive mode: check for conflicts and queue
    let workspace_root = find_workspace_root();
    let queue = crate::discovery::ApprovalQueue::new(&workspace_root);
    let approved = queue.list_approved()?;
    let mut servers_to_queue = Vec::new();

    for (result, score) in &results {
        let conflict = approved.iter().find(|existing| {
            existing.server_info.name == result.server_info.name
                || (!result.server_info.endpoint.is_empty()
                    && existing.server_info.endpoint == result.server_info.endpoint)
        });

        if conflict.is_none() {
            // No conflict, add to pending
            servers_to_queue.push((result.clone(), *score));
        }
        // If conflict exists, skip silently in non-interactive mode
    }

    // Queue selected results
    let mut queued_ids = Vec::new();
    for (result, score) in servers_to_queue {
        let id = agent.queue_result(&goal, result, score)?;
        queued_ids.push(id);
    }

    Ok(queued_ids)
}

/// Search catalog
pub async fn search_catalog(_query: String) -> RuntimeResult<Vec<String>> {
    // TODO: Implement catalog search logic
    Ok(vec![])
}

/// Inspect capability details
pub async fn inspect_capability(id: String) -> RuntimeResult<String> {
    // TODO: Implement capability inspection logic
    Ok(format!("Details for capability: {}", id))
}

/// Handle manual URL entry - prompts for URL type and routes to appropriate handler
/// Loops to allow adding multiple URLs until user is done
async fn handle_manual_url_entry(
    agent: &GoalDiscoveryAgent,
    goal: &str,
    llm_enabled: bool,
) -> RuntimeResult<Vec<String>> {
    let mut all_ids = Vec::new();

    loop {
        // Ask what type of URL
        let url_types = vec![
            "MCP Server endpoint (e.g., https://api.example.com/mcp/)",
            "API Documentation page (requires --llm flag)",
            "Done - no more URLs to add",
        ];

        let url_type_idx: usize;
        #[cfg(feature = "tui")]
        {
            url_type_idx = dialoguer::Select::with_theme(&dialoguer::theme::ColorfulTheme::default())
                .with_prompt("What type of URL are you providing?")
                .items(&url_types)
                .default(0)
                .interact()
                .map_err(|e| {
                    rtfs::runtime::error::RuntimeError::Generic(format!("Selection error: {}", e))
                })?;
        }

        #[cfg(not(feature = "tui"))]
        {
            return Err(rtfs::runtime::error::RuntimeError::Generic(
                "Interactive mode not available".to_string(),
            ));
        }

        // Exit loop if user is done
        if url_type_idx == 2 {
            break;
        }

        #[cfg(feature = "tui")]
        let url: String = dialoguer::Input::with_theme(&ColorfulTheme::default())
            .with_prompt("Enter URL")
            .interact_text()
            .map_err(|e| {
                rtfs::runtime::error::RuntimeError::Generic(format!("Input error: {}", e))
            })?;

        #[cfg(not(feature = "tui"))]
        let url: String = return Err(rtfs::runtime::error::RuntimeError::Generic(
            "Interactive mode not available".to_string(),
        ));

        let ids = if url_type_idx == 0 {
            // MCP Server endpoint
            handle_mcp_url(agent, goal, &url).await?
        } else {
            // API Documentation page
            if !llm_enabled {
                println!("⚠️  Documentation parsing requires the --llm flag.");
                println!(
                    "   Run again with: ccos discover goal \"{}\" --interactive --llm",
                    goal
                );
                println!();
                continue; // Let user try another URL type
            }
            handle_documentation_url(agent, goal, &url).await?
        };

        all_ids.extend(ids);

        // Ask if user wants to add more
        println!();
        #[cfg(feature = "tui")]
        let add_more = dialoguer::Confirm::with_theme(&dialoguer::theme::ColorfulTheme::default())
            .with_prompt("Do you want to add another server/API?")
            .default(false)
            .interact()
            .unwrap_or(false);

        #[cfg(not(feature = "tui"))]
        let add_more = false;

        if !add_more {
            break;
        }
        println!();
    }

    if !all_ids.is_empty() {
        println!("\n✓ Queued {} server(s) for approval.", all_ids.len());
        println!("  • Use 'ccos approval pending' to review and approve.");
    }

    Ok(all_ids)
}

/// Handle MCP server URL - introspect and queue
async fn handle_mcp_url(
    agent: &GoalDiscoveryAgent,
    goal: &str,
    url: &str,
) -> RuntimeResult<Vec<String>> {
    println!("🔍 Introspecting MCP server at {}...", url);

    // Derive server name from URL
    let name_guess = url
        .split("://")
        .nth(1)
        .unwrap_or("unknown")
        .split('/')
        .next()
        .unwrap_or("unknown")
        .to_string();

    match crate::ops::server::introspect_server_by_url(url, &name_guess, None).await {
        Ok(introspection) => {
            println!("   ✅ Introspection successful!");
            println!("   Server Name: {}", introspection.server_name);
            println!("   Tools Found: {}", introspection.tools.len());

            // Create a synthetic RegistrySearchResult
            let result = crate::discovery::registry_search::RegistrySearchResult {
                source: crate::discovery::approval_queue::DiscoverySource::Manual {
                    user: "cli".to_string(),
                },
                server_info: crate::discovery::approval_queue::ServerInfo {
                    name: introspection.server_name.clone(),
                    endpoint: introspection.server_url.clone(),
                    description: Some("Manually added MCP server".to_string()),
                    auth_env_var: None,
                    capabilities_path: None,
                    alternative_endpoints: Vec::new(),
                },
                match_score: 1.0,
                alternative_endpoints: Vec::new(),
            };

            let id = agent.queue_result(goal, result.clone(), 1.0)?;

            // Save tools
            if let Err(e) = crate::ops::server::save_discovered_tools(
                &introspection,
                &result.server_info,
                Some(&id),
            )
            .await
            {
                println!("   ⚠️  Failed to save capabilities: {}", e);
            } else {
                println!("   💾 Capabilities saved to RTFS file");
            }

            Ok(vec![id])
        }
        Err(e) => {
            println!("❌ Failed to introspect MCP server: {}", e);
            Ok(vec![])
        }
    }
}

/// Handle documentation URL - use LLM to parse API docs
async fn handle_documentation_url(
    agent: &GoalDiscoveryAgent,
    goal: &str,
    url: &str,
) -> RuntimeResult<Vec<String>> {
    println!("🔍 Fetching API documentation from {}...", url);
    println!("🤖 Using LLM to extract API endpoints...");

    // Get LLM provider from arbiter (async)
    let llm_provider = match crate::arbiter::get_default_llm_provider().await {
        Some(provider) => provider,
        None => {
            println!("❌ No LLM provider configured.");
            println!("   Set OPENAI_API_KEY or ANTHROPIC_API_KEY environment variable.");
            return Ok(vec![]);
        }
    };

    // Parse documentation with LLM
    let parser = crate::synthesis::introspection::llm_doc_parser::LlmDocParser::new();

    // Extract domain from URL for context
    let domain = url
        .split("://")
        .nth(1)
        .unwrap_or("unknown")
        .split('/')
        .next()
        .unwrap_or("unknown")
        .to_string();

    match parser
        .parse_from_url(url, &domain, llm_provider.as_ref())
        .await
    {
        Ok(api_result) => {
            println!("   ✅ Documentation parsed successfully!");
            println!(
                "   API: {} v{}",
                api_result.api_title, api_result.api_version
            );
            println!("   Base URL: {}", api_result.base_url);
            println!("   Endpoints found: {}", api_result.endpoints.len());

            for endpoint in api_result.endpoints.iter().take(5) {
                println!(
                    "      • {} {} - {}",
                    endpoint.method, endpoint.path, endpoint.name
                );
            }
            if api_result.endpoints.len() > 5 {
                println!("      ... and {} more", api_result.endpoints.len() - 5);
            }

            // Create a synthetic RegistrySearchResult for the API
            let result = crate::discovery::registry_search::RegistrySearchResult {
                source: crate::discovery::approval_queue::DiscoverySource::Manual {
                    user: "cli".to_string(),
                },
                server_info: crate::discovery::approval_queue::ServerInfo {
                    name: api_result.api_title.clone(),
                    endpoint: api_result.base_url.clone(),
                    description: Some(format!("HTTP API parsed from documentation: {}", url)),
                    auth_env_var: api_result.auth_requirements.env_var_name.clone(),
                    capabilities_path: None,
                    alternative_endpoints: Vec::new(),
                },
                match_score: 1.0,
                alternative_endpoints: Vec::new(),
            };

            let id = agent.queue_result(goal, result.clone(), 1.0)?;

            // Save the parsed API as capabilities
            if let Err(e) = crate::ops::server::save_api_capabilities(&api_result, &id).await {
                println!("   ⚠️  Failed to save capabilities: {}", e);
            } else {
                println!("   💾 Capabilities saved to RTFS file");
            }

            Ok(vec![id])
        }
        Err(e) => {
            println!("❌ Failed to parse documentation: {}", e);
            println!("   💡 Tips:");
            println!("      • Ensure the URL points to API documentation");
            println!("      • Check that LLM API key is valid");
            println!("      • Try a more specific documentation page");
            Ok(vec![])
        }
    }
}
