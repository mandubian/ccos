//! Robust Planning Demo
//!
//! This example demonstrates a robust workflow for converting a complex natural language goal
//! into a list of capabilities and a plan, avoiding brittle keyword heuristics.
//!
//! Workflow:
//! 1. **Decomposition**: Uses LLM to break the goal into logical steps.
//! 2. **Discovery**: Uses Semantic Search (via Catalog) to find relevant capabilities for each step.
//! 3. **Selection**: Uses LLM to select the best capability from candidates and extract arguments.
//! 4. **Planning**: Generates a structured plan.
//!
//! Usage:
//!   cargo run --example robust_planning_demo -- --goal "find the issues of repository ccos and user mandubian and filter them to keep only those containing RTFS"

use clap::Parser;
use std::error::Error;
use std::sync::Arc;
use std::collections::HashMap;

use ccos::CCOS;
use rtfs::config::types::AgentConfig;
use ccos::arbiter::DelegatingArbiter;
use ccos::synthesis::mcp_registry_client::McpRegistryClient;
use rtfs::runtime::values::Value;
use rtfs::runtime::error::RuntimeResult;

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

#[derive(Debug, serde::Deserialize)]
struct PlannedStep {
    description: String,
    capability_hint: String,
}

#[derive(Debug, serde::Deserialize)]
struct Decomposition {
    steps: Vec<PlannedStep>,
}

#[derive(Debug)]
enum SelectionSource {
    Local,
    Remote,
    Missing,
}

struct CapabilitySummary {
    step: String,
    capability: String,
    source: SelectionSource,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();
    println!("🚀 Robust Planning Demo");
    println!("Goal: {}", args.goal);

    // 1. Initialize CCOS
    let agent_config = load_agent_config(&args.config)?;
    apply_llm_profile(&agent_config, args.profile.as_deref())?;

    // Ensure model is set if not provided by profile
    if std::env::var("CCOS_DELEGATING_MODEL").is_err() {
        println!("⚠️ CCOS_DELEGATING_MODEL not set, defaulting to 'gpt-4o'");
        std::env::set_var("CCOS_DELEGATING_MODEL", "gpt-4o");
    }
    
    // Force delegation enabled for this demo
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

    let arbiter = ccos.get_delegating_arbiter()
        .ok_or("Delegating arbiter not available. Check configuration.")?;
    let marketplace = ccos.get_capability_marketplace();
    let catalog = ccos.get_catalog();

    // Ensure required capabilities exist (mock if necessary)
    ensure_github_capability(&marketplace).await;

    // 2. Decompose Goal
    println!("\n🧩 Decomposing goal into steps...");
    let steps = decompose_goal(&arbiter, &args.goal).await?;
    
    for (i, step) in steps.iter().enumerate() {
        println!("  {}. {} (Hint: {})", i+1, step.description, step.capability_hint);
    }

    // 3. Discovery & Selection Loop
    println!("\n🔍 Discovery & Selection Loop...");
    let mut selected_capabilities = Vec::new();
    let mut summary_log = Vec::new();

    for step in steps {
        println!("\n👉 Processing Step: {}", step.description);

        // A. Semantic Search
        // Use the hint + description for search
        let query = format!("{} {}", step.capability_hint, step.description);
        println!("   🔎 Searching catalog for: '{}'", query);
        
        // Use catalog to find candidates
        // We filter for 'Capability' kind
        let filter = ccos::catalog::CatalogFilter::for_kind(ccos::catalog::CatalogEntryKind::Capability);
        let hits = catalog.search_semantic(&query, Some(&filter), 5); // Top 5
        
        if hits.is_empty() {
            println!("   ⚠️ No capabilities found in catalog. Trying keyword search...");
            // Fallback?
        }

        // Proper fetch:
        let mut candidates = Vec::new();
        for hit in hits {
            if let Some(cap) = marketplace.get_capability(&hit.entry.id).await {
                candidates.push(cap);
            }
        }

        println!("   📋 Found {} candidates:", candidates.len());
        for c in &candidates {
            println!("      - {} ({})", c.id, c.description);
        }

        if candidates.is_empty() {
            println!("   ❌ No candidates found in local catalog. Trying external registry...");
            if let Some(found_remote) = search_external_registry(step.description.as_str(), step.capability_hint.as_str()).await {
                summary_log.push(CapabilitySummary {
                    step: step.description.clone(),
                    capability: found_remote,
                    source: SelectionSource::Remote,
                });
            } else {
                summary_log.push(CapabilitySummary {
                    step: step.description.clone(),
                    capability: "None".to_string(),
                    source: SelectionSource::Missing,
                });
            }
            continue;
        }

        // B. LLM Selection & Argument Extraction
        
        let tool_names: Vec<String> = candidates.iter().map(|c| c.id.clone()).collect();
        let mut tool_schemas = HashMap::new();
        for c in &candidates {
            if let Some(schema) = &c.input_schema {
                // Convert TypeExpr to JSON for the prompt (best effort)
                let json_schema = serde_json::json!({
                    "description": format!("RTFS Schema: {:?}", schema) // Simplified
                });
                tool_schemas.insert(c.id.clone(), json_schema);
            }
        }

        println!("   🤖 Asking Arbiter to select best tool from {} candidates...", candidates.len());
        
        // We want to allow "NO_MATCH"
        // Custom selection logic using raw text generation for flexibility
        match select_tool_robust(&arbiter, &step.description, &tool_names).await {
            Ok(Some((selected_id, args))) => {
                println!("   ✅ Selected: {}", selected_id);
                println!("   📦 Arguments: {:?}", args);
                selected_capabilities.push((selected_id.clone(), args));
                summary_log.push(CapabilitySummary {
                    step: step.description.clone(),
                    capability: selected_id,
                    source: SelectionSource::Local,
                });
            },
            Ok(None) => {
                println!("   🤔 Arbiter rejected all local candidates (low confidence).");
                println!("   🌐 Switching to External MCP Registry Search...");
                if let Some(found_remote) = search_external_registry(step.description.as_str(), step.capability_hint.as_str()).await {
                    summary_log.push(CapabilitySummary {
                        step: step.description.clone(),
                        capability: found_remote,
                        source: SelectionSource::Remote,
                    });
                } else {
                    summary_log.push(CapabilitySummary {
                        step: step.description.clone(),
                        capability: "None".to_string(),
                        source: SelectionSource::Missing,
                    });
                }
            },
            Err(e) => {
                println!("   ⚠️ Selection failed: {}", e);
            }
        }
    }

    // 4. Plan Summary
    println!("\n📝 Final Capability List for Plan:");
    for (i, (cap, args)) in selected_capabilities.iter().enumerate() {
        println!("{}. {} {:?}", i+1, cap, args);
    }

    // 5. Discovery Summary
    println!("\n📊 Capability Discovery Summary:");
    println!("--------------------------------------------------");
    
    println!("🟢 Found Locally:");
    let local: Vec<_> = summary_log.iter().filter(|s| matches!(s.source, SelectionSource::Local)).collect();
    if local.is_empty() { println!("   (None)"); }
    for s in local {
        println!("   - Step: \"{}\" -> Used: {}", s.step, s.capability);
    }

    println!("\n🔵 Found Remotely (MCP Registry):");
    let remote: Vec<_> = summary_log.iter().filter(|s| matches!(s.source, SelectionSource::Remote)).collect();
    if remote.is_empty() { println!("   (None)"); }
    for s in remote {
        println!("   - Step: \"{}\" -> Discovered: {}", s.step, s.capability);
    }

    println!("\n🔴 Missing / To Synthesize:");
    let missing: Vec<_> = summary_log.iter().filter(|s| matches!(s.source, SelectionSource::Missing)).collect();
    if missing.is_empty() { println!("   (None)"); }
    for s in missing {
        println!("   - Step: \"{}\" -> No match found", s.step);
    }
    println!("--------------------------------------------------");

    Ok(())
}

async fn search_external_registry(description: &str, hint: &str) -> Option<String> {
    println!("      🔎 Querying MCP Registry for '{}'...", hint);
    let client = McpRegistryClient::new();
    match client.search_servers(hint).await {
        Ok(servers) => {
            if servers.is_empty() {
                println!("      ❌ No servers found in registry.");
                println!("      🛠️  Fallback: Synthesize new RTFS capability for '{}'", description);
                None
            } else {
                println!("      ✅ Found {} servers in registry.", servers.len());
                for s in servers.iter().take(3) {
                    println!("         - {} ({})", s.name, s.description);
                }
                println!("      👉 (Simulation) Would install and use best match.");
                // Return the name of the first server as the "found" capability source
                Some(format!("mcp.registry.{}", servers[0].name))
            }
        }
        Err(e) => {
            println!("      ⚠️ Registry search failed: {}", e);
            None
        }
    }
}

async fn select_tool_robust(
    arbiter: &DelegatingArbiter,
    goal: &str,
    tools: &[String],
) -> Result<Option<(String, HashMap<String, String>)>, Box<dyn Error>> {
    let prompt = format!(
        r#"You are an expert tool selector.
Goal: "{}"

Available Tools:
{}

Select the tool that BEST matches the goal.
If NONE of the tools are a good match, respond with "NO_MATCH".

If you select a tool, extract the arguments from the goal.
Respond in this JSON format:
{{
  "tool": "tool_name_or_NO_MATCH",
  "arguments": {{ "arg1": "value1" }}
}}
"#,
        goal,
        tools.join("\n")
    );

    let response = arbiter.generate_raw_text(&prompt).await?;
    
    // Extract JSON block
    let json_str = if let Some(start) = response.find('{') {
        if let Some(end) = response.rfind('}') {
            &response[start..=end]
        } else {
            &response
        }
    } else {
        &response
    };

    #[derive(serde::Deserialize)]
    struct Selection {
        tool: String,
        arguments: HashMap<String, String>,
    }

    let selection: Selection = serde_json::from_str(json_str)?;
    
    if selection.tool == "NO_MATCH" {
        Ok(None)
    } else {
        Ok(Some((selection.tool, selection.arguments)))
    }
}

async fn decompose_goal(arbiter: &DelegatingArbiter, goal: &str) -> Result<Vec<PlannedStep>, Box<dyn Error>> {
    let prompt = format!(
        r#"You are an expert planner. Decompose the following goal into a sequence of logical steps.
For each step, provide a description and a short "capability hint" (e.g. "calendar.list_events", "email.send").

Goal: "{}"

Respond ONLY with a JSON object in this format:
{{
  "steps": [
    {{ "description": "Fetch today's meetings", "capability_hint": "calendar.list" }},
    {{ "description": "Email the summary to the team", "capability_hint": "email.send" }}
  ]
}}
"#,
        goal
    );

    let response = arbiter.generate_raw_text(&prompt).await?;
    
    // Extract JSON block
    let json_str = if let Some(start) = response.find('{') {
        if let Some(end) = response.rfind('}') {
            &response[start..=end]
        } else {
            &response
        }
    } else {
        &response
    };

    let decomposition: Decomposition = serde_json::from_str(json_str)?;
    Ok(decomposition.steps)
}

async fn ensure_github_capability(marketplace: &Arc<ccos::capability_marketplace::CapabilityMarketplace>) {
    if marketplace.get_capability("mcp.github.list_issues").await.is_none() {
        println!("⚠️ 'mcp.github.list_issues' not found. Registering mock for demo...");
        // Register a mock capability with a rich description for semantic search
        let handler = Arc::new(|_args: &Value| -> RuntimeResult<Value> {
            Ok(Value::Nil)
        });
        
        let _ = marketplace.register_local_capability(
            "mcp.github.list_issues".to_string(),
            "List GitHub Issues".to_string(),
            "List issues from a GitHub repository. Supports filtering by owner, repo, state, and other criteria. Useful for retrieving issue data.".to_string(),
            handler
        ).await;
        println!("   ✅ Registered mock 'mcp.github.list_issues'");
    }
}

fn load_agent_config(config_path: &str) -> Result<AgentConfig, Box<dyn Error>> {
    let mut content = std::fs::read_to_string(config_path)?;
    if content.starts_with("# RTFS") {
        content = content.lines().skip(1).collect::<Vec<_>>().join("\n");
    }
    toml::from_str(&content).map_err(|e| format!("failed to parse agent config: {}", e).into())
}

fn apply_llm_profile(agent_config: &AgentConfig, profile: Option<&str>) -> Result<(), Box<dyn Error>> {
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
