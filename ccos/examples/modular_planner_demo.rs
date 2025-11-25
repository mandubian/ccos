//! Modular Planner Demo
//!
//! This example demonstrates the new modular planning architecture that:
//! 1. Uses pluggable decomposition strategies (pattern-first, then LLM fallback)
//! 2. Properly stores all intents in the IntentGraph as real nodes
//! 3. Uses resolution strategies to map semantic intents to capabilities
//! 4. Generates executable RTFS plans from resolved capabilities
//!
//! The key difference from autonomous_agent_demo is that this architecture:
//! - Separates WHAT (decomposition produces semantic intents) from HOW (resolution finds capabilities)
//! - Uses pattern matching first for common goal structures (fast, deterministic)
//! - Falls back to LLM only when patterns don't match
//! - Stores all planning decisions in IntentGraph for audit/reuse
//!
//! Usage:
//!   cargo run --example modular_planner_demo -- --goal "list issues in mandubian/ccos but ask me for the page size"

use std::collections::HashMap;
use std::error::Error;
use std::sync::{Arc, Mutex};

use clap::Parser;
use ccos::intent_graph::IntentGraph;
use ccos::intent_graph::config::IntentGraphConfig;
use ccos::planner::modular_planner::{
    ModularPlanner, PlannerConfig,
    PatternDecomposition,
    CatalogResolution,
    ResolvedCapability,
    DecompositionStrategy,
};
use ccos::planner::modular_planner::resolution::semantic::{CapabilityCatalog, CapabilityInfo};
use ccos::planner::modular_planner::orchestrator::{PlanResult, TraceEvent};

// ============================================================================
// CLI Arguments
// ============================================================================

#[derive(Parser, Debug)]
struct Args {
    /// Natural language goal
    #[arg(long, default_value = "list issues in mandubian/ccos but ask me for the page size")]
    goal: String,

    /// Show detailed planning trace
    #[arg(long)]
    verbose: bool,

    /// Discover tools from MCP servers (requires GITHUB_TOKEN)
    #[arg(long)]
    discover_mcp: bool,
}

// ============================================================================
// MCP Tool Catalog (bridges to MCP discovery)
// ============================================================================

/// A catalog that queries MCP servers for available tools
struct McpToolCatalog {
    /// Discovered MCP tools
    tools: Vec<CapabilityInfo>,
}

impl McpToolCatalog {
    fn new() -> Self {
        Self { tools: Vec::new() }
    }

    /// Discover tools from configured MCP servers
    async fn discover_from_servers(&mut self) -> Result<(), Box<dyn Error + Send + Sync>> {
        // Try to discover from GitHub MCP server
        if let Some((server_url, server_name)) = resolve_mcp_server("github") {
            println!("  📡 Discovering tools from MCP server: {}", server_name);
            
            match discover_mcp_tools(&server_url, &server_name).await {
                Ok(tools) => {
                    println!("     Found {} tools", tools.len());
                    self.tools.extend(tools);
                }
                Err(e) => {
                    println!("     ⚠️ Failed to discover tools: {}", e);
                }
            }
        } else {
            println!("  ⚠️ No MCP server configured (check capabilities/mcp/overrides.json)");
        }
        
        Ok(())
    }
}

#[async_trait::async_trait(?Send)]
impl CapabilityCatalog for McpToolCatalog {
    async fn list_capabilities(&self, domain: Option<&str>) -> Vec<CapabilityInfo> {
        match domain {
            Some(d) => self.tools.iter()
                .filter(|t| t.id.to_lowercase().contains(&d.to_lowercase()))
                .cloned()
                .collect(),
            None => self.tools.clone(),
        }
    }

    async fn get_capability(&self, id: &str) -> Option<CapabilityInfo> {
        self.tools.iter().find(|t| t.id == id).cloned()
    }

    async fn search(&self, query: &str, limit: usize) -> Vec<CapabilityInfo> {
        let query_lower = query.to_lowercase();
        let query_words: Vec<&str> = query_lower.split_whitespace().collect();
        
        let mut scored: Vec<(CapabilityInfo, usize)> = self.tools.iter()
            .map(|t| {
                let text = format!("{} {}", t.name, t.description).to_lowercase();
                let score = query_words.iter()
                    .filter(|w| w.len() > 2 && text.contains(*w))
                    .count();
                (t.clone(), score)
            })
            .filter(|(_, score)| *score > 0)
            .collect();
        
        scored.sort_by(|a, b| b.1.cmp(&a.1));
        scored.into_iter().take(limit).map(|(t, _)| t).collect()
    }
}

// ============================================================================
// MCP Discovery Helpers
// ============================================================================

/// Resolve MCP server URL from overrides file
fn resolve_mcp_server(hint: &str) -> Option<(String, String)> {
    let overrides_path = std::path::Path::new("capabilities/mcp/overrides.json");
    if !overrides_path.exists() {
        // Try parent directory
        let parent_path = std::path::Path::new("../capabilities/mcp/overrides.json");
        if !parent_path.exists() {
            return None;
        }
    }
    
    let path = if overrides_path.exists() {
        overrides_path
    } else {
        std::path::Path::new("../capabilities/mcp/overrides.json")
    };
    
    let content = std::fs::read_to_string(path).ok()?;
    let overrides: serde_json::Value = serde_json::from_str(&content).ok()?;
    
    let hint_lower = hint.to_lowercase();
    
    if let Some(servers) = overrides.get("servers").and_then(|s| s.as_object()) {
        for (name, info) in servers {
            if name.to_lowercase().contains(&hint_lower) {
                if let Some(url) = info.get("url").and_then(|u| u.as_str()) {
                    return Some((url.to_string(), name.clone()));
                }
            }
        }
    }
    
    None
}

/// Discover tools from an MCP server
async fn discover_mcp_tools(server_url: &str, server_name: &str) -> Result<Vec<CapabilityInfo>, Box<dyn Error + Send + Sync>> {
    use ccos::synthesis::mcp_session::{MCPSessionManager, MCPServerInfo};
    
    let auth_headers = get_mcp_auth_headers();
    let session_manager = MCPSessionManager::new(Some(auth_headers));
    
    let client_info = MCPServerInfo {
        name: "modular-planner-demo".to_string(),
        version: "1.0.0".to_string(),
    };
    
    let session = session_manager.initialize_session(server_url, &client_info).await?;
    let tools_resp = session_manager.make_request(&session, "tools/list", serde_json::json!({})).await?;
    
    let mut capabilities = Vec::new();
    
    if let Some(tools) = tools_resp.get("tools").and_then(|t| t.as_array()) {
        for tool in tools {
            let name = tool.get("name").and_then(|n| n.as_str()).unwrap_or("unknown");
            let description = tool.get("description").and_then(|d| d.as_str()).unwrap_or("");
            
            capabilities.push(CapabilityInfo {
                id: format!("mcp.{}.{}", server_name, name),
                name: name.to_string(),
                description: description.to_string(),
                input_schema: tool.get("inputSchema").map(|s| s.to_string()),
            });
        }
    }
    
    Ok(capabilities)
}

/// Get authentication headers for MCP requests
fn get_mcp_auth_headers() -> HashMap<String, String> {
    let mut headers = HashMap::new();
    
    if let Ok(token) = std::env::var("GITHUB_TOKEN") {
        headers.insert("Authorization".to_string(), format!("Bearer {}", token));
    } else if let Ok(token) = std::env::var("MCP_AUTH_TOKEN") {
        headers.insert("Authorization".to_string(), format!("Bearer {}", token));
    }
    
    headers
}

// ============================================================================
// Main Demo
// ============================================================================

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    let args = Args::parse();
    
    println!("\n╔══════════════════════════════════════════════════════════════╗");
    println!("║           🧩 Modular Planner Demo                            ║");
    println!("╚══════════════════════════════════════════════════════════════╝\n");
    
    println!("📋 Goal: \"{}\"\n", args.goal);
    
    // 1. Initialize IntentGraph
    println!("🔧 Initializing IntentGraph...");
    let intent_graph = Arc::new(Mutex::new(
        IntentGraph::with_config(IntentGraphConfig::with_in_memory_storage())?
    ));
    
    // 2. Build capability catalog
    println!("\n🔍 Setting up capability catalog...");
    let mut catalog = McpToolCatalog::new();
    
    if args.discover_mcp {
        catalog.discover_from_servers().await?;
    } else {
        println!("  ⏭️ Skipping MCP discovery (use --discover-mcp to enable)");
    }
    
    let catalog = Arc::new(catalog);
    
    // 3. Create decomposition strategy (pattern-only for this demo)
    println!("\n📐 Using PatternDecomposition (fast, deterministic)");
    let decomposition: Box<dyn DecompositionStrategy> = Box::new(PatternDecomposition::new());
    
    // 4. Create resolution strategy (catalog with builtin support)
    let resolution = Box::new(CatalogResolution::new(catalog.clone()));
    
    // 5. Create the modular planner
    let config = PlannerConfig {
        max_depth: 5,
        persist_intents: true,
        create_edges: true,
        intent_namespace: "demo".to_string(),
    };
    
    let mut planner = ModularPlanner::new(decomposition, resolution, intent_graph.clone())
        .with_config(config);
    
    // 6. Plan!
    println!("\n🚀 Planning...\n");
    
    match planner.plan(&args.goal).await {
        Ok(result) => {
            print_plan_result(&result, args.verbose);
            
            // Show IntentGraph state
            println!("\n📊 IntentGraph State:");
            let graph = intent_graph.lock().unwrap();
            println!("   Root intent: {}", &result.root_intent_id[..40.min(result.root_intent_id.len())]);
            println!("   Total intents created: {}", result.intent_ids.len() + 1); // +1 for root
            
            if let Some(root) = graph.get_intent(&result.root_intent_id) {
                println!("   Root goal: \"{}\"", root.goal);
            }
        }
        Err(e) => {
            println!("\n❌ Planning failed: {}", e);
            println!("\n💡 Tip: The pattern decomposition only handles specific goal patterns:");
            println!("   - \"X but ask me for Y\"");
            println!("   - \"ask me for X then Y\"");
            println!("   - \"X then Y\"");
            println!("   - \"X and filter/sort by Y\"");
        }
    }
    
    println!("\n✅ Demo complete!");
    Ok(())
}

/// Print the plan result
fn print_plan_result(result: &PlanResult, verbose: bool) {
    println!("═══════════════════════════════════════════════════════════════");
    println!("📋 Plan Result");
    println!("═══════════════════════════════════════════════════════════════\n");
    
    // Show resolved steps
    println!("📝 Resolved Steps ({}):", result.intent_ids.len());
    for (i, intent_id) in result.intent_ids.iter().enumerate() {
        if let Some(resolution) = result.resolutions.get(intent_id) {
            let (status, cap_id) = match resolution {
                ResolvedCapability::Local { capability_id, .. } => ("Local", capability_id.as_str()),
                ResolvedCapability::Remote { capability_id, .. } => ("Remote", capability_id.as_str()),
                ResolvedCapability::BuiltIn { capability_id, .. } => ("BuiltIn", capability_id.as_str()),
                ResolvedCapability::Synthesized { capability_id, .. } => ("Synth", capability_id.as_str()),
                ResolvedCapability::NeedsReferral { reason, .. } => ("Referral", reason.as_str()),
            };
            println!("   {}. [{}] {}", i + 1, status, cap_id);
        }
    }
    
    // Show generated RTFS plan
    println!("\n📜 Generated RTFS Plan:");
    println!("───────────────────────────────────────────────────────────────");
    println!("{}", result.rtfs_plan);
    println!("───────────────────────────────────────────────────────────────");
    
    // Show trace if verbose
    if verbose {
        println!("\n🔍 Planning Trace:");
        for event in &result.trace.events {
            match event {
                TraceEvent::DecompositionStarted { strategy } => {
                    println!("   → Decomposition started with strategy: {}", strategy);
                }
                TraceEvent::DecompositionCompleted { num_intents, confidence } => {
                    println!("   ✓ Decomposition completed: {} intents, confidence: {:.2}", num_intents, confidence);
                }
                TraceEvent::IntentCreated { intent_id, description } => {
                    println!("   + Intent created: {} - \"{}\"", &intent_id[..20.min(intent_id.len())], description);
                }
                TraceEvent::EdgeCreated { from, to, edge_type } => {
                    println!("   ⟶ Edge: {} -> {} ({})", &from[..16.min(from.len())], &to[..16.min(to.len())], edge_type);
                }
                TraceEvent::ResolutionStarted { intent_id } => {
                    println!("   🔍 Resolving: {}", &intent_id[..20.min(intent_id.len())]);
                }
                TraceEvent::ResolutionCompleted { intent_id, capability } => {
                    println!("   ✓ Resolved: {} → {}", &intent_id[..16.min(intent_id.len())], capability);
                }
                TraceEvent::ResolutionFailed { intent_id, reason } => {
                    println!("   ✗ Failed: {} - {}", &intent_id[..16.min(intent_id.len())], reason);
                }
            }
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_pattern_decomposition() {
        let intent_graph = Arc::new(Mutex::new(
            IntentGraph::with_config(IntentGraphConfig::with_in_memory_storage()).unwrap()
        ));
        
        let catalog = Arc::new(McpToolCatalog::new());
        
        let mut planner = ModularPlanner::new(
            Box::new(PatternDecomposition::new()),
            Box::new(CatalogResolution::new(catalog)),
            intent_graph,
        );
        
        let result = planner.plan("list issues but ask me for page size").await.unwrap();
        
        assert_eq!(result.intent_ids.len(), 2);
        assert!(result.rtfs_plan.contains("ccos.user.ask"));
    }
    
    #[tokio::test]
    async fn test_mcp_catalog_search() {
        let mut catalog = McpToolCatalog::new();
        catalog.tools.push(CapabilityInfo {
            id: "mcp.github.list_issues".to_string(),
            name: "list_issues".to_string(),
            description: "List issues in a GitHub repository".to_string(),
            input_schema: None,
        });
        
        let results = catalog.search("list issues repository", 5).await;
        assert!(!results.is_empty());
        assert_eq!(results[0].id, "mcp.github.list_issues");
    }
}
