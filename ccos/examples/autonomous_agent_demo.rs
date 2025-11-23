//! Autonomous Agent Demo (Iterative & Recursive)
//!
//! This example demonstrates an advanced, self-evolving autonomous agent that:
//! 1. Takes a high-level goal from the user.
//! 2. Iteratively decomposes it into steps using the Arbiter.
//! 3. Resolves capabilities for each step (Local -> Semantic Search -> MCP Registry).
//! 4. Recursively plans for missing capabilities that can't be found directly.
//! 5. Constructs a final executable RTFS plan.
//! 6. Traces the decision process.
//!
//! Usage:
//!   cargo run --example autonomous_agent_demo -- --goal "find the issues of repository ccos and user mandubian and filter them to keep only those containing RTFS"

use std::sync::Arc;
use std::collections::HashMap;
use std::error::Error;
use std::future::Future;
use std::pin::Pin;

use clap::Parser;
use ccos::CCOS;
use rtfs::config::types::AgentConfig;
use ccos::arbiter::DelegatingArbiter;
use ccos::catalog::{CatalogService, CatalogFilter, CatalogEntryKind};
use ccos::capability_marketplace::{CapabilityMarketplace, CapabilityManifest};
use ccos::synthesis::mcp_registry_client::McpRegistryClient;
use rtfs::runtime::security::RuntimeContext;
use rtfs::runtime::values::Value;
use rtfs::runtime::error::RuntimeResult;

// ============================================================================
// CLI Arguments
// ============================================================================

#[derive(Parser, Debug)]
struct Args {
    /// Natural language goal
    #[arg(long, default_value = "find the issues of repository ccos and user mandubian and filter them to keep only those containing RTFS")]
    goal: String,

    /// Path to agent config file
    #[arg(long, default_value = "config/agent_config.toml")]
    config: String,

    /// Optional LLM profile name
    #[arg(long)]
    profile: Option<String>,
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

#[derive(Debug, Clone)]
enum ResolutionStatus {
    ResolvedLocal(String, HashMap<String, String>), // ID, Args
    ResolvedRemote(String, HashMap<String, String>), // ID, Args (installed from MCP)
    ResolvedSynthesized(String, HashMap<String, String>), // ID, Args (generated)
    NeedsSubPlan(String, String), // Goal, Hint (Recursive)
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
    ResolutionAttempt { step: String, status: String },
    MCPDiscovery { hint: String, found: bool },
    Synthesis { capability: String, success: bool },
    RecursiveSubPlan { parent_step: String, sub_goal: String },
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
}

impl IterativePlanner {
    fn new(ccos: Arc<CCOS>) -> Result<Self, Box<dyn Error + Send + Sync>> {
        let arbiter = ccos.get_delegating_arbiter()
            .ok_or::<Box<dyn Error + Send + Sync>>("Delegating arbiter not available".into())?;
        let marketplace = ccos.get_capability_marketplace();
        let catalog = ccos.get_catalog();

        Ok(Self {
            _ccos: ccos,
            arbiter: arbiter.clone(),
            marketplace,
            catalog,
            trace: PlanningTrace {
                goal: "Unknown".to_string(),
                decisions: Vec::new(),
            },
        })
    }

    // Recursive async function requires manual boxing
    fn solve<'a>(&'a mut self, goal: &'a str, depth: usize) -> Pin<Box<dyn Future<Output = Result<String, Box<dyn Error + Send + Sync>>> + 'a>> {
        Box::pin(async move {
            if depth > 5 {
                return Err("Max recursion depth exceeded".into());
            }
            self.trace.goal = goal.to_string();
            println!("\n🧠 Solving Goal (Depth {}): \"{}\"", depth, goal);

            // 1. Decompose
            let steps = self.decompose(goal).await?;
            self.trace.decisions.push(TraceEvent::Decomposition(steps.clone()));

            let mut rtfs_steps = Vec::new();

            for (i, step) in steps.iter().enumerate() {
                println!("\n  👉 Step {}: {} (Hint: {})", i+1, step.description, step.capability_hint);
                
                // 2. Resolve
                let status = self.resolve_step(step).await?;
                
                self.trace.decisions.push(TraceEvent::ResolutionAttempt { 
                    step: step.description.clone(), 
                    status: format!("{:?}", status) 
                });

                match status {
                    ResolutionStatus::ResolvedLocal(id, args) => {
                        println!("     ✅ Resolved Local: {}", id);
                        rtfs_steps.push(self.generate_call(&id, args));
                    },
                    ResolutionStatus::ResolvedRemote(id, args) => {
                        println!("     ✅ Resolved Remote (Installed): {}", id);
                        rtfs_steps.push(self.generate_call(&id, args));
                    },
                    ResolutionStatus::ResolvedSynthesized(id, args) => {
                        println!("     ✅ Resolved Synthesized: {}", id);
                        rtfs_steps.push(self.generate_call(&id, args));
                    },
                    ResolutionStatus::NeedsSubPlan(sub_goal, _hint) => {
                        println!("     🔄 Complex Step -> Triggering Recursive Sub-Planning...");
                        self.trace.decisions.push(TraceEvent::RecursiveSubPlan {
                            parent_step: step.description.clone(),
                            sub_goal: sub_goal.clone()
                        });
                        
                        // Recursive call!
                        let sub_plan_rtfs = self.solve(&sub_goal, depth + 1).await?;
                        rtfs_steps.push(format!(";; Sub-plan for: {}\n{}", sub_goal, sub_plan_rtfs));
                    },
                    ResolutionStatus::Failed(reason) => {
                        println!("     ❌ Failed: {}", reason);
                        return Err(format!("Planning failed at step '{}': {}", step.description, reason).into());
                    }
                }
            }

            Ok(self.wrap_in_program(rtfs_steps))
        })
    }

    async fn decompose(&self, goal: &str) -> Result<Vec<PlannedStep>, Box<dyn Error + Send + Sync>> {
        let prompt = format!(
            r#"You are an expert planner. Decompose the following goal into a sequence of logical steps.
For each step, provide a description and a short "capability hint" that looks like a tool ID (e.g. "github.list_repos", "calendar.create_event", "data.filter").
Try to map high-level actions to potential tool names if possible.

Goal: "{}"

Respond ONLY with a JSON object in this format:
{{
  "steps": [
    {{ "description": "Fetch today's meetings", "capability_hint": "calendar.list_events" }},
    {{ "description": "Email the summary to the team", "capability_hint": "email.send" }}
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

    async fn resolve_step(&mut self, step: &PlannedStep) -> Result<ResolutionStatus, Box<dyn Error + Send + Sync>> {
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
            if let Some((id, args)) = self.try_select_from_candidates(&local_candidates, &step.description).await? {
                return Ok(ResolutionStatus::ResolvedLocal(id, args));
            }
            println!("     ⚠️  Local candidates rejected by LLM. Trying registry...");
        }

        // B. If no local (or rejected), try MCP Registry (Remote)
        println!("     🔍 Searching MCP Registry for '{}' (Context: '{}')...", step.capability_hint, step.description);
        if let Some(installed_cap) = self.try_install_from_registry(&step.capability_hint, &step.description).await? {
            println!("     📦 Installed capability: {}", installed_cap.id);
            let remote_candidates = vec![installed_cap];
            if let Some((id, args)) = self.try_select_from_candidates(&remote_candidates, &step.description).await? {
                return Ok(ResolutionStatus::ResolvedRemote(id, args));
            } else {
                println!("     ⚠️  Installed capability rejected by LLM.");
            }
        } else {
            println!("     ❌ Registry search returned no results.");
        }

        // C. If registry fails, try Synthesis (Simulated)
        println!("     🧪 Registry failed. Attempting to synthesize capability for '{}'...", step.description);
        if let Some(synth_cap) = self.try_synthesize(&step.description, &step.capability_hint).await? {
             println!("     ✨ Synthesized new capability: {}", synth_cap.id);
             let candidates = vec![synth_cap];
             if let Some((id, args)) = self.try_select_from_candidates(&candidates, &step.description).await? {
                return Ok(ResolutionStatus::ResolvedSynthesized(id, args));
            }
        }

        // D. If we are here, either no candidates found OR LLM rejected them.
        Ok(ResolutionStatus::NeedsSubPlan(step.description.clone(), step.capability_hint.clone()))
    }

    async fn try_synthesize(&mut self, description: &str, hint: &str) -> Result<Option<CapabilityManifest>, Box<dyn Error + Send + Sync>> {
        // In a real system, this would use the LLM to write RTFS code.
        // For this demo, we simulate synthesis of common missing primitives.
        let desc_lower = description.to_lowercase();
        let hint_lower = hint.to_lowercase();

        if desc_lower.contains("filter") || hint_lower.contains("filter") {
            self.trace.decisions.push(TraceEvent::Synthesis { capability: "ccos.data.filter".into(), success: true });
            // Synthesize a generic filter capability
            let id = "ccos.data.filter";
            if self.marketplace.get_capability(id).await.is_none() {
                 let handler = Arc::new(|_args: &Value| -> RuntimeResult<Value> {
                     // Mock filter execution
                     println!("     [ccos.data.filter] 🧪 Executing synthesized filter logic...");
                     // Return a dummy list (e.g. list of 1 item) to simulate success
                     Ok(Value::Vector(vec![
                        Value::String("Issue #1: Add RTFS support (Filtered)".into())
                     ]))
                 });

                 let _ = self.marketplace.register_local_capability(
                     id.to_string(),
                     "Generic Data Filter".to_string(),
                     "Filters a list of items based on criteria.".to_string(),
                     handler
                 ).await;
            }
            return Ok(self.marketplace.get_capability(id).await);
        }
        
        if desc_lower.contains("search") || hint_lower.contains("search") {
             self.trace.decisions.push(TraceEvent::Synthesis { capability: "ccos.data.search".into(), success: true });
             // Synthesize search
             let id = "ccos.data.search";
             if self.marketplace.get_capability(id).await.is_none() {
                 let handler = Arc::new(|_args: &Value| -> RuntimeResult<Value> {
                     println!("     [ccos.data.search] 🧪 Executing synthesized search logic...");
                     Ok(Value::Vector(vec![
                        Value::String("Found Item: RTFS Spec".into())
                     ]))
                 });

                 let _ = self.marketplace.register_local_capability(
                     id.to_string(),
                     "Generic Data Search".to_string(),
                     "Searches data for a pattern.".to_string(),
                     handler
                 ).await;
            }
            return Ok(self.marketplace.get_capability(id).await);
        }

        self.trace.decisions.push(TraceEvent::Synthesis { capability: "unknown".into(), success: false });
        
        // [Demo] Ultimate Fallback: Synthesize a generic placeholder to allow the demo to complete
        // instead of failing with recursion depth.
        println!("     ⚠️  Could not find specific match. Synthesizing generic placeholder for '{}'", hint);
        let id = format!("ccos.generated.{}", hint.replace(".", "_").replace(" ", "_"));
        let handler = Arc::new(|_args: &Value| -> RuntimeResult<Value> {
             println!("     [{}] 🧪 Executing generic synthesized logic...", "ccos.generated.placeholder");
             Ok(Value::String("Success (Mocked)".into()))
        });
        let _ = self.marketplace.register_local_capability(
            id.clone(),
            format!("Generated: {}", hint),
            description.to_string(),
            handler
        ).await;
        
        return Ok(self.marketplace.get_capability(&id).await);
    }

    async fn try_select_from_candidates(&self, candidates: &[CapabilityManifest], goal: &str) -> Result<Option<(String, HashMap<String, String>)>, Box<dyn Error + Send + Sync>> {
        let tool_descriptions: Vec<String> = candidates.iter()
            .map(|c| format!("{}: {}", c.id, c.description))
            .collect();
        
        self.select_tool_robust(goal, &tool_descriptions).await
    }

     async fn try_install_from_registry(&mut self, hint: &str, description: &str) -> Result<Option<CapabilityManifest>, Box<dyn Error + Send + Sync>> {
        let client = McpRegistryClient::new();
        // Search registry using hint and description keywords
        let search_query = if hint.contains(".") { hint } else { description };
        let servers = client.search_servers(search_query).await.unwrap_or_default();
        
        if let Some(server) = servers.first() {
            self.trace.decisions.push(TraceEvent::MCPDiscovery { hint: hint.to_string(), found: true });
            println!("     🌐 Found server in registry: {}", server.name);
            
            // [Demo] In a real system, we would use MCP client to list tools from this server.
            // For the demo, we will assume the server has the requested tool if the names align.
            if server.name.contains("github") {
                 if hint.contains("issues") || hint.contains("list") {
                     self.ensure_github_capability().await;
                     return Ok(self.marketplace.get_capability("mcp.github.list_issues").await);
                 }
                 // NEW: Handle repos list specifically
                 if hint.contains("repos") || description.contains("repos") || description.contains("repository") {
                    let cap_id = "mcp.github.list_repos";
                    if self.marketplace.get_capability(cap_id).await.is_none() {
                        let handler = Arc::new(|_args: &Value| -> RuntimeResult<Value> {
                            Ok(Value::Vector(vec![
                                Value::String("ccos (Mocked Repo)".into()),
                                Value::String("rtfs (Mocked Repo)".into())
                            ]))
                        });
                        let _ = self.marketplace.register_local_capability(
                            cap_id.to_string(),
                            "List Repositories".to_string(),
                            "List repositories for a user".to_string(),
                            handler
                        ).await;
                    }
                    return Ok(self.marketplace.get_capability(cap_id).await);
                 }
            }
        }

        // Fallback: Simulate discovery for demo purposes if registry is not reachable or returns nothing
        let hint_lower = hint.to_lowercase();
        let desc_lower = description.to_lowercase();
        
        let is_github = hint_lower.contains("github") || desc_lower.contains("github");
        let is_repo = hint_lower.contains("repo") || desc_lower.contains("repo") || desc_lower.contains("repositories");
        let is_issue = hint_lower.contains("issue") || desc_lower.contains("issue");
        let is_auth = hint_lower.contains("auth") || desc_lower.contains("auth") || desc_lower.contains("login");

        if is_github && (is_repo || is_issue || is_auth) {
             self.trace.decisions.push(TraceEvent::MCPDiscovery { hint: hint.to_string(), found: true });
             println!("     🌐 [Demo] Simulating registry discovery based on context '{}'", description);
             
             let cap_id = if is_auth {
                 self.ensure_github_auth_capability().await;
                 "mcp.github.authenticate"
             } else if is_repo {
                 // Register repo list capability if missing
                 let id = "mcp.github.list_repos";
                 if self.marketplace.get_capability(id).await.is_none() {
                        let handler = Arc::new(|_args: &Value| -> RuntimeResult<Value> {
                            Ok(Value::Vector(vec![
                                Value::String("ccos (Mocked)".into()),
                                Value::String("rtfs-engine (Mocked)".into())
                            ]))
                        });
                        let _ = self.marketplace.register_local_capability(
                            id.to_string(), "List Repos".to_string(), "List repositories for a user".to_string(), handler
                        ).await;
                 }
                 id
             } else {
                 self.ensure_github_capability().await;
                 "mcp.github.list_issues"
             };

             let cap = self.marketplace.get_capability(cap_id).await;
             if cap.is_none() {
                 println!("     ⚠️  Failed to retrieve mocked capability after registration!");
             }
             return Ok(cap);
        } else {
            self.trace.decisions.push(TraceEvent::MCPDiscovery { hint: hint.to_string(), found: false });
            println!("     ⚠️  Fallback condition failed for hint: '{}'", hint);
        }

        Ok(None)
    }

    async fn select_tool_robust(&self, goal: &str, tools: &[String]) -> Result<Option<(String, HashMap<String, String>)>, Box<dyn Error + Send + Sync>> {
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
            Ok(Some((selection.tool, selection.arguments.unwrap_or_default())))
        }
    }

    fn generate_call(&self, capability_id: &str, args: HashMap<String, String>) -> String {
        // Convert args map to RTFS map syntax: {:key "value" ...}
        let args_str = args.iter()
            .map(|(k, v)| format!(":{} \"{}\"", k, v))
            .collect::<Vec<_>>()
            .join(" ");
        
        format!("(call \"{}\" {{{}}})", capability_id, args_str)
    }

    fn wrap_in_program(&self, steps: Vec<String>) -> String {
        // Wrap in a do block
        let body = steps.join("\n  ");
        format!("(do\n  {}\n)", body)
    }

    // Helper to register mock for demo purposes if registry install "succeeds"
    async fn ensure_github_capability(&self) {
        if self.marketplace.get_capability("mcp.github.list_issues").await.is_none() {
            let handler = Arc::new(|_args: &Value| -> RuntimeResult<Value> {
                // Mock return value
                let mut map = HashMap::new();
                map.insert(rtfs::ast::MapKey::Keyword(rtfs::ast::Keyword("content".into())), Value::Vector(vec![
                    Value::Map({
                        let mut inner = HashMap::new();
                        inner.insert(rtfs::ast::MapKey::Keyword(rtfs::ast::Keyword("text".into())), Value::String("Issue #1: Add RTFS support".into()));
                        inner
                    })
                ]));
                Ok(Value::Map(map))
            });
            
            let _ = self.marketplace.register_local_capability(
                "mcp.github.list_issues".to_string(),
                "List GitHub Issues".to_string(),
                "List issues from a GitHub repository.".to_string(),
                handler
            ).await;
        }
    }

    async fn ensure_github_auth_capability(&self) {
        if self.marketplace.get_capability("mcp.github.authenticate").await.is_none() {
            let handler = Arc::new(|_args: &Value| -> RuntimeResult<Value> {
                println!("     [mcp.github.authenticate] 🔐 Authenticating with GitHub (Mock)...");
                println!("     [mcp.github.authenticate] 🔑 Token: ghp_mock_token_12345");
                Ok(Value::String("ghp_mock_token_12345".into()))
            });
            
            let _ = self.marketplace.register_local_capability(
                "mcp.github.authenticate".to_string(),
                "Authenticate with GitHub".to_string(),
                "Authenticate using personal access token or OAuth.".to_string(),
                handler
            ).await;
        }
    }
}

// ============================================================================
// Utils
// ============================================================================

fn extract_json(response: &str) -> &str {
    if let Some(start) = response.find('{') {
        if let Some(end) = response.rfind('}') {
            &response[start..=end]
        } else {
            response
        }
    } else {
        response
    }
}

fn load_agent_config(config_path: &str) -> Result<AgentConfig, Box<dyn Error + Send + Sync>> {
    let mut content = std::fs::read_to_string(config_path)?;
    if content.starts_with("# RTFS") {
        content = content.lines().skip(1).collect::<Vec<_>>().join("\n");
    }
    toml::from_str(&content).map_err(|e| format!("failed to parse agent config: {}", e).into())
}

fn apply_llm_profile(agent_config: &AgentConfig, profile: Option<&str>) -> Result<(), Box<dyn Error + Send + Sync>> {
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
        println!("⚠️ CCOS_DELEGATING_MODEL not set, defaulting to 'gpt-4o'");
        std::env::set_var("CCOS_DELEGATING_MODEL", "gpt-4o");
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
    ccos::capabilities::defaults::register_default_capabilities(&ccos.get_capability_marketplace()).await?;

    // Initialize Planner
    let mut planner = IterativePlanner::new(ccos.clone())?;

    // Plan
    println!("\n🏗️  Building Plan...");
    let final_plan_rtfs = planner.solve(&args.goal, 0).await?;

    println!("\n📝 Generated RTFS Plan:");
    println!("--------------------------------------------------");
    println!("{}", final_plan_rtfs);
    println!("--------------------------------------------------");

    // Execute
    println!("\n🚀 Executing Plan...");
    let plan_obj = ccos::types::Plan {
        plan_id: "iterative-plan".to_string(),
        name: Some("Generated Plan".to_string()),
        body: ccos::types::PlanBody::Rtfs(final_plan_rtfs),
        intent_ids: vec![], // Simplification
        ..Default::default()
    };

    let context = RuntimeContext::full();
    let result = ccos.validate_and_execute_plan(plan_obj, &context).await?;

    println!("\n🏁 Execution Result:");
    println!("   Success: {}", result.success);
    if result.success {
        println!("   Result: {:?}", result.value);
    }
    if let Some(error) = result.metadata.get("error") {
        println!("   Error: {:?}", error);
    }

    // Dump Trace
    println!("\n🔍 Planning Trace:");
    println!("{}", serde_json::to_string_pretty(&planner.trace)?);

    Ok(())
}
