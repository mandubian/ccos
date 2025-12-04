//! Autonomous Agent Demo (Iterative & Recursive)
//!
//! This example demonstrates an advanced, self-evolving autonomous agent that:
//! 1. Takes a high-level goal from the user.
//! 2. Iteratively decomposes it into steps using the Arbiter.
//! 3. Resolves capabilities for each step (Local -> Semantic Search -> MCP Registry).
//! 4. Recursively plans for missing capabilities that can't be found directly.
//! 5. Synthesizes missing capabilities using LLM-generated RTFS code (Phase B).
//! 6. Constructs a final executable RTFS plan.
//! 7. Traces the decision process.
//!
//! **Phase B (Implemented):** True Code Synthesis
//! - When a capability is missing, the agent asks the LLM to write RTFS code
//! - Generated code is validated (parse check) and registered dynamically
//! - The synthesized capability can be executed just like any other capability
//!
//! **Phase C (Planned):** Data Flow Adapters
//! - When capabilities have known input/output schemas, create adapters
//! - Transform previous step output to match next step input requirements
//! - Example: remote.list_items returns {:records [...]}, but filter expects {:items [...]}
//! - Adapter: (fn [prev-output] {:items (:records prev-output)})
//!
//! Usage:
//!   cargo run --example autonomous_agent_demo -- --goal "find the issues of repository ccos and user mandubian and filter them to keep only those containing RTFS"

use std::collections::HashMap;
use std::error::Error;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use ccos::arbiter::DelegatingArbiter;
use ccos::capabilities::{MCPSessionHandler, SessionPoolManager};
use ccos::capability_marketplace::{CapabilityManifest, CapabilityMarketplace};
use ccos::catalog::{CatalogEntryKind, CatalogFilter, CatalogService};
use ccos::discovery::capability_matcher::{compute_mcp_tool_score, keyword_overlap};
use ccos::discovery::embedding_service::EmbeddingService;
use ccos::mcp::discovery_session::{MCPServerInfo, MCPSessionManager};
use ccos::mcp::registry::MCPRegistryClient;
use ccos::mcp::types::DiscoveredMCPTool;
use ccos::synthesis::mcp_introspector::MCPIntrospector;
use ccos::CCOS;
use clap::Parser;
use rtfs::config::types::AgentConfig;
use rtfs::runtime::error::RuntimeResult;
use rtfs::runtime::security::RuntimeContext;
use rtfs::runtime::values::Value;

// Import Modular Planner components
use ccos::planner::modular_planner::orchestrator::{PlanResult, TraceEvent as ModularTraceEvent};
use ccos::planner::modular_planner::resolution::mcp::RuntimeMcpDiscovery;
use ccos::planner::modular_planner::resolution::{CompositeResolution, McpResolution};
use ccos::planner::modular_planner::{
    CatalogResolution, DecompositionStrategy, ModularPlanner, PatternDecomposition, PlannerConfig,
    ResolvedCapability,
};
use ccos::planner::CcosCatalogAdapter;

// ============================================================================
// CLI Arguments
// ============================================================================

#[derive(Parser, Debug)]
struct Args {
    /// Natural language goal
    #[arg(long, default_value = "Find the weather in Paris and filter for rain")]
    goal: String,

    /// Path to agent config file
    #[arg(long, default_value = "config/agent_config.toml")]
    config: String,

    /// Optional LLM profile name
    #[arg(long)]
    profile: Option<String>,

    /// Simulate a runtime error to test the repair loop
    #[arg(long)]
    simulate_error: bool,

    /// Enable mock fallback for missing capabilities (disabled by default)
    #[arg(long)]
    allow_mock: bool,

    /// Use the new modular planner architecture
    #[arg(long)]
    use_modular_planner: bool,
}

// ============================================================================
// Data Structures for Planning
// ============================================================================

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
struct PlannedStep {
    description: String,
    capability_hint: String,
}

#[derive(Debug, serde::Deserialize)]
struct Decomposition {
    steps: Vec<PlannedStep>,
}

/// Primary intent extracted from a goal - the main action and object
#[derive(Debug, Clone, serde::Deserialize)]
struct PrimaryIntent {
    /// The main action verb (e.g., "list", "check", "get", "create")
    action: String,
    /// The main object/target (e.g., "pull requests", "issues", "user")
    object: String,
    /// Alternative phrasings for the object (e.g., "PRs" for "pull requests")
    #[serde(default)]
    object_synonyms: Vec<String>,
}

/// Resolution status for a planning step
#[derive(Debug, Clone)]
enum ResolutionStatus {
    ResolvedLocal(String, HashMap<String, String>), // ID, Args
    ResolvedRemote(String, HashMap<String, String>), // ID, Args (installed from MCP)
    ResolvedSynthesized(String, HashMap<String, String>), // ID, Args (generated)
    NeedsSubPlan(String, String),                   // Goal, Hint (Recursive)
    /// Capability cannot be resolved - needs external referral (user or another entity)
    /// Contains: capability description, what's needed, suggested action
    NeedsReferral {
        description: String,
        reason: String,
        suggested_action: String,
    },
    Failed(String), // Reason
}

#[derive(Debug, serde::Serialize)]
struct PlanningTrace {
    goal: String,
    decisions: Vec<TraceEvent>,
}

#[derive(Debug, serde::Serialize)]
enum TraceEvent {
    Decomposition(Vec<PlannedStep>),
    ResolutionAttempt {
        step: String,
        status: String,
    },
    MCPDiscovery {
        hint: String,
        found: bool,
    },
    Synthesis {
        capability: String,
        success: bool,
    },
    RecursiveSubPlan {
        parent_step: String,
        sub_goal: String,
    },
}

// ============================================================================
// Main Planner Loop
// ============================================================================

struct IterativePlanner {
    _ccos: Arc<CCOS>,
    arbiter: Arc<DelegatingArbiter>,
    marketplace: Arc<CapabilityMarketplace>,
    catalog: Arc<CatalogService>,
    trace: PlanningTrace,
    simulate_error: bool,
    allow_mock: bool,
    embedding_service: Option<EmbeddingService>,
}

impl IterativePlanner {
    fn new(
        ccos: Arc<CCOS>,
        simulate_error: bool,
        allow_mock: bool,
    ) -> Result<Self, Box<dyn Error + Send + Sync>> {
        let arbiter = ccos
            .get_delegating_arbiter()
            .ok_or::<Box<dyn Error + Send + Sync>>("Delegating arbiter not available".into())?;
        let marketplace = ccos.get_capability_marketplace();
        let catalog = ccos.get_catalog();

        // Initialize embedding service for semantic tool matching
        // Priority: LOCAL_EMBEDDING_URL (Ollama) > OPENROUTER_API_KEY
        let embedding_service = EmbeddingService::from_env();
        if let Some(ref svc) = embedding_service {
            println!(
                "     🧬 Embedding service initialized: {}",
                svc.provider_description()
            );
        } else {
            println!("     ⚠️  No embedding service available (set LOCAL_EMBEDDING_URL or OPENROUTER_API_KEY)");
        }

        Ok(Self {
            _ccos: ccos,
            arbiter: arbiter.clone(),
            marketplace,
            catalog,
            trace: PlanningTrace {
                goal: "Unknown".to_string(),
                decisions: Vec::new(),
            },
            simulate_error,
            allow_mock,
            embedding_service,
        })
    }

    // Recursive async function requires manual boxing
    fn solve<'a>(
        &'a mut self,
        goal: &'a str,
        depth: usize,
    ) -> Pin<Box<dyn Future<Output = Result<String, Box<dyn Error + Send + Sync>>> + 'a>> {
        Box::pin(async move {
            if depth > 5 {
                return Err("Max recursion depth exceeded".into());
            }
            self.trace.goal = goal.to_string();
            println!("\n🧠 Solving Goal (Depth {}): \"{}\"", depth, goal);

            // 0. Try direct MCP tool match before decomposition
            // This avoids over-decomposition for simple API calls
            // BUT only if the goal is simple!
            if !self.is_complex_goal(goal) {
                if let Some((cap_id, args)) = self.try_direct_mcp_match(goal).await? {
                    println!("     ✅ Direct MCP match found: {}", cap_id);
                    let call_expr = self.generate_call(&cap_id, args);
                    return Ok(call_expr);
                }
            } else {
                println!("     🔄 Goal appears complex (contains conjunctions/logic), skipping direct match to force decomposition.");
            }

            // 1. Decompose
            let steps = self.decompose(goal).await?;
            self.trace
                .decisions
                .push(TraceEvent::Decomposition(steps.clone()));

            // Build nested let bindings with context accumulation for data flow
            let mut step_bindings = Vec::new();
            let mut context_entries: Vec<String> = Vec::new(); // Track all previous steps for context map

            for (i, step) in steps.iter().enumerate() {
                println!(
                    "\n  👉 Step {}: {} (Hint: {})",
                    i + 1,
                    step.description,
                    step.capability_hint
                );

                // 2. Resolve
                let status = self.resolve_step(step).await?;

                self.trace.decisions.push(TraceEvent::ResolutionAttempt {
                    step: step.description.clone(),
                    status: format!("{:?}", status),
                });

                let (capability_id, args) = match status {
                    ResolutionStatus::ResolvedLocal(id, a) => {
                        println!("     ✅ Resolved Local: {}", id);
                        (id, a)
                    }
                    ResolutionStatus::ResolvedRemote(id, a) => {
                        println!("     ✅ Resolved Remote (Installed): {}", id);
                        (id, a)
                    }
                    ResolutionStatus::ResolvedSynthesized(id, a) => {
                        println!("     ✅ Resolved Synthesized: {}", id);
                        (id, a)
                    }
                    ResolutionStatus::NeedsSubPlan(sub_goal, _hint) => {
                        println!("     🔄 Complex Step -> Triggering Recursive Sub-Planning...");
                        self.trace.decisions.push(TraceEvent::RecursiveSubPlan {
                            parent_step: step.description.clone(),
                            sub_goal: sub_goal.clone(),
                        });

                        // Recursive call!
                        let sub_plan_rtfs = self.solve(&sub_goal, depth + 1).await?;
                        step_bindings.push(("subplan".to_string(), sub_plan_rtfs));
                        continue;
                    }
                    ResolutionStatus::NeedsReferral {
                        description,
                        reason,
                        suggested_action,
                    } => {
                        println!("     🔔 REFERRAL NEEDED: {}", description);
                        println!("        Reason: {}", reason);
                        println!("        Suggested action: {}", suggested_action);

                        // Instead of failing, we create a placeholder call that will ask for input
                        // This uses the built-in ccos.user.ask capability
                        let referral_call = format!(
                            r#"(call "ccos.user.ask" {{:prompt "{}"}})"#,
                            format!(
                                "Cannot complete step: {}. {}. {}",
                                description, reason, suggested_action
                            )
                            .replace('"', "\\\"")
                        );
                        step_bindings.push((format!("step_{}_referral", i + 1), referral_call));
                        continue;
                    }
                    ResolutionStatus::Failed(reason) => {
                        println!("     ❌ Failed: {}", reason);
                        return Err(format!(
                            "Planning failed at step '{}': {}",
                            step.description, reason
                        )
                        .into());
                    }
                };

                // Generate call with data flow (Phase E: Context-Aware Adapter Synthesis)
                let step_var = format!("step_{}", i + 1);

                let call_expr = if !context_entries.is_empty() {
                    // If we have context from previous steps, try to adapt it to the current step
                    // If args are empty OR step implies dependency, try context adapter
                    if args.is_empty() || self.step_implies_context_dependency(&step.description) {
                        match self
                            .synthesize_adapter_with_context(
                                &context_entries,
                                &capability_id,
                                &step.description,
                                &args,
                            )
                            .await
                        {
                            Ok(adapter_expr) => adapter_expr,
                            Err(e) => {
                                println!("     ⚠️  Context-aware adapter synthesis failed: {}. Falling back to direct call.", e);
                                self.generate_call(&capability_id, args)
                            }
                        }
                    } else {
                        // If args are present and no dependency implied, assume they are sufficient
                        self.generate_call(&capability_id, args)
                    }
                } else {
                    // First step or no previous output
                    self.generate_call(&capability_id, args)
                };

                step_bindings.push((step_var.clone(), call_expr));
                context_entries.push(step_var.clone());
            }

            // Build nested let expression from bindings
            if step_bindings.is_empty() {
                return Ok("nil".to_string());
            }

            let plan_expr = if step_bindings.len() == 1 {
                // Single step - no binding needed, just the call
                step_bindings[0].1.clone()
            } else {
                // Multiple steps - build nested lets from innermost to outermost
                // Start with the last step's variable as the final result
                let last_var = &step_bindings[step_bindings.len() - 1].0;
                let mut expr = last_var.clone();

                // Wrap from last step to first step (building from inside out)
                for (var, call_expr) in step_bindings.iter().rev() {
                    if var == last_var {
                        // Last step - bind the call and use the variable as result
                        expr = format!("(let [{} {}]\n  {})", var, call_expr, expr);
                    } else {
                        // Earlier step - bind and continue nesting
                        expr = format!("(let [{} {}]\n  {})", var, call_expr, expr);
                    }
                }
                expr
            };

            Ok(plan_expr)
        })
    }

    async fn decompose(
        &self,
        goal: &str,
    ) -> Result<Vec<PlannedStep>, Box<dyn Error + Send + Sync>> {
        let prompt = format!(
            r#"You are an expert planner. Decompose the following goal into a sequence of logical steps.
For each step, provide a description and a short "capability hint" that looks like a tool ID (e.g. "service.action", "data.transform").

Goal: "{}"

Respond ONLY with a JSON object in this format:
{{
  "steps": [
    {{ "description": "Step description", "capability_hint": "service.action" }}
  ]
}}
"#,
            goal
        );

        let response = self.arbiter.generate_raw_text(&prompt).await?;
        let json_str = extract_json(&response);
        let decomposition: Decomposition = serde_json::from_str(json_str)?;
        Ok(decomposition.steps)
    }

    /// Check if goal implies multiple steps or complexity that warrants decomposition
    fn is_complex_goal(&self, goal: &str) -> bool {
        let lower = goal.to_lowercase();
        // Indicators that suggest the goal requires multiple steps, user interaction, or post-processing
        let complex_indicators = [
            " and ",
            " then ",
            " but ",
            " after ",
            " before ", // Conjunctions
            "ask me",
            "prompt me",
            "wait for",
            "user input", // Interaction
            "filter",
            "sort",
            "group by",
            "count",
            "aggregate", // Post-processing
            "save to",
            "write to",
            "export", // Side effects usually separate
            "extract",
            "transform", // Data manipulation
        ];

        for indicator in &complex_indicators {
            if lower.contains(indicator) {
                return true;
            }
        }
        false
    }

    /// Try to find a direct MCP tool match for the goal before decomposition.
    /// This avoids over-decomposition for simple API calls.
    async fn try_direct_mcp_match(
        &mut self,
        goal: &str,
    ) -> Result<Option<(String, HashMap<String, String>)>, Box<dyn Error + Send + Sync>> {
        println!("     🔍 Checking for direct MCP tool match...");

        let goal_lower = goal.to_lowercase();

        // First, extract the primary intent from the goal using LLM
        let primary_intent = self.extract_primary_intent(goal).await?;
        if let Some(ref intent) = primary_intent {
            println!(
                "     🎯 Primary intent: action='{}', object='{}'",
                intent.action, intent.object
            );
        }

        // Try to get MCP server from overrides
        // Use a generic hint like "github" to match the github MCP server
        let keywords: Vec<&str> = goal.split_whitespace().filter(|w| w.len() > 3).collect();

        // Check if any keyword matches an override
        let mut server_info: Option<(String, String)> = None;
        for keyword in &keywords {
            if let Some(info) = resolve_server_url_from_overrides(keyword) {
                server_info = Some(info);
                break;
            }
        }

        // Also try common patterns
        if server_info.is_none() {
            for pattern in &["github", "gitlab", "slack", "notion"] {
                if goal_lower.contains(pattern) {
                    if let Some(info) = resolve_server_url_from_overrides(pattern) {
                        server_info = Some(info);
                        break;
                    }
                }
            }
        }

        // Semantic inference: keywords that strongly suggest GitHub context
        if server_info.is_none() {
            let github_domain_keywords = [
                "issue",
                "issues",
                "repository",
                "repo",
                "pull request",
                "pr",
                "commit",
                "branch",
                "fork",
                "star",
                "gist",
                "release",
            ];
            for keyword in &github_domain_keywords {
                if goal_lower.contains(keyword) {
                    if let Some(info) = resolve_server_url_from_overrides("github") {
                        println!("     💡 Detected GitHub context from keyword '{}'", keyword);
                        server_info = Some(info);
                        break;
                    }
                }
            }
        }

        let (server_url, server_name) = match server_info {
            Some(info) => info,
            None => {
                println!("     ⚠️  No MCP server found for goal keywords");
                return Ok(None);
            }
        };

        // Get auth headers
        let auth_headers = get_mcp_auth_headers();

        // Try to discover tools from this server using the same pattern as try_real_mcp_discovery
        let session_manager = MCPSessionManager::new(auth_headers.clone());
        let client_info = MCPServerInfo {
            name: "ccos-autonomous-agent".to_string(),
            version: "1.0.0".to_string(),
        };

        let session = match session_manager
            .initialize_session(&server_url, &client_info)
            .await
        {
            Ok(s) => s,
            Err(e) => {
                println!("     ⚠️  Failed to initialize MCP session: {}", e);
                return Ok(None);
            }
        };

        // Call tools/list
        let tools_resp = match session_manager
            .make_request(&session, "tools/list", serde_json::json!({}))
            .await
        {
            Ok(r) => r,
            Err(e) => {
                println!("     ⚠️  Failed to list MCP tools: {}", e);
                return Ok(None);
            }
        };

        let empty_vec: Vec<serde_json::Value> = vec![];
        let tools_array = tools_resp
            .get("result")
            .and_then(|r| r.get("tools"))
            .and_then(|t| t.as_array())
            .unwrap_or(&empty_vec);

        // Score each tool against the goal using keyword + embedding matching
        let mut best_match: Option<(String, f64, serde_json::Value, String)> = None; // (name, score, schema, description)

        // Prepare goal text for embedding (expand to natural language)
        let goal_expanded = goal.replace('.', " ").replace('_', " ");

        for tool in tools_array {
            let tool_name = tool.get("name").and_then(|n| n.as_str()).unwrap_or("");
            let tool_description = tool
                .get("description")
                .and_then(|d| d.as_str())
                .unwrap_or("");

            // Compute embedding-based similarity if available
            let embedding_score = if let Some(ref mut emb_svc) = self.embedding_service {
                let tool_text = format!("{} {}", tool_name.replace('_', " "), tool_description);
                match (
                    emb_svc.embed(&goal_expanded).await,
                    emb_svc.embed(&tool_text).await,
                ) {
                    (Ok(goal_emb), Ok(tool_emb)) => {
                        let similarity = EmbeddingService::cosine_similarity(&goal_emb, &tool_emb);
                        // Scale similarity (0-1) to score range (0-5)
                        similarity * 5.0
                    }
                    _ => 0.0,
                }
            } else {
                0.0
            };

            // Combine keyword score with embedding score
            let keyword_score = compute_mcp_tool_score(goal, tool_name, tool_description);
            let score = keyword_score + embedding_score;

            if score > 3.0 {
                // Threshold for direct match
                if best_match.is_none() || score > best_match.as_ref().unwrap().1 {
                    best_match = Some((
                        tool_name.to_string(),
                        score,
                        tool.clone(),
                        tool_description.to_string(),
                    ));
                }
            }
        }

        if let Some((tool_name, score, _tool_schema, tool_description)) = best_match {
            println!(
                "     ✨ Found direct MCP match: {} (score: {:.2})",
                tool_name, score
            );

            // IMPORTANT: Validate the match against the primary intent
            // This prevents returning a completely unrelated tool (e.g., get_me when user asks about PRs)
            if let Some(ref intent) = primary_intent {
                if !self.validate_tool_against_intent(&tool_name, &tool_description, intent) {
                    println!(
                        "     ❌ Tool '{}' doesn't match primary intent (action='{}', object='{}')",
                        tool_name, intent.action, intent.object
                    );
                    println!("     🔄 Searching for better match among all tools...");

                    // Try to find a tool that actually matches the intent
                    let mut intent_match: Option<(String, f64, serde_json::Value)> = None;
                    for tool in tools_array {
                        let name = tool.get("name").and_then(|n| n.as_str()).unwrap_or("");
                        let desc = tool
                            .get("description")
                            .and_then(|d| d.as_str())
                            .unwrap_or("");

                        if self.validate_tool_against_intent(name, desc, intent) {
                            // Compute embedding-based similarity if available
                            let embedding_score = if let Some(ref mut emb_svc) =
                                self.embedding_service
                            {
                                let tool_text = format!("{} {}", name.replace('_', " "), desc);
                                match (
                                    emb_svc.embed(&goal_expanded).await,
                                    emb_svc.embed(&tool_text).await,
                                ) {
                                    (Ok(goal_emb), Ok(tool_emb)) => {
                                        EmbeddingService::cosine_similarity(&goal_emb, &tool_emb)
                                            * 5.0
                                    }
                                    _ => 0.0,
                                }
                            } else {
                                0.0
                            };

                            let s = compute_mcp_tool_score(goal, name, desc) + embedding_score;
                            if s > 2.0 {
                                if intent_match.is_none() || s > intent_match.as_ref().unwrap().1 {
                                    intent_match = Some((name.to_string(), s, tool.clone()));
                                }
                            }
                        }
                    }

                    if let Some((matched_name, matched_score, _)) = intent_match {
                        println!(
                            "     ✅ Found intent-matching tool: {} (score: {:.2})",
                            matched_name, matched_score
                        );
                        let cap_id = format!("mcp.{}.{}", server_name, matched_name);

                        // Check if already registered
                        if let Some(manifest) = self.marketplace.get_capability(&cap_id).await {
                            let args = self.extract_args_for_capability(&manifest, goal).await?;
                            return Ok(Some((cap_id, args)));
                        }

                        // Not registered yet - run full discovery
                        if let Ok(Some(manifest)) = self
                            .try_real_mcp_discovery(
                                &server_url,
                                auth_headers,
                                &matched_name,
                                &server_name,
                            )
                            .await
                        {
                            let args = self.extract_args_for_capability(&manifest, goal).await?;
                            return Ok(Some((manifest.id, args)));
                        }
                    } else {
                        println!("     ⚠️  No tool found matching intent. Cannot proceed with direct MCP match.");
                        println!(
                            "     💡 Available tools may not support '{}' on '{}'",
                            intent.action, intent.object
                        );
                        return Ok(None);
                    }
                    return Ok(None);
                }
            }

            let cap_id = format!("mcp.{}.{}", server_name, tool_name);

            // Check if already registered
            if let Some(manifest) = self.marketplace.get_capability(&cap_id).await {
                // Already registered, extract args with LLM
                let args = self.extract_args_for_capability(&manifest, goal).await?;
                return Ok(Some((cap_id, args)));
            }

            // Not registered yet - run full discovery to register it
            if let Ok(Some(manifest)) = self
                .try_real_mcp_discovery(&server_url, auth_headers, &tool_name, &server_name)
                .await
            {
                let args = self.extract_args_for_capability(&manifest, goal).await?;
                return Ok(Some((manifest.id, args)));
            }
        }

        println!("     ⚠️  No direct MCP tool match found (best score < 3.0)");
        Ok(None)
    }

    /// Extract the primary intent (action + object) from a goal using LLM
    /// This helps validate that a matched tool actually corresponds to the user's intent
    async fn extract_primary_intent(
        &self,
        goal: &str,
    ) -> Result<Option<PrimaryIntent>, Box<dyn Error + Send + Sync>> {
        let prompt = format!(
            r#"Analyze this goal and extract the PRIMARY action and object.

Goal: "{}"

Respond with ONLY a JSON object:
{{
  "action": "the main verb (list, check, get, create, search, etc.)",
  "object": "the main target/object (pull requests, issues, user, files, etc.)",
  "object_synonyms": ["alternative names for the object, e.g. 'PRs' for 'pull requests'"]
}}

Examples:
- "check PR in repository" → {{"action": "check", "object": "pull requests", "object_synonyms": ["PR", "PRs"]}}
- "list issues from repo" → {{"action": "list", "object": "issues", "object_synonyms": []}}
- "get my user info" → {{"action": "get", "object": "user", "object_synonyms": ["me", "profile"]}}
"#,
            goal
        );

        match self.arbiter.generate_raw_text(&prompt).await {
            Ok(response) => {
                let json_str = extract_json(&response);
                match serde_json::from_str::<PrimaryIntent>(json_str) {
                    Ok(intent) => Ok(Some(intent)),
                    Err(e) => {
                        eprintln!("     ⚠️  Failed to parse primary intent: {}", e);
                        Ok(None)
                    }
                }
            }
            Err(e) => {
                eprintln!("     ⚠️  Failed to extract primary intent: {}", e);
                Ok(None)
            }
        }
    }

    /// Validate that a matched tool actually corresponds to the primary intent
    /// Returns true if the match is valid, false if it's likely a mismatch
    fn validate_tool_against_intent(
        &self,
        tool_name: &str,
        tool_description: &str,
        intent: &PrimaryIntent,
    ) -> bool {
        let tool_lower = tool_name.to_lowercase();
        let desc_lower = tool_description.to_lowercase();
        let action_lower = intent.action.to_lowercase();
        let object_lower = intent.object.to_lowercase();

        // First, validate ACTION compatibility
        // Define read/query actions vs write/mutate actions
        let read_actions = [
            "check", "list", "get", "show", "view", "search", "find", "fetch", "read", "query",
            "display", "see", "retrieve",
        ];
        let write_actions = [
            "create", "add", "update", "delete", "remove", "merge", "push", "commit", "edit",
            "modify", "assign", "close", "open",
        ];

        let intent_is_read = read_actions.iter().any(|a| action_lower.contains(a));
        let intent_is_write = write_actions.iter().any(|a| action_lower.contains(a));

        // Check tool's action from its name/description
        let tool_is_read = tool_lower.starts_with("list_")
            || tool_lower.starts_with("get_")
            || tool_lower.starts_with("search_")
            || tool_lower.ends_with("_read")
            || desc_lower.contains("get ")
            || desc_lower.contains("list ")
            || desc_lower.contains("search ")
            || desc_lower.contains("retrieve");

        let tool_is_write = tool_lower.starts_with("create_")
            || tool_lower.starts_with("add_")
            || tool_lower.starts_with("update_")
            || tool_lower.starts_with("delete_")
            || tool_lower.starts_with("merge_")
            || tool_lower.starts_with("push_")
            || tool_lower.ends_with("_write")
            || desc_lower.contains("create ")
            || desc_lower.contains("add ")
            || desc_lower.contains("update ")
            || desc_lower.contains("delete ")
            || desc_lower.contains("merge ");

        // Reject read intent + write tool (and vice versa)
        if intent_is_read && tool_is_write && !tool_is_read {
            return false;
        }
        if intent_is_write && tool_is_read && !tool_is_write {
            return false;
        }

        // Normalize common synonyms
        let object_variants: Vec<String> = {
            let mut variants = vec![object_lower.clone()];
            // Add synonyms
            for syn in &intent.object_synonyms {
                variants.push(syn.to_lowercase());
            }
            // Add common variations
            if object_lower.contains("pull request")
                || object_lower == "pr"
                || object_lower == "prs"
            {
                variants.extend(
                    ["pull_request", "pull_requests", "pr", "prs"]
                        .iter()
                        .map(|s| s.to_string()),
                );
            }
            if object_lower == "issues" || object_lower == "issue" {
                variants.extend(["issue", "issues"].iter().map(|s| s.to_string()));
            }
            if object_lower == "user" || object_lower == "me" || object_lower == "profile" {
                variants.extend(
                    ["user", "me", "profile", "authenticated"]
                        .iter()
                        .map(|s| s.to_string()),
                );
            }
            variants
        };

        // Check if the tool name or description contains the object or its variants
        let object_match = object_variants
            .iter()
            .any(|variant| tool_lower.contains(variant) || desc_lower.contains(variant));

        // If object doesn't match at all, this is likely a wrong tool
        if !object_match {
            return false;
        }

        // Additional check: if intent is about PRs but tool is about issues (or vice versa), reject
        let intent_is_pr = object_variants
            .iter()
            .any(|v| v.contains("pull") || v == "pr" || v == "prs");
        let intent_is_issue = object_lower == "issue" || object_lower == "issues";
        let tool_is_pr = tool_lower.contains("pull") || tool_lower.contains("_pr");
        let tool_is_issue = tool_lower.contains("issue") && !tool_lower.contains("pull");

        if intent_is_pr && tool_is_issue && !tool_is_pr {
            return false;
        }
        if intent_is_issue && tool_is_pr && !tool_is_issue {
            return false;
        }

        true
    }

    /// Extract arguments for a capability based on the goal description
    async fn extract_args_for_capability(
        &self,
        capability: &CapabilityManifest,
        goal: &str,
    ) -> Result<HashMap<String, String>, Box<dyn Error + Send + Sync>> {
        // For MCP capabilities, parameters are stored in metadata as JSON schema
        let schema_json = capability
            .metadata
            .get("mcp_input_schema_json")
            .map(|s| s.as_str())
            .unwrap_or("");

        // If no schema available, just return empty args
        if schema_json.is_empty() {
            println!("     ℹ️  No input schema for argument extraction");
            return Ok(HashMap::new());
        }

        println!("     🔍 Extracting arguments from goal using LLM...");

        let prompt = format!(
            r#"Given this capability: {}
Description: {}
Input Schema (JSON): {}

And this goal: "{}"

Extract parameter values from the goal. IMPORTANT:
- Use the EXACT parameter names from the Input Schema above.
- Do NOT use synonyms or alternative names from the goal text.
- The parameter names in your response must match the schema exactly.
- Respond with ONLY a JSON object mapping schema parameter names to extracted values.
- If a parameter value cannot be determined from the goal, omit it.
"#,
            capability.id, &capability.description, schema_json, goal
        );

        let response = self.arbiter.generate_raw_text(&prompt).await?;
        let json_str = extract_json(&response);
        let args: HashMap<String, serde_json::Value> =
            serde_json::from_str(json_str).unwrap_or_default();

        // Convert to String values
        let string_args: HashMap<String, String> = args
            .into_iter()
            .map(|(k, v)| (k, v.to_string().trim_matches('"').to_string()))
            .collect();

        if !string_args.is_empty() {
            println!("     ✅ Extracted arguments: {:?}", string_args);
        }

        Ok(string_args)
    }

    async fn resolve_step(
        &mut self,
        step: &PlannedStep,
    ) -> Result<ResolutionStatus, Box<dyn Error + Send + Sync>> {
        // Resolution strategy:
        // 1. Check if hint matches any MCP server in overrides.json
        // 2. If yes, try MCP first
        // 3. Otherwise, try local first, then MCP as fallback

        let hint_has_mcp_override =
            resolve_server_url_from_overrides(&step.capability_hint).is_some();

        if hint_has_mcp_override {
            // Try MCP first for hints that match known MCP servers
            println!(
                "     🔍 Hint '{}' matches MCP override. Trying MCP first...",
                step.capability_hint
            );
            if let Some(mcp_result) = self.try_mcp_resolution(step).await? {
                return Ok(mcp_result);
            }
            println!("     ⚠️  MCP resolution failed. Falling back to local search...");
        }

        // A. Semantic Search (Local)
        let query = format!("{} {}", step.capability_hint, step.description);
        let filter = CatalogFilter::for_kind(CatalogEntryKind::Capability);
        let hits = self.catalog.search_semantic(&query, Some(&filter), 5);

        let mut local_candidates = Vec::new();
        for hit in hits {
            if let Some(cap) = self.marketplace.get_capability(&hit.entry.id).await {
                local_candidates.push(cap);
            }
        }

        // Try to select from local candidates
        if !local_candidates.is_empty() {
            if let Some((id, args)) = self
                .try_select_from_candidates(&local_candidates, &step.description)
                .await?
            {
                return Ok(ResolutionStatus::ResolvedLocal(id, args));
            }
            println!("     ⚠️  Local candidates rejected by LLM. Trying registry...");
        }

        // B. If no local (or rejected), try MCP Registry (Remote)
        if !hint_has_mcp_override {
            // Only try MCP here if we didn't already try it above
            println!(
                "     🔍 Searching MCP Registry for '{}' (Context: '{}')...",
                step.capability_hint, step.description
            );
            if let Some(mcp_result) = self.try_mcp_resolution(step).await? {
                return Ok(mcp_result);
            }
        }

        // C. If registry fails, consider synthesis but be conservative
        // Only synthesize if we can use safe stdlib functions
        println!(
            "     🧪 Registry failed. Checking if safe synthesis is possible for '{}'...",
            step.description
        );

        // Determine if this task requires external services/APIs (which we can't safely synthesize)
        let requires_external =
            self.check_requires_external_service(&step.description, &step.capability_hint);

        if requires_external {
            // This capability requires external services - request referral instead of broken synthesis
            return Ok(ResolutionStatus::NeedsReferral {
                description: step.description.clone(),
                reason: format!(
                    "This task requires external service access ({}). No matching MCP tool was found.",
                    step.capability_hint
                ),
                suggested_action: format!(
                    "Please provide a specific MCP server or capability for '{}', or manually configure the required service.",
                    step.capability_hint
                ),
            });
        }

        // Only synthesize for pure data transformations
        if let Some(synth_cap) = self
            .try_synthesize(&step.description, &step.capability_hint)
            .await?
        {
            println!("     ✨ Synthesized new capability: {}", synth_cap.id);
            // For synthesized capabilities, we need to extract arguments more intelligently
            // The synthesized function expects an 'args' map, so we need to construct it from the step description
            let synth_cap_id = synth_cap.id.clone();
            let candidates = vec![synth_cap];

            // Enhanced prompt for synthesized capabilities: they typically need data from previous steps
            let enhanced_goal = format!(
                 "{}. Note: This is a synthesized capability that operates on data. Extract any parameters from the description, but note that the actual data to process will come from previous step outputs.",
                 step.description
             );
            if let Some((id, args)) = self
                .try_select_from_candidates(&candidates, &enhanced_goal)
                .await?
            {
                return Ok(ResolutionStatus::ResolvedSynthesized(id, args));
            } else {
                // If LLM fails to extract args, use empty map (will fail at runtime, but that's Phase C issue)
                println!("     ⚠️  Could not extract arguments for synthesized capability. Using empty args (may fail at runtime).");
                return Ok(ResolutionStatus::ResolvedSynthesized(
                    synth_cap_id,
                    HashMap::new(),
                ));
            }
        }

        // D. If we are here, either no candidates found OR LLM rejected them.
        Ok(ResolutionStatus::NeedsSubPlan(
            step.description.clone(),
            step.capability_hint.clone(),
        ))
    }

    /// Check if a task description implies need for external service access
    /// Returns true if the task requires APIs, network calls, or external data sources
    fn check_requires_external_service(&self, description: &str, hint: &str) -> bool {
        let desc_lower = description.to_lowercase();
        let hint_lower = hint.to_lowercase();

        // Keywords that indicate external service requirements
        let external_keywords = [
            // API/HTTP patterns
            "api",
            "http",
            "https",
            "fetch",
            "request",
            "endpoint",
            "rest",
            "graphql",
            // Database patterns
            "database",
            "query",
            "sql",
            "mongodb",
            "postgres",
            "mysql",
            // Platform-specific patterns
            "github",
            "gitlab",
            "slack",
            "jira",
            "notion",
            "confluence",
            "aws",
            "azure",
            "gcp",
            // Authentication patterns
            "authenticate",
            "login",
            "credentials",
            "oauth",
            "token",
            // Network patterns
            "connect",
            "network",
            "remote",
            "server",
            "service",
            // File system patterns that need actual files
            "read file",
            "write file",
            "open file",
            "download",
            "upload",
        ];

        for keyword in &external_keywords {
            if desc_lower.contains(keyword) || hint_lower.contains(keyword) {
                return true;
            }
        }

        // Hint patterns that suggest external needs
        let external_hint_patterns = [
            "platform.",
            "system.",
            "access.",
            "service.",
            "auth.",
            "connect.",
            "network.",
            "data.query",
            "data.fetch",
        ];

        for pattern in &external_hint_patterns {
            if hint_lower.contains(pattern) {
                return true;
            }
        }

        false
    }

    async fn try_synthesize(
        &mut self,
        description: &str,
        hint: &str,
    ) -> Result<Option<CapabilityManifest>, Box<dyn Error + Send + Sync>> {
        // Phase B: Real LLM Synthesis
        println!(
            "     🧪 Synthesizing RTFS implementation for: {}",
            description
        );

        let prompt = format!(
            r#"You are an expert RTFS (Lisp-like) programmer.
Write an anonymous RTFS function (using `fn`) that performs the following task:
"{}"

The function will receive a SINGLE argument `args` which is a Map containing input parameters.
The function should return the result of the operation.

IMPORTANT - Available Functions:
- To parse a string to a number (integer/float), use `(tool/parse-json string_value)`. Example: `(tool/parse-json "123")` returns `123`.
- To convert to string, use `(str value)`.
- Use `int?` to check for integers, `float?` for floats, `string?` for strings, `map?` for maps. DO NOT use `integer?` or `is-integer`.
- Use standard Lisp/Clojure-like functions: `map`, `filter`, `reduce`, `get`, `first`, `rest`, `nth`, `keys`, `vals`, `assoc`, `dissoc`, `conj`, `concat`, `contains?`, `count`.

Examples:
- Task: "Filter items to keep only active ones"
  Code: (fn [args] (filter (fn [item] (= (:status item) "active")) (:items args)))

- Task: "Extract the 'id' field from a list of objects"
  Code: (fn [args] (map (fn [item] (:id item)) (:data args)))

- Task: "Search for text containing a substring"
  Code: (fn [args] (filter (fn [item] (contains? (:text item) (:query args))) (:items args)))

- Task: "Parse page size from input string"
  Code: (fn [args] (tool/parse-json (get args "pageSize")))

Respond ONLY with the valid RTFS code for the function. Do not add markdown formatting, code fences, or explanation.
"#,
            description
        );

        let generated_code = self.arbiter.generate_raw_text(&prompt).await?;

        // Clean up: remove markdown code fences, trim whitespace
        let mut clean_code = generated_code.trim();
        if clean_code.starts_with("```") {
            // Remove opening fence (may have language tag like ```rtfs or ```lisp)
            if let Some(end) = clean_code.find('\n') {
                clean_code = &clean_code[end + 1..];
            } else {
                clean_code = &clean_code[3..];
            }
        }
        if clean_code.ends_with("```") {
            clean_code = &clean_code[..clean_code.len() - 3];
        }
        clean_code = clean_code.trim().trim_matches('`').trim();

        // Remove language tags if present
        if clean_code.starts_with("rtfs") || clean_code.starts_with("lisp") {
            if let Some(space) = clean_code.find(' ') {
                clean_code = &clean_code[space + 1..];
            }
        }
        clean_code = clean_code.trim();

        println!("     📝 Generated Code: {}", clean_code);

        // Validate: Try parsing the code to catch syntax errors early
        // We only check if the code parses, not if it executes successfully
        // Wrap in parentheses to make it a valid expression
        let test_parse = format!("({})", clean_code);
        let mut final_code = clean_code.to_string();
        let mut parse_valid = rtfs::parser::parse(&test_parse).is_ok();
        let mut repair_attempts = 0;
        const MAX_REPAIR_ATTEMPTS: usize = 2;

        // Auto-repair loop: if parse fails, ask LLM to fix it
        while !parse_valid && !final_code.is_empty() && repair_attempts < MAX_REPAIR_ATTEMPTS {
            repair_attempts += 1;
            println!(
                "     🔧 Parse validation failed. Attempting auto-repair (attempt {}/{})...",
                repair_attempts, MAX_REPAIR_ATTEMPTS
            );

            let repair_prompt = format!(
                r#"The following RTFS code failed to parse. Please fix the syntax errors and return ONLY the corrected code.

Original code:
{}
Error: Parse validation failed

Task description: "{}"

Respond ONLY with the corrected RTFS function code. Do not add markdown formatting or explanation.
"#,
                final_code, description
            );

            let repaired_code = self.arbiter.generate_raw_text(&repair_prompt).await?;
            let mut repaired_clean = repaired_code.trim();

            // Clean up repaired code
            if repaired_clean.starts_with("```") {
                if let Some(end) = repaired_clean.find('\n') {
                    repaired_clean = &repaired_clean[end + 1..];
                } else {
                    repaired_clean = &repaired_clean[3..];
                }
            }
            if repaired_clean.ends_with("```") {
                repaired_clean = &repaired_clean[..repaired_clean.len() - 3];
            }
            repaired_clean = repaired_clean.trim().trim_matches('`').trim();

            if repaired_clean.starts_with("rtfs") || repaired_clean.starts_with("lisp") {
                if let Some(space) = repaired_clean.find(' ') {
                    repaired_clean = &repaired_clean[space + 1..];
                }
            }
            repaired_clean = repaired_clean.trim();

            if !repaired_clean.is_empty() && repaired_clean != final_code {
                final_code = repaired_clean.to_string();
                println!("     📝 Repaired Code: {}", final_code);

                // Test the repaired code: just check if it parses
                let test_parse_repaired = format!("({})", final_code);
                parse_valid = rtfs::parser::parse(&test_parse_repaired).is_ok();

                if parse_valid {
                    println!("     ✅ Auto-repair succeeded!");
                }
            } else {
                println!("     ⚠️  LLM returned same code or empty. Stopping repair attempts.");
                break;
            }
        }

        if !parse_valid && !final_code.is_empty() {
            println!("     ⚠️  Code still fails parse validation after {} repair attempts, but proceeding anyway...", repair_attempts);
        }

        // Fallback if LLM refuses or returns empty (for demo stability)
        if final_code.is_empty()
            || final_code.contains("I cannot")
            || final_code.contains("I'm sorry")
        {
            println!("     ⚠️  LLM failed to synthesize code. Using fallback mock.");
            final_code = "(fn [args] (str \"Fallback Result for: \" args))".to_string();
        }

        let id = format!(
            "ccos.synthesized.{}",
            hint.replace(".", "_").replace(" ", "_")
        );

        self.trace.decisions.push(TraceEvent::Synthesis {
            capability: id.clone(),
            success: !final_code.contains("Fallback"),
        });

        let code_clone = final_code.clone();

        // Save synthesized capability to file for inspection
        save_synthesized_capability(&id, &final_code, description, hint);

        // Register a handler that executes this code dynamically
        if self.marketplace.get_capability(&id).await.is_none() {
            let id_for_handler = id.clone();
            let handler = Arc::new(move |args: &Value| -> RuntimeResult<Value> {
                println!(
                    "     [{}] 🧪 Executing synthesized RTFS code...",
                    id_for_handler
                );

                // Instantiate a fresh ephemeral runtime for this execution
                let module_registry = Arc::new(rtfs::runtime::ModuleRegistry::new());
                let runtime =
                    rtfs::runtime::Runtime::new_with_tree_walking_strategy(module_registry);

                // The marketplace wraps our args as {:args [...], :context ...}
                // Extract the actual args from the :args key, or use raw args if not wrapped
                let actual_args = match args {
                    Value::Map(m) => {
                        // Check if it's the marketplace wrapper format {:args [...]}
                        if let Some(inner) = m.get(&rtfs::ast::MapKey::Keyword(rtfs::ast::Keyword(
                            "args".to_string(),
                        ))) {
                            // If it's a list, we need to decide if we want the first element or the list itself
                            // The previous code assumed list[0] if len == 1, but for synthesized caps expecting a map, this might be right.
                            // However, if the synthesis expects specific keys, we might need to handle it better.
                            match inner {
                                Value::List(list) if list.len() == 1 => list[0].clone(),
                                Value::List(list) if list.is_empty() => {
                                    Value::Map(std::collections::HashMap::new())
                                }
                                // If multiple args, wrap them in a map with indices or similar?
                                // For now, let's just pass the inner value if it's not a single-element list
                                _ => inner.clone(),
                            }
                        } else {
                            // If no :args key, check if we have the arguments directly in the map
                            // This happens when the plan calls the capability with named args like {:data step_2 :min 1}
                            // In this case, 'args' IS the map of arguments.
                            args.clone()
                        }
                    }
                    _ => args.clone(),
                };

                // DEBUG: Print actual args to help debug "got nil" errors
                println!("     🐛 Synthesized execution args: {:?}", actual_args);

                // Convert Value to RTFS literal representation
                let args_rtfs = value_to_rtfs_literal(&actual_args);

                // Construct the program: (fn_code arg) - NOT ((fn_code) arg) which would call fn_code with 0 args first
                let program = format!("({} {})", code_clone, args_rtfs);

                match runtime.evaluate(&program) {
                    Ok(v) => {
                        println!("     ✅ Synthesis execution succeeded");
                        Ok(v)
                    }
                    Err(e) => {
                        println!("     ❌ Synthesis Execution Error: {}", e);
                        // Return error but don't crash - let the planner handle it
                        Err(e)
                    }
                }
            });

            let _ = self
                .marketplace
                .register_local_capability(
                    id.clone(),
                    format!("Synthesized: {}", hint),
                    description.to_string(),
                    handler,
                )
                .await;
        }

        return Ok(self.marketplace.get_capability(&id).await);
    }

    fn step_implies_context_dependency(&self, description: &str) -> bool {
        let lower = description.to_lowercase();
        lower.contains("using the")
            || lower.contains("from previous")
            || lower.contains("with the")
            || lower.contains("based on")
            || lower.contains("specified")
            || lower.contains("result of")
    }

    async fn synthesize_adapter_with_context(
        &self,
        context_vars: &[String],
        target_capability_id: &str,
        target_description: &str,
        known_args: &HashMap<String, String>,
    ) -> Result<String, Box<dyn Error + Send + Sync>> {
        println!(
            "     🔌 Synthesizing context-aware data adapter for '{}'...",
            target_capability_id
        );

        // Fetch capability to get schema
        let schema_info =
            if let Some(cap) = self.marketplace.get_capability(target_capability_id).await {
                cap.metadata
                    .get("mcp_input_schema_json")
                    .cloned()
                    .unwrap_or_default()
            } else {
                String::new()
            };

        let schema_section = if !schema_info.is_empty() {
            format!("Target Tool Input Schema (JSON): {}\nIMPORTANT: Use ONLY parameter names defined in this schema.", schema_info)
        } else {
            String::new()
        };

        // Build context map description for the prompt
        let context_desc = if context_vars.len() == 1 {
            format!(
                "The variable '{}' contains the result from the previous step.",
                context_vars[0]
            )
        } else {
            let vars_list: Vec<String> = context_vars.iter().map(|v| format!("'{}'", v)).collect();
            format!(
                "Variables {} contain results from previous steps (in order).",
                vars_list.join(", ")
            )
        };

        // Determine which context variable(s) to use
        let prev_var = context_vars.last().unwrap(); // Most recent by default
        let all_vars_available = context_vars.join(", ");

        // If the previous step was a user prompt, the result is a simple string, not a map.
        // The context adapter needs to know this to avoid trying to extract fields from a string.
        let context_hint = if context_vars.len() > 0 {
            "Note: If the previous step was a user prompt (ccos.user.ask), the variable contains a simple string value (the user's input)."
        } else {
            ""
        };

        if self.simulate_error {
            println!("     ⚠️  SIMULATING ERROR: Generating broken adapter code...");
            return Ok("(call \"non_existent_function_to_force_crash\" {})".to_string());
        }

        let known_args_info = if !known_args.is_empty() {
            format!("Known static arguments (extracted from goal): {:?}\nInclude these in the call unless overridden by context.", known_args)
        } else {
            String::new()
        };

        let prompt = format!(
            r#"You are an expert RTFS programmer. Generate ONLY the RTFS code to call '{}' with data from previous steps.

Task: {}
{}
{}
{}
{}

Available context variables: {}
Most recent result is in: {}

If the task requires extracting specific fields (e.g., "get the ID from the first item"), generate code that:
1. Accesses the appropriate context variable
2. Extracts the required data (use 'get', 'first', 'nth', etc.)
3. Passes it to the capability call along with any known static arguments.

Examples:
- Simple pass-through: (call "{}" {{:data {}}})
- Extract field from map: (call "{}" {{:query (get {} :field_name)}})
- Use string input directly: (call "{}" {{:search_term {}}})
- First item ID: (call "{}" {{:id (get (first (get {} :items)) :id)}})
- Filter operation: (call "{}" {{:data {} :filter "condition"}})
- Mixed args: (call "{}" {{:static "value" :dynamic (get {} :key)}})

Respond with ONLY the RTFS expression - no markdown, no explanation.
"#,
            target_capability_id,
            target_description,
            schema_section,
            context_desc,
            known_args_info,
            context_hint,
            all_vars_available,
            prev_var,
            target_capability_id,
            prev_var,
            target_capability_id,
            prev_var,
            target_capability_id,
            prev_var,
            target_capability_id,
            prev_var,
            target_capability_id,
            prev_var,
            target_capability_id,
            prev_var
        );

        let generated_code = self.arbiter.generate_raw_text(&prompt).await?;

        // Clean up code
        let mut clean_code = generated_code.trim();
        if clean_code.starts_with("```") {
            if let Some(end) = clean_code.find('\n') {
                clean_code = &clean_code[end + 1..];
            } else {
                clean_code = &clean_code[3..];
            }
        }
        if clean_code.ends_with("```") {
            clean_code = &clean_code[..clean_code.len() - 3];
        }
        clean_code = clean_code.trim().trim_matches('`').trim();

        // Remove language tags
        if clean_code.starts_with("rtfs")
            || clean_code.starts_with("lisp")
            || clean_code.starts_with("clojure")
        {
            if let Some(space) = clean_code.find(' ') {
                clean_code = &clean_code[space + 1..];
            }
        }
        clean_code = clean_code.trim();

        Ok(clean_code.to_string())
    }

    async fn synthesize_adapter(
        &self,
        prev_var: &str,
        target_capability_id: &str,
        target_description: &str,
    ) -> Result<String, Box<dyn Error + Send + Sync>> {
        // Legacy single-variable adapter - delegate to context-aware version with empty known args
        self.synthesize_adapter_with_context(
            &[prev_var.to_string()],
            target_capability_id,
            target_description,
            &HashMap::new(),
        )
        .await
    }

    #[allow(dead_code)]
    async fn synthesize_adapter_old(
        &self,
        prev_var: &str,
        target_capability_id: &str,
        target_description: &str,
    ) -> Result<String, Box<dyn Error + Send + Sync>> {
        println!(
            "     🔌 Synthesizing data adapter for '{}'...",
            target_capability_id
        );

        if self.simulate_error {
            println!("     ⚠️  SIMULATING ERROR: Generating broken adapter code...");
            return Ok("(call \"non_existent_function_to_force_crash\" {})".to_string());
        }

        let prompt = format!(
            r#"You are an expert RTFS programmer.
We need to pass data from a previous step (variable `{}`) to a tool named `{}`.
The tool's description is: "{}".

Write an RTFS expression that calls the tool with the correct arguments, using the data from `{}`.
The previous step output is likely a Map or List. You may need to wrap it in a map with a specific key (e.g. :data, :items, :records).

Examples:
- Tool: "filter_records", Input: `step_1` (List of records)
  Expression: (call "filter_records" {{:records step_1}})

- Tool: "summarize_text", Input: `step_1` (String)
  Expression: (call "summarize_text" {{:text step_1}})

- Tool: "process_data", Input: `step_1` (Map)
  Expression: (call "process_data" step_1)

Respond ONLY with the RTFS expression.
"#,
            prev_var, target_capability_id, target_description, prev_var
        );

        let generated_code = self.arbiter.generate_raw_text(&prompt).await?;

        // Clean up code
        let mut clean_code = generated_code.trim();
        if clean_code.starts_with("```") {
            if let Some(end) = clean_code.find('\n') {
                clean_code = &clean_code[end + 1..];
            } else {
                clean_code = &clean_code[3..];
            }
        }
        if clean_code.ends_with("```") {
            clean_code = &clean_code[..clean_code.len() - 3];
        }
        clean_code = clean_code.trim().trim_matches('`').trim();

        Ok(clean_code.to_string())
    }

    async fn try_select_from_candidates(
        &self,
        candidates: &[CapabilityManifest],
        goal: &str,
    ) -> Result<Option<(String, HashMap<String, String>)>, Box<dyn Error + Send + Sync>> {
        let tool_descriptions: Vec<String> = candidates
            .iter()
            .map(|c| format!("{}: {}", c.id, c.description))
            .collect();

        self.select_tool_robust(goal, &tool_descriptions).await
    }

    /// Try to resolve a step using MCP registry/server
    async fn try_mcp_resolution(
        &mut self,
        step: &PlannedStep,
    ) -> Result<Option<ResolutionStatus>, Box<dyn Error + Send + Sync>> {
        println!(
            "     🔍 Searching MCP Registry for '{}' (Context: '{}')...",
            step.capability_hint, step.description
        );

        if let Some(installed_cap) = self
            .try_install_from_registry(&step.capability_hint, &step.description)
            .await?
        {
            println!("     📦 Found MCP capability: {}", installed_cap.id);
            let remote_candidates = vec![installed_cap];
            if let Some((id, args)) = self
                .try_select_from_candidates(&remote_candidates, &step.description)
                .await?
            {
                return Ok(Some(ResolutionStatus::ResolvedRemote(id, args)));
            } else {
                println!("     ⚠️  MCP capability rejected by LLM.");
            }
        } else {
            println!("     ❌ MCP Registry search returned no results.");
        }

        Ok(None)
    }

    async fn try_install_from_registry(
        &mut self,
        hint: &str,
        description: &str,
    ) -> Result<Option<CapabilityManifest>, Box<dyn Error + Send + Sync>> {
        let client = MCPRegistryClient::new();
        let search_query = if hint.contains(".") {
            hint
        } else {
            description
        };
        let servers = client
            .search_servers(search_query)
            .await
            .unwrap_or_default();

        // Try to find a matching MCP server configuration
        let mcp_server_config = self.find_mcp_server_config(hint, &servers);

        if let Some((server_url, auth_headers, server_name)) = mcp_server_config {
            // Attempt real MCP discovery
            println!("     🔌 Attempting real MCP connection to: {}", server_url);
            match self
                .try_real_mcp_discovery(&server_url, auth_headers, hint, &server_name)
                .await
            {
                Ok(Some(manifest)) => {
                    println!("     ✅ Real MCP capability discovered: {}", manifest.id);
                    self.trace.decisions.push(TraceEvent::MCPDiscovery {
                        hint: hint.to_string(),
                        found: true,
                    });
                    return Ok(Some(manifest));
                }
                Ok(None) => {
                    println!("     ⚠️  Real MCP connection succeeded but tool not found");
                }
                Err(e) => {
                    println!(
                        "     ⚠️  Real MCP connection failed: {}. Falling back to mock.",
                        e
                    );
                }
            }
        }

        // Fallback to generic mock if real MCP fails or no server config found
        // Only use mock if --allow-mock flag is explicitly set
        if !self.allow_mock {
            println!(
                "     🚫 Mock fallback disabled (use --allow-mock to enable). Will try synthesis."
            );
            self.trace.decisions.push(TraceEvent::MCPDiscovery {
                hint: hint.to_string(),
                found: false,
            });
            return Ok(None);
        }

        let should_install = !servers.is_empty() || hint.starts_with("mcp.") || hint.contains(".");

        if should_install {
            let cap_id = if hint.contains(".") {
                hint.to_string()
            } else {
                format!("mcp.{}", hint.replace(" ", "_"))
            };

            if self.marketplace.get_capability(&cap_id).await.is_some() {
                return Ok(self.marketplace.get_capability(&cap_id).await);
            }

            self.trace.decisions.push(TraceEvent::MCPDiscovery {
                hint: hint.to_string(),
                found: true,
            });
            println!(
                "     🌐 [Demo] Installing generic mock capability: {}",
                cap_id
            );

            self.install_generic_mock_capability(&cap_id, description)
                .await?;

            return Ok(self.marketplace.get_capability(&cap_id).await);
        }

        self.trace.decisions.push(TraceEvent::MCPDiscovery {
            hint: hint.to_string(),
            found: false,
        });
        Ok(None)
    }

    /// Find a matching MCP server configuration from agent config or overrides
    /// Returns (url, auth_headers, server_name)
    fn find_mcp_server_config(
        &self,
        hint: &str,
        servers: &[ccos::mcp::registry::McpServer],
    ) -> Option<(
        String,
        Option<std::collections::HashMap<String, String>>,
        String,
    )> {
        // 1. First, check overrides.json for matching server
        if let Some((server_url, server_name)) = resolve_server_url_from_overrides(hint) {
            println!(
                "     📁 Found MCP server in overrides: {} (namespace: {})",
                server_url, server_name
            );
            let auth_headers = get_mcp_auth_headers();
            return Some((server_url, auth_headers, server_name));
        }

        // 2. Check if any server from registry has a usable remote URL
        for server in servers {
            if let Some(remotes) = &server.remotes {
                if let Some(url) =
                    ccos::mcp::registry::MCPRegistryClient::select_best_remote_url(remotes)
                {
                    println!(
                        "     🌐 Found MCP server in registry: {} ({})",
                        server.name, url
                    );
                    let auth_headers = get_mcp_auth_headers();
                    // Use server name from registry
                    return Some((url, auth_headers, server.name.clone()));
                }
            }
        }

        // 3. Fallback: check environment variable for explicit endpoint
        // Match GitHub-related hints: github, repository, issue, pull_request, pr, commit, branch, etc.
        let hint_lower = hint.to_ascii_lowercase();
        let is_github_related = hint_lower.contains("github")
            || hint_lower.contains("repository")
            || hint_lower.contains("repo")
            || hint_lower.contains("issue")
            || hint_lower.contains("pull_request")
            || hint_lower.contains("pull-request")
            || hint_lower.starts_with("pr.")
            || hint_lower.contains(".pr")
            || hint_lower.contains("commit")
            || hint_lower.contains("branch");

        if is_github_related {
            if let Ok(endpoint) = std::env::var("GITHUB_MCP_ENDPOINT") {
                println!(
                    "     🔧 Using GITHUB_MCP_ENDPOINT from environment: {}",
                    endpoint
                );
                let auth_headers = get_mcp_auth_headers();
                return Some((endpoint, auth_headers, "github".to_string()));
            }
        }

        None
    }

    /// Try to discover and install a capability from a real MCP server
    async fn try_real_mcp_discovery(
        &mut self,
        server_url: &str,
        auth_headers: Option<std::collections::HashMap<String, String>>,
        hint: &str,
        server_name: &str,
    ) -> Result<Option<CapabilityManifest>, Box<dyn Error + Send + Sync>> {
        // Clone auth_headers for later use in output schema introspection
        let auth_headers_for_introspection = auth_headers.clone();

        // Create session manager and initialize session (like single_mcp_discovery.rs)
        let session_manager = MCPSessionManager::new(auth_headers);
        let client_info = MCPServerInfo {
            name: "ccos-autonomous-agent".to_string(),
            version: "1.0.0".to_string(),
        };

        let session = session_manager
            .initialize_session(server_url, &client_info)
            .await
            .map_err(|e| format!("MCP initialization failed: {}", e))?;

        // Call tools/list on the session
        let tools_resp = session_manager
            .make_request(&session, "tools/list", serde_json::json!({}))
            .await
            .map_err(|e| format!("MCP tools/list failed: {}", e))?;

        // Parse tools array
        let tools_array = tools_resp
            .get("result")
            .and_then(|r| r.get("tools"))
            .and_then(|t| t.as_array())
            .ok_or_else(|| "Invalid MCP tools/list response — no tools array")?;

        println!("     🔍 Found {} tools from MCP server", tools_array.len());

        // Build list of tool candidates with scores using embedding + keyword matching
        let mut candidates: Vec<(f64, String, serde_json::Value)> = Vec::new();

        // Prepare query for embedding (expand hint to natural language)
        let hint_expanded = hint.replace('.', " ").replace('_', " ");

        for tool_json in tools_array {
            let tool_name = tool_json
                .get("name")
                .and_then(|n| n.as_str())
                .unwrap_or("unknown")
                .to_string();
            let description = tool_json
                .get("description")
                .and_then(|d| d.as_str())
                .unwrap_or("");

            // Compute embedding-based similarity if available
            let embedding_score = if let Some(ref mut emb_svc) = self.embedding_service {
                let tool_text = format!("{} {}", tool_name.replace('_', " "), description);
                match (
                    emb_svc.embed(&hint_expanded).await,
                    emb_svc.embed(&tool_text).await,
                ) {
                    (Ok(hint_emb), Ok(tool_emb)) => {
                        let similarity = EmbeddingService::cosine_similarity(&hint_emb, &tool_emb);
                        // Scale similarity (0-1) to score range (0-5)
                        similarity * 5.0
                    }
                    _ => 0.0,
                }
            } else {
                0.0
            };

            // Combine keyword score with embedding score
            let keyword_score = compute_mcp_tool_score(hint, &tool_name, description);
            let combined_score = keyword_score + embedding_score;

            if combined_score > 0.0 {
                candidates.push((combined_score, tool_name, tool_json.clone()));
            }
        }

        // Sort by score (descending) and take best match
        candidates.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

        // Log top candidates for debugging
        if !candidates.is_empty() {
            println!("     📊 Top MCP tool candidates:");
            for (score, name, _) in candidates.iter().take(3) {
                println!("        - {} (score: {:.2})", name, score);
            }
        }

        // Select best match if score is above threshold (>= 3.0 or overlap >= 0.75)
        if let Some((score, tool_name, tool_json)) = candidates.first() {
            let overlap = keyword_overlap(hint, tool_name);
            if *score >= 3.0 || overlap >= 0.75 {
                println!(
                    "     ✅ Matched MCP tool: {} (score: {:.2}, overlap: {:.2})",
                    tool_name, score, overlap
                );

                // Convert to DiscoveredMCPTool
                let description = tool_json
                    .get("description")
                    .and_then(|d| d.as_str())
                    .map(String::from);
                let input_schema_json = tool_json.get("inputSchema").cloned();
                let input_schema = input_schema_json
                    .as_ref()
                    .and_then(|s| MCPIntrospector::type_expr_from_json_schema(s).ok());

                let introspector = MCPIntrospector::new();
                let mut discovered_tool = DiscoveredMCPTool {
                    tool_name: tool_name.clone(),
                    description: description.clone(),
                    input_schema,
                    output_schema: None,
                    input_schema_json,
                };

                // Try to introspect output schema by calling the tool once with safe inputs
                // This uses the same approach as single_mcp_discovery.rs
                let (output_schema, sample_output) = match introspector
                    .introspect_output_schema(
                        &discovered_tool,
                        server_url,
                        server_name,
                        auth_headers_for_introspection.clone(),
                        None, // No input overrides - will use safe defaults
                    )
                    .await
                {
                    Ok((schema, sample)) => {
                        if schema.is_some() {
                            println!("     📊 Inferred output schema from live call");
                        }
                        (schema, sample)
                    }
                    Err(e) => {
                        eprintln!("     ⚠️  Output schema introspection failed: {}", e);
                        (None, None)
                    }
                };

                // Update discovered_tool with inferred output schema
                discovered_tool.output_schema = output_schema;

                // Create capability manifest using the server name from overrides/config
                let introspection_result =
                    ccos::synthesis::mcp_introspector::MCPIntrospectionResult {
                        server_url: server_url.to_string(),
                        server_name: server_name.to_string(),
                        protocol_version: session.protocol_version.clone(),
                        tools: vec![discovered_tool],
                    };

                let mut manifest = introspector
                    .create_capability_from_mcp_tool(
                        &introspection_result.tools[0],
                        &introspection_result,
                    )
                    .map_err(|e| format!("Failed to create manifest: {}", e))?;

                // Update manifest with the inferred output schema
                if let Some(ref schema) = introspection_result.tools[0].output_schema {
                    manifest.output_schema = Some(schema.clone());
                }

                // Save discovered MCP capability using MCPIntrospector (like single_mcp_discovery.rs)
                let implementation_code = introspector
                    .generate_mcp_rtfs_implementation(&introspection_result.tools[0], server_url);
                let output_dir = get_capabilities_discovered_dir();
                match introspector.save_capability_to_rtfs(
                    &manifest,
                    &implementation_code,
                    &output_dir,
                    sample_output.as_deref(),
                ) {
                    Ok(path) => println!(
                        "     💾 Saved discovered MCP capability to: {}",
                        path.display()
                    ),
                    Err(e) => eprintln!("     ⚠️  Failed to save MCP capability: {}", e),
                }

                // Register the MCP capability in the marketplace for execution
                // This ensures the MCPExecutor will handle execution via tools/call
                if let Err(e) = self
                    .marketplace
                    .register_capability_manifest(manifest.clone())
                    .await
                {
                    eprintln!("     ⚠️  Failed to register MCP capability: {}", e);
                } else {
                    println!(
                        "     📦 Registered MCP capability in marketplace: {}",
                        manifest.id
                    );
                }

                return Ok(Some(manifest));
            } else {
                println!(
                    "     ⚠️  Best match '{}' below threshold (score: {:.2}, overlap: {:.2})",
                    tool_name, score, overlap
                );
            }
        }

        Ok(None)
    }

    async fn install_generic_mock_capability(
        &self,
        id: &str,
        description: &str,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        println!("     🛠️  Generating generic mock data for '{}'...", id);

        let prompt = format!(
            r#"Generate a sample JSON return value for a tool named "{}" which has the description: "{}".
            
            If the tool implies returning a list (e.g. "list_items", "get_records", "search_results"), return a JSON Array with 2-3 realistic items.
            If it implies a single object, return a JSON Object.
            
            Respond ONLY with the JSON.
            "#,
            id, description
        );

        let json_str = self.arbiter.generate_raw_text(&prompt).await?;
        let clean_json = extract_json(&json_str);

        let sample_data: serde_json::Value = serde_json::from_str(clean_json).unwrap_or(serde_json::json!({"status": "mocked", "message": "LLM failed to generate sample data"}));

        let rtfs_value = json_to_rtfs_value(sample_data);

        let handler =
            Arc::new(move |_args: &Value| -> RuntimeResult<Value> { Ok(rtfs_value.clone()) });

        self.marketplace
            .register_local_capability(
                id.to_string(),
                format!("Mock: {}", id),
                description.to_string(),
                handler,
            )
            .await?;

        Ok(())
    }

    async fn select_tool_robust(
        &self,
        goal: &str,
        tools: &[String],
    ) -> Result<Option<(String, HashMap<String, String>)>, Box<dyn Error + Send + Sync>> {
        let prompt = format!(
            r#"You are an expert tool selector.
Goal: "{}"

Available Tools (Format: "ToolID: Description"):
{}

Select the tool ID that BEST matches the goal.
If NONE of the tools are a good match, respond with "NO_MATCH".

If you select a tool, extract the arguments from the goal.
Respond in this JSON format:
{{
  "tool": "ToolID_or_NO_MATCH",
  "arguments": {{ "arg1": "value1" }}
}}
"#,
            goal,
            tools.join("\n")
        );

        let response = self.arbiter.generate_raw_text(&prompt).await?;
        let json_str = extract_json(&response);

        #[derive(serde::Deserialize)]
        struct Selection {
            tool: String,
            arguments: Option<HashMap<String, String>>,
        }

        let selection: Selection = serde_json::from_str(json_str)?;

        if selection.tool == "NO_MATCH" {
            Ok(None)
        } else {
            Ok(Some((
                selection.tool,
                selection.arguments.unwrap_or_default(),
            )))
        }
    }

    async fn repair_plan(
        &self,
        plan: &str,
        error: &str,
    ) -> Result<String, Box<dyn Error + Send + Sync>> {
        println!("     🔧 Plan execution failed. Attempting to repair...");

        let prompt = format!(
            r#"The following RTFS plan failed to execute.
Error: {}

Plan:
{}

Please fix the plan to resolve the error.
Respond ONLY with the corrected RTFS code. Do not add markdown formatting or explanation.
"#,
            error, plan
        );

        let generated_code = self.arbiter.generate_raw_text(&prompt).await?;

        // Clean up code (similar to try_synthesize)
        let mut clean_code = generated_code.trim();
        if clean_code.starts_with("```") {
            if let Some(end) = clean_code.find('\n') {
                clean_code = &clean_code[end + 1..];
            } else {
                clean_code = &clean_code[3..];
            }
        }
        if clean_code.ends_with("```") {
            clean_code = &clean_code[..clean_code.len() - 3];
        }
        clean_code = clean_code.trim().trim_matches('`').trim();

        if clean_code.starts_with("rtfs") || clean_code.starts_with("lisp") {
            if let Some(space) = clean_code.find(' ') {
                clean_code = &clean_code[space + 1..];
            }
        }

        Ok(clean_code.trim().to_string())
    }

    fn generate_call(&self, capability_id: &str, args: HashMap<String, String>) -> String {
        let args_str = args
            .iter()
            .map(|(k, v)| format!(":{} \"{}\"", k, v))
            .collect::<Vec<_>>()
            .join(" ");

        format!("(call \"{}\" {{{}}})", capability_id, args_str)
    }
}

// ============================================================================
// Utils
// ============================================================================

/// Convert a RTFS Value to its RTFS literal representation
fn value_to_rtfs_literal(value: &Value) -> String {
    use rtfs::ast::MapKey;
    match value {
        Value::Nil => "nil".to_string(),
        Value::String(s) => format!("\"{}\"", s.replace("\"", "\\\"").replace("\n", "\\n")),
        Value::Integer(i) => i.to_string(),
        Value::Float(f) => {
            if f.fract() == 0.0 {
                (*f as i64).to_string()
            } else {
                f.to_string()
            }
        }
        Value::Boolean(b) => b.to_string(),
        Value::Vector(v) => {
            let items: Vec<String> = v.iter().map(value_to_rtfs_literal).collect();
            format!("[{}]", items.join(" "))
        }
        Value::Map(m) => {
            let pairs: Vec<String> = m
                .iter()
                .map(|(k, v)| {
                    let key_str = match k {
                        MapKey::String(s) => format!("\"{}\"", s),
                        MapKey::Keyword(kw) => format!(":{}", kw.0),
                        MapKey::Integer(i) => i.to_string(),
                    };
                    format!("{} {}", key_str, value_to_rtfs_literal(v))
                })
                .collect();
            format!("{{{}}}", pairs.join(" "))
        }
        Value::List(l) => {
            let items: Vec<String> = l.iter().map(value_to_rtfs_literal).collect();
            format!("({})", items.join(" "))
        }
        Value::Symbol(s) => s.0.clone(),
        Value::Keyword(k) => format!(":{}", k.0),
        Value::Timestamp(t) => format!("\"{}\"", t),
        Value::Uuid(u) => format!("\"{}\"", u),
        Value::ResourceHandle(r) => format!("\"{}\"", r),
        Value::Function(_) => "#<function>".to_string(),
        Value::FunctionPlaceholder(_) => "#<function-placeholder>".to_string(),
        Value::Error(e) => format!("#<error: {}>", e.message),
    }
}

fn extract_json(response: &str) -> &str {
    let response = response.trim();

    // Handle code blocks
    let clean = if response.starts_with("```") {
        let mut lines = response.lines();
        lines.next(); // skip start fence
        let content: Vec<&str> = lines.collect();
        let joined = content.join("\n");
        if let Some(end) = joined.rfind("```") {
            joined[..end].to_string()
        } else {
            joined
        }
    } else {
        response.to_string()
    };

    // Simpler approach compatible with &str return:
    let start_idx = response.find(|c| c == '{' || c == '[');
    let end_idx = response.rfind(|c| c == '}' || c == ']');

    if let (Some(s), Some(e)) = (start_idx, end_idx) {
        if s <= e {
            return &response[s..=e];
        }
    }

    response
}

fn load_agent_config(config_path: &str) -> Result<AgentConfig, Box<dyn Error + Send + Sync>> {
    // Try the provided path first, then try parent directory (for running from ccos/ subdirectory)
    let path = std::path::Path::new(config_path);
    let actual_path = if path.exists() {
        path.to_path_buf()
    } else {
        // Try parent directory
        let parent_path = std::path::Path::new("..").join(config_path);
        if parent_path.exists() {
            parent_path
        } else {
            return Err(format!(
                "Config file not found: '{}' (also tried '../{}'). Run from the workspace root directory.",
                config_path, config_path
            ).into());
        }
    };

    let mut content = std::fs::read_to_string(&actual_path).map_err(|e| {
        format!(
            "Failed to read config file '{}': {}",
            actual_path.display(),
            e
        )
    })?;
    if content.starts_with("# RTFS") {
        content = content.lines().skip(1).collect::<Vec<_>>().join("\n");
    }
    toml::from_str(&content).map_err(|e| format!("failed to parse agent config: {}", e).into())
}

fn apply_llm_profile(
    agent_config: &AgentConfig,
    profile: Option<&str>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    if let Some(profile_name) = profile {
        let (expanded_profiles, _, _) =
            rtfs::config::profile_selection::expand_profiles(agent_config);

        if let Some(llm_profile) = expanded_profiles.iter().find(|p| p.name == profile_name) {
            std::env::set_var("CCOS_DELEGATING_PROVIDER", llm_profile.provider.clone());
            std::env::set_var("CCOS_DELEGATING_MODEL", llm_profile.model.clone());
            if let Some(api_key_env) = &llm_profile.api_key_env {
                if let Ok(api_key) = std::env::var(api_key_env) {
                    std::env::set_var("OPENAI_API_KEY", api_key);
                }
            } else if let Some(api_key) = &llm_profile.api_key {
                std::env::set_var("OPENAI_API_KEY", api_key.clone());
            }
        } else {
            return Err(format!("LLM profile '{}' not found in config", profile_name).into());
        }
    }
    Ok(())
}

/// Save synthesized capability to a file for inspection
fn save_synthesized_capability(id: &str, code: &str, description: &str, hint: &str) {
    // Determine output directory - save in capabilities/generated/<id>/capability.rtfs
    let root = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    // Check if we're in the workspace root (has ccos/Cargo.toml) or inside ccos/ dir
    let base_dir = if root.join("ccos/Cargo.toml").exists() {
        // We're at workspace root, save in capabilities/
        root.join("capabilities/generated")
    } else if root.join("Cargo.toml").exists() && root.ends_with("ccos") {
        // We're inside ccos/ directory, go up to workspace root
        root.parent()
            .unwrap_or(&root)
            .join("capabilities/generated")
    } else {
        // Fallback
        root.join("capabilities/generated")
    };

    // Create directory for this capability: capabilities/generated/<id>/
    let cap_dir = base_dir.join(id.replace(".", "_").replace("/", "_"));
    if let Err(e) = std::fs::create_dir_all(&cap_dir) {
        eprintln!(
            "     ⚠️  Failed to create synthesized capability directory: {}",
            e
        );
        return;
    }

    // Save as capability.rtfs (matching existing structure)
    let filepath = cap_dir.join("capability.rtfs");

    // Generate RTFS capability file content
    let rtfs_content = format!(
        r#";; Synthesized Capability: {}
;; Description: {}
;; Hint: {}
;; Generated at: {}

(capability
  :id "{}"
  :name "Synthesized: {}"
  :description "{}"
  :implementation
    {}
)
"#,
        id,
        description,
        hint,
        chrono::Utc::now().to_rfc3339(),
        id,
        hint,
        description.replace("\"", "\\\""),
        code
    );

    match std::fs::write(&filepath, &rtfs_content) {
        Ok(_) => println!(
            "     💾 Saved synthesized capability to: {}",
            filepath.display()
        ),
        Err(e) => eprintln!("     ⚠️  Failed to save synthesized capability: {}", e),
    }
}

/// Get the discovered capabilities directory path
fn get_capabilities_discovered_dir() -> std::path::PathBuf {
    let root = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    // Check if we're in the workspace root (has ccos/Cargo.toml) or inside ccos/ dir
    if root.join("ccos/Cargo.toml").exists() {
        // We're at workspace root
        root.join("capabilities/discovered")
    } else if root.join("Cargo.toml").exists() && root.ends_with("ccos") {
        // We're inside ccos/ directory, go up to workspace root
        root.parent()
            .unwrap_or(&root)
            .join("capabilities/discovered")
    } else {
        // Fallback
        root.join("capabilities/discovered")
    }
}

/// Compute MCP tool match score using existing scoring helpers
// Note: compute_mcp_tool_score and keyword_overlap are imported from ccos::discovery::capability_matcher

/// Convert serde_json::Value to rtfs::Value
fn json_to_rtfs_value(v: serde_json::Value) -> Value {
    match v {
        serde_json::Value::Null => Value::Nil,
        serde_json::Value::Bool(b) => Value::Boolean(b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Value::Integer(i)
            } else if let Some(f) = n.as_f64() {
                Value::Float(f)
            } else {
                Value::Integer(0)
            }
        }
        serde_json::Value::String(s) => Value::String(s),
        serde_json::Value::Array(a) => {
            Value::Vector(a.into_iter().map(json_to_rtfs_value).collect())
        }
        serde_json::Value::Object(o) => {
            let mut map = HashMap::new();
            for (k, v) in o {
                map.insert(
                    rtfs::ast::MapKey::Keyword(rtfs::ast::Keyword(k)),
                    json_to_rtfs_value(v),
                );
            }
            Value::Map(map)
        }
    }
}

// ============================================================================
// MCP Server Resolution Helpers
// ============================================================================

/// Resolve MCP server URL and name from overrides.json
/// This checks the curated overrides file for matching server configurations
/// Returns (url, server_name) where server_name is derived from the match pattern prefix (e.g., "github" from "github.*")
fn resolve_server_url_from_overrides(hint: &str) -> Option<(String, String)> {
    // Try to load curated overrides from 'capabilities/mcp/overrides.json' (in workspace root)
    let root = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    // Check if we're in the workspace root (has ccos/Cargo.toml) or inside ccos/ dir
    let overrides_path = if root.join("ccos/Cargo.toml").exists() {
        // We're at workspace root
        root.join("capabilities/mcp/overrides.json")
    } else if root.join("Cargo.toml").exists() && root.ends_with("ccos") {
        // We're inside ccos/ directory, go up to workspace root
        root.parent()
            .unwrap_or(&root)
            .join("capabilities/mcp/overrides.json")
    } else {
        // Fallback
        root.join("capabilities/mcp/overrides.json")
    };

    if !overrides_path.exists() {
        return None;
    }

    let content = std::fs::read_to_string(&overrides_path).ok()?;
    let parsed: serde_json::Value = serde_json::from_str(&content).ok()?;
    let entries = parsed.get("entries")?.as_array()?;

    for entry in entries {
        if let Some(server) = entry.get("server") {
            // Check server name equality first
            if let Some(name) = server.get("name").and_then(|n| n.as_str()) {
                if hint.contains(name) || name.contains(hint) {
                    // Get best HTTP remote
                    if let Some(url) = get_http_remote_url(server) {
                        // Use the server name from overrides as the namespace
                        return Some((url, name.to_string()));
                    }
                }
            }

            // Check if `matches` patterns include the hint
            if let Some(matches) = entry.get("matches").and_then(|m| m.as_array()) {
                for pat in matches {
                    if let Some(p) = pat.as_str() {
                        // Check if pattern matches hint (pattern may contain wildcards like "github.*")
                        let pattern_clean = p.trim_end_matches(".*").trim_end_matches('*');
                        if hint.contains(pattern_clean) || pattern_clean.contains(hint) {
                            if let Some(url) = get_http_remote_url(server) {
                                // Use the pattern prefix as the server namespace (e.g., "github" from "github.*")
                                let server_name = pattern_clean.to_string();
                                return Some((url, server_name));
                            }
                        }
                    }
                }
            }
        }
    }

    None
}

/// Extract HTTP/HTTPS remote URL from server definition
fn get_http_remote_url(server: &serde_json::Value) -> Option<String> {
    if let Some(remotes) = server.get("remotes").and_then(|r| r.as_array()) {
        for remote in remotes {
            if let Some(url) = remote.get("url").and_then(|u| u.as_str()) {
                if url.starts_with("http://") || url.starts_with("https://") {
                    return Some(url.to_string());
                }
            }
        }
    }
    None
}

/// Get MCP authentication headers from environment variables
fn get_mcp_auth_headers() -> Option<std::collections::HashMap<String, String>> {
    // Use MCP_AUTH_TOKEN for MCP server authentication
    if let Ok(tok) = std::env::var("MCP_AUTH_TOKEN") {
        if !tok.is_empty() {
            let mut headers = std::collections::HashMap::new();
            headers.insert("Authorization".to_string(), format!("Bearer {}", tok));
            println!("     🔑 Using auth token from MCP_AUTH_TOKEN");
            return Some(headers);
        }
    }

    None
}

// ============================================================================
// Main Entry
// ============================================================================

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    let args = Args::parse();
    println!("🤖 Autonomous Agent Demo (Iterative)");
    println!("====================================");
    println!("Goal: {}", args.goal);

    // Setup
    let agent_config = load_agent_config(&args.config)?;
    apply_llm_profile(&agent_config, args.profile.as_deref())?;

    if std::env::var("CCOS_DELEGATING_MODEL").is_err() {
        println!("⚠️ CCOS_DELEGATING_MODEL not set, defaulting to 'deepseek/deepseek-v3.2-exp'");
        std::env::set_var("CCOS_DELEGATING_MODEL", "deepseek/deepseek-v3.2-exp");
    }
    std::env::set_var("CCOS_DELEGATION_ENABLED", "true");

    let ccos = Arc::new(
        CCOS::new_with_agent_config_and_configs_and_debug_callback(
            Default::default(),
            None,
            Some(agent_config),
            None,
        )
        .await?,
    );

    // Register basic tools
    ccos::capabilities::defaults::register_default_capabilities(&ccos.get_capability_marketplace())
        .await?;

    // Configure session pool for MCP execution
    let mut session_pool = SessionPoolManager::new();
    session_pool.register_handler("mcp", std::sync::Arc::new(MCPSessionHandler::new()));
    let session_pool = std::sync::Arc::new(session_pool);
    ccos.get_capability_marketplace()
        .set_session_pool(session_pool)
        .await;
    println!("✅ Session pool configured with MCPSessionHandler");

    if args.use_modular_planner {
        run_modular_planner(ccos, args.goal, args.profile).await?;
    } else {
        // Initialize Planner
        let mut planner =
            IterativePlanner::new(ccos.clone(), args.simulate_error, args.allow_mock)?;

        // Plan
        println!("\n🏗️  Building Plan...");
        let final_plan_rtfs = planner.solve(&args.goal, 0).await?;

        println!("\n📝 Generated RTFS Plan:");
        println!("--------------------------------------------------");
        println!("{}", final_plan_rtfs);
        println!("--------------------------------------------------");

        // Execute
        println!("\n🚀 Executing Plan...");

        let mut current_plan_rtfs = final_plan_rtfs;
        let mut attempts = 0;
        const MAX_ATTEMPTS: usize = 3;

        loop {
            attempts += 1;
            let plan_obj = ccos::types::Plan {
                plan_id: "iterative-plan".to_string(),
                name: Some("Generated Plan".to_string()),
                body: ccos::types::PlanBody::Rtfs(current_plan_rtfs.clone()),
                intent_ids: vec![], // Simplification
                ..Default::default()
            };

            let context = RuntimeContext::full();
            let result = ccos.validate_and_execute_plan(plan_obj, &context).await;

            let (success, error_msg) = match result {
                Ok(exec_result) => {
                    if exec_result.success {
                        println!("\n🏁 Execution Result:");
                        println!("   Success: {}", exec_result.success);
                        println!("   Result: {:?}", exec_result.value);
                        break;
                    } else {
                        let msg = exec_result
                            .metadata
                            .get("error")
                            .map(|v| value_to_rtfs_literal(v))
                            .unwrap_or_else(|| "Unknown error".to_string());
                        (false, msg)
                    }
                }
                Err(e) => (false, format!("Runtime Error: {}", e)),
            };

            if !success {
                println!(
                    "\n❌ Execution Failed (Attempt {}/{}): {}",
                    attempts, MAX_ATTEMPTS, error_msg
                );

                if attempts >= MAX_ATTEMPTS {
                    println!("   Giving up after {} attempts.", MAX_ATTEMPTS);
                    break;
                }

                // Repair
                match planner.repair_plan(&current_plan_rtfs, &error_msg).await {
                    Ok(repaired) => {
                        println!("   📝 Repaired Plan:\n{}", repaired);
                        current_plan_rtfs = repaired;
                    }
                    Err(e) => {
                        println!("   ⚠️ Failed to repair plan: {}", e);
                        break;
                    }
                }
            }
        }

        // Dump Trace
        println!("\n🔍 Planning Trace:");
        println!("{}", serde_json::to_string_pretty(&planner.trace)?);
    }

    Ok(())
}

// ============================================================================
// Modular Planner Mode
// ============================================================================

async fn run_modular_planner(
    ccos: Arc<CCOS>,
    goal: String,
    profile: Option<String>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    println!("\n╔══════════════════════════════════════════════════════════════╗");
    println!("║           🧩 Modular Planner Mode                            ║");
    println!("╚══════════════════════════════════════════════════════════════╝\n");

    println!("📋 Goal: \"{}\"\n", goal);

    // Apply LLM profile if specified (already done in main, but ensuring env vars are set)
    if let Some(p) = profile {
        println!("   Using LLM Profile: {}", p);
    }

    // 1. Use IntentGraph from CCOS
    println!("🔧 Using IntentGraph from CCOS...");
    let intent_graph = ccos.get_intent_graph();

    // 2. Build capability catalog using adapter
    println!("\n🔍 Setting up capability catalog...");
    let catalog = Arc::new(CcosCatalogAdapter::new(ccos.get_catalog()));

    // 3. Create decomposition strategy
    println!("\n📐 Using PatternDecomposition (fast, deterministic)");
    let decomposition: Box<dyn DecompositionStrategy> = Box::new(PatternDecomposition::new());

    // 4. Create resolution strategy (Composite: Catalog + MCP)
    let mut composite_resolution = CompositeResolution::new();

    // A. Catalog Resolution
    composite_resolution.add_strategy(Box::new(CatalogResolution::new(catalog.clone())));

    // B. MCP Resolution
    let mut auth_headers = HashMap::new();
    if let Ok(token) = std::env::var("MCP_AUTH_TOKEN") {
        if !token.is_empty() {
            auth_headers.insert("Authorization".to_string(), format!("Bearer {}", token));
        }
    }

    // Create discovery service with auth headers
    let discovery_service = Arc::new(
        ccos::mcp::core::MCPDiscoveryService::with_auth_headers(Some(auth_headers))
            .with_marketplace(ccos.get_capability_marketplace()),
    );

    // Create runtime MCP discovery using the unified discovery service
    let mcp_discovery = Arc::new(RuntimeMcpDiscovery::with_discovery_service(
        ccos.get_capability_marketplace(),
        discovery_service,
    ));

    let mcp_resolution = McpResolution::new(mcp_discovery);
    composite_resolution.add_strategy(Box::new(mcp_resolution));

    // 5. Create the modular planner
    let config = PlannerConfig {
        max_depth: 5,
        persist_intents: true,
        create_edges: true,
        intent_namespace: "auto".to_string(),
        verbose_llm: false,
        show_prompt: false,
        confirm_llm: false,
        eager_discovery: true,
    };

    let mut planner = ModularPlanner::new(
        decomposition,
        Box::new(composite_resolution),
        intent_graph.clone(),
    )
    .with_config(config);

    // 6. Plan
    println!("\n🚀 Planning...\n");

    let plan_result = match planner.plan(&goal).await {
        Ok(result) => result,
        Err(e) => {
            println!("\n❌ Planning failed: {}", e);
            return Ok(());
        }
    };

    println!("═══════════════════════════════════════════════════════════════");
    println!("📋 Plan Result");
    println!("═══════════════════════════════════════════════════════════════\n");

    println!("📝 Resolved Steps ({}):", plan_result.intent_ids.len());
    for (i, intent_id) in plan_result.intent_ids.iter().enumerate() {
        if let Some(resolution) = plan_result.resolutions.get(intent_id) {
            let (status, cap_id) = match resolution {
                ResolvedCapability::Local { capability_id, .. } => {
                    ("Local", capability_id.as_str())
                }
                ResolvedCapability::Remote { capability_id, .. } => {
                    ("Remote", capability_id.as_str())
                }
                ResolvedCapability::BuiltIn { capability_id, .. } => {
                    ("BuiltIn", capability_id.as_str())
                }
                ResolvedCapability::Synthesized { capability_id, .. } => {
                    ("Synth", capability_id.as_str())
                }
                ResolvedCapability::NeedsReferral { reason, .. } => ("Referral", reason.as_str()),
            };
            println!("   {}. [{}] {}", i + 1, status, cap_id);
        }
    }

    println!("\n📜 Generated RTFS Plan:");
    println!("───────────────────────────────────────────────────────────────");
    println!("{}", plan_result.rtfs_plan);
    println!("───────────────────────────────────────────────────────────────");

    // Show trace if verbose
    println!("\n🔍 Planning Trace:");
    for event in &plan_result.trace.events {
        match event {
            ModularTraceEvent::DecompositionStarted { strategy } => {
                println!("   → Decomposition started with strategy: {}", strategy);
            }
            ModularTraceEvent::DecompositionCompleted {
                num_intents,
                confidence,
            } => {
                println!(
                    "   ✓ Decomposition completed: {} intents, confidence: {:.2}",
                    num_intents, confidence
                );
            }
            ModularTraceEvent::IntentCreated {
                intent_id,
                description,
            } => {
                println!(
                    "   + Intent created: {} - \"{}\"",
                    &intent_id[..20.min(intent_id.len())],
                    description
                );
            }
            ModularTraceEvent::EdgeCreated {
                from,
                to,
                edge_type,
            } => {
                println!(
                    "   ⟶ Edge: {} -> {} ({})",
                    &from[..16.min(from.len())],
                    &to[..16.min(to.len())],
                    edge_type
                );
            }
            ModularTraceEvent::ResolutionStarted { intent_id } => {
                println!("   🔍 Resolving: {}", &intent_id[..20.min(intent_id.len())]);
            }
            ModularTraceEvent::ResolutionCompleted {
                intent_id,
                capability,
            } => {
                println!(
                    "   ✓ Resolved: {} → {}",
                    &intent_id[..16.min(intent_id.len())],
                    capability
                );
            }
            ModularTraceEvent::ResolutionFailed { intent_id, reason } => {
                println!(
                    "   ✗ Failed: {} - {}",
                    &intent_id[..16.min(intent_id.len())],
                    reason
                );
            }
        }
    }

    // 7. Execute
    println!("\n⚡ Executing Plan...");
    println!("───────────────────────────────────────────────────────────────");

    let plan_obj = ccos::types::Plan {
        plan_id: format!("modular-plan-{}", uuid::Uuid::new_v4()),
        name: Some("Modular Plan".to_string()),
        body: ccos::types::PlanBody::Rtfs(plan_result.rtfs_plan.clone()),
        intent_ids: plan_result.intent_ids.clone(),
        ..Default::default()
    };

    let context = RuntimeContext::full();
    match ccos.validate_and_execute_plan(plan_obj, &context).await {
        Ok(exec_result) => {
            println!("\n🏁 Execution Result:");
            println!("   Success: {}", exec_result.success);

            // Use existing value_to_rtfs_literal for nicer output if available, else debug
            println!("   Result: {:?}", exec_result.value);

            if !exec_result.success {
                if let Some(err) = exec_result.metadata.get("error") {
                    println!("   Error: {:?}", err);
                }
            }
        }
        Err(e) => {
            println!("\n❌ Execution Failed: {}", e);
        }
    }

    println!("\n✅ Modular Planner run complete!");
    Ok(())
}
