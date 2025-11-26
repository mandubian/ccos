//! Modular Planner Demo
//!
//! This example demonstrates the new modular planning architecture that:
//! 1. Uses pluggable decomposition strategies (pattern-first, then LLM fallback)
//! 2. Properly stores all intents in the IntentGraph as real nodes
//! 3. Uses resolution strategies to map semantic intents to capabilities
//! 4. Generates executable RTFS plans from resolved capabilities
//! 5. EXECUTES the generated plan using the CCOS runtime
//!
//! The key difference from autonomous_agent_demo is that this architecture:
//! - Separates WHAT (decomposition produces semantic intents) from HOW (resolution finds capabilities)
//! - Uses pattern matching first for common goal structures (fast, deterministic)
//! - Falls back to LLM only when patterns don't match
//! - Stores all planning decisions in IntentGraph for audit/reuse
//!
//! Usage:
//!   cargo run --example modular_planner_demo -- --goal "list issues in mandubian/ccos but ask me for the page size"

use std::error::Error;
use std::sync::Arc;

use clap::Parser;
use ccos::CCOS;
use ccos::planner::modular_planner::{
    ModularPlanner, PlannerConfig,
    PatternDecomposition,
    CatalogResolution,
    ResolvedCapability,
    DecompositionStrategy,
};
use ccos::planner::modular_planner::decomposition::HybridDecomposition;
use ccos::planner::modular_planner::decomposition::llm_adapter::CcosLlmAdapter;
use ccos::arbiter::llm_provider::{LlmProviderFactory, LlmProviderConfig, LlmProviderType};
use ccos::planner::modular_planner::resolution::{
    CompositeResolution, McpResolution,
};
use ccos::synthesis::mcp_session::MCPSessionManager;
use ccos::planner::modular_planner::resolution::semantic::{CapabilityCatalog, CapabilityInfo};
use ccos::planner::modular_planner::orchestrator::{PlanResult, TraceEvent};
use ccos::capabilities::{SessionPoolManager, MCPSessionHandler};
use rtfs::runtime::security::RuntimeContext;
use rtfs::config::types::AgentConfig;

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

    /// Path to agent config file
    #[arg(long, default_value = "config/agent_config.toml")]
    config: String,
    
    /// Execute the plan after generation
    #[arg(long, default_value_t = true)]
    execute: bool,
}

// ============================================================================
// CCOS Catalog Adapter
// ============================================================================

/// Adapts the CCOS CatalogService to the CapabilityCatalog trait required by the planner
struct CcosCatalogAdapter {
    catalog: Arc<ccos::catalog::CatalogService>,
}

impl CcosCatalogAdapter {
    fn new(catalog: Arc<ccos::catalog::CatalogService>) -> Self {
        Self { catalog }
    }
}

#[async_trait::async_trait(?Send)]
impl CapabilityCatalog for CcosCatalogAdapter {
    async fn list_capabilities(&self, _domain: Option<&str>) -> Vec<CapabilityInfo> {
        // Return all capabilities (limit to 100 for sanity)
        let hits = self.catalog.search_keyword("", None, 100);
        hits.into_iter().map(catalog_hit_to_info).collect()
    }

    async fn get_capability(&self, id: &str) -> Option<CapabilityInfo> {
        // Search specifically for this ID
        let hits = self.catalog.search_keyword(id, None, 10);
        hits.into_iter()
            .find(|h| h.entry.id == id)
            .map(catalog_hit_to_info)
    }

    async fn search(&self, query: &str, limit: usize) -> Vec<CapabilityInfo> {
        // Use semantic search from CCOS catalog
        let hits = self.catalog.search_semantic(query, None, limit);
        hits.into_iter().map(catalog_hit_to_info).collect()
    }
}

/// Helper to convert catalog hit to capability info
fn catalog_hit_to_info(hit: ccos::catalog::CatalogHit) -> CapabilityInfo {
    CapabilityInfo {
        id: hit.entry.id,
        name: hit.entry.name.unwrap_or_else(|| "unknown".to_string()),
        description: hit.entry.description.unwrap_or_default(),
        input_schema: None, // We don't need full schema for resolution matching
    }
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
    
    // 0. Initialize CCOS Environment
    println!("🔧 Initializing CCOS Environment...");
    let agent_config = load_agent_config(&args.config)?;
    
    // Ensure delegation is enabled for LLM
    std::env::set_var("CCOS_DELEGATION_ENABLED", "true");
    if std::env::var("CCOS_DELEGATING_MODEL").is_err() {
        std::env::set_var("CCOS_DELEGATING_MODEL", "deepseek/deepseek-v3.2-exp");
    }

    let ccos = Arc::new(
        CCOS::new_with_agent_config_and_configs_and_debug_callback(
            Default::default(),
            None,
            Some(agent_config.clone()),
            None,
        )
        .await?,
    );
    
    // Register basic tools (like ccos.user.ask)
    ccos::capabilities::defaults::register_default_capabilities(&ccos.get_capability_marketplace()).await?;

    // Configure session pool for MCP execution
    let mut session_pool_manager = SessionPoolManager::new();
    session_pool_manager.register_handler("mcp", std::sync::Arc::new(MCPSessionHandler::new()));
    let session_pool = std::sync::Arc::new(session_pool_manager);
    ccos.get_capability_marketplace().set_session_pool(session_pool.clone()).await;
    println!("   ✅ Session pool configured with MCPSessionHandler");

    // 1. Use IntentGraph from CCOS
    println!("🔧 Using IntentGraph from CCOS...");
    let intent_graph = ccos.get_intent_graph();
    
    // 2. Build capability catalog using adapter
    println!("\n🔍 Setting up capability catalog...");
    // Wrap the real CCOS catalog
    let catalog = Arc::new(CcosCatalogAdapter::new(ccos.get_catalog()));
    
    // 3. Create decomposition strategy
    println!("\n📐 Setting up decomposition strategy...");
    
    // Create LLM provider from agent config
    // Try to find a configured profile, otherwise fall back to defaults
    let llm_config = if let Some(ref profiles) = agent_config.llm_profiles {
        if let Some(default_name) = &profiles.default {
            if let Some(profile) = profiles.profiles.iter().find(|p| &p.name == default_name) {
                println!("   Using LLM Profile: {}", profile.name);
                let provider_type = match profile.provider.as_str() {
                    "openai" => LlmProviderType::OpenAI,
                    "anthropic" => LlmProviderType::Anthropic,
                    "stub" => LlmProviderType::Stub,
                    _ => LlmProviderType::OpenAI, 
                };
                
                LlmProviderConfig {
                    provider_type,
                    model: profile.model.clone(),
                    api_key: profile.api_key.clone().or_else(|| {
                        profile.api_key_env.as_ref().and_then(|env| std::env::var(env).ok())
                    }),
                    base_url: profile.base_url.clone(),
                    max_tokens: profile.max_tokens,
                    temperature: profile.temperature,
                    timeout_seconds: None,
                    retry_config: Default::default(),
                }
            } else {
                 LlmProviderConfig {
                    provider_type: LlmProviderType::OpenAI,
                    model: "openai/gpt-4o".to_string(),
                    api_key: std::env::var("OPENROUTER_API_KEY").ok(),
                    base_url: Some("https://openrouter.ai/api/v1".to_string()),
                    max_tokens: None,
                    temperature: None,
                    timeout_seconds: None,
                    retry_config: Default::default(),
                }
            }
        } else {
             LlmProviderConfig {
                provider_type: LlmProviderType::OpenAI,
                model: "openai/gpt-4o".to_string(),
                api_key: std::env::var("OPENROUTER_API_KEY").ok(),
                base_url: Some("https://openrouter.ai/api/v1".to_string()),
                max_tokens: None,
                temperature: None,
                timeout_seconds: None,
                retry_config: Default::default(),
            }
        }
    } else {
         LlmProviderConfig {
            provider_type: LlmProviderType::OpenAI,
            model: "openai/gpt-4o".to_string(),
            api_key: std::env::var("OPENROUTER_API_KEY").ok(),
            base_url: Some("https://openrouter.ai/api/v1".to_string()),
            max_tokens: None,
            temperature: None,
            timeout_seconds: None,
            retry_config: Default::default(),
        }
    };
    
    let mut decomposition: Box<dyn DecompositionStrategy> = Box::new(PatternDecomposition::new());

    // Try to create LLM provider and upgrade to Hybrid
    match LlmProviderFactory::create_provider(llm_config).await {
        Ok(provider) => {
            println!("   ✅ LLM Provider initialized for Hybrid Decomposition");
            let adapter = Arc::new(CcosLlmAdapter::new(provider));
            
            let hybrid = HybridDecomposition::new()
                .with_llm(adapter);
                
            decomposition = Box::new(hybrid);
        },
        Err(e) => {
            println!("   ⚠️  Failed to init LLM provider: {}. Falling back to Pattern-only.", e);
        }
    }
    
    // 4. Create resolution strategy (Composite: Catalog + MCP)
    let mut composite_resolution = CompositeResolution::new();
    
    // A. Catalog Resolution (for local/builtin)
    composite_resolution.add_strategy(Box::new(CatalogResolution::new(catalog.clone())));
    
    // B. MCP Resolution (for remote tools)
    // Create separate session manager for discovery
    let mut auth_headers = std::collections::HashMap::new();
    if let Ok(token) = std::env::var("MCP_AUTH_TOKEN") {
        if !token.is_empty() {
             auth_headers.insert("Authorization".to_string(), format!("Bearer {}", token));
        }
    }
    let discovery_session_manager = Arc::new(MCPSessionManager::new(Some(auth_headers)));
    
    // Create runtime MCP discovery using our real session pool
    use ccos::planner::modular_planner::resolution::mcp::RuntimeMcpDiscovery;
    let mcp_discovery = Arc::new(RuntimeMcpDiscovery::new(
        discovery_session_manager,
        ccos.get_capability_marketplace(),
    ));
    
    let mcp_resolution = McpResolution::new(mcp_discovery);
    // TODO: Add embedding provider if available
    
    if args.discover_mcp {
        println!("   ✅ Enabled MCP Resolution");
        composite_resolution.add_strategy(Box::new(mcp_resolution));
    } else {
        println!("   ⏭️ Skipping MCP Resolution (use --discover-mcp to enable)");
    }
    
    // 5. Create the modular planner
    let config = PlannerConfig {
        max_depth: 5,
        persist_intents: true,
        create_edges: true,
        intent_namespace: "demo".to_string(),
    };
    
    let mut planner = ModularPlanner::new(decomposition, Box::new(composite_resolution), intent_graph.clone())
        .with_config(config);
    
    // 6. Plan!
    println!("\n🚀 Planning...\n");
    
    let plan_result = match planner.plan(&args.goal).await {
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
            
            Some(result)
        }
        Err(e) => {
            println!("\n❌ Planning failed: {}", e);
            println!("\n💡 Tip: The pattern decomposition only handles specific goal patterns:");
            println!("   - \"X but ask me for Y\"");
            println!("   - \"ask me for X then Y\"");
            println!("   - \"X then Y\"");
            println!("   - \"X and filter/sort by Y\"");
            None
        }
    };
    
    // 7. Execute!
    if let Some(result) = plan_result {
        if args.execute {
            println!("\n⚡ Executing Plan...");
            println!("───────────────────────────────────────────────────────────────");
            
            let plan_obj = ccos::types::Plan {
                plan_id: format!("modular-plan-{}", uuid::Uuid::new_v4()),
                name: Some("Modular Plan".to_string()),
                body: ccos::types::PlanBody::Rtfs(result.rtfs_plan.clone()),
                intent_ids: result.intent_ids.clone(),
                ..Default::default()
            };

            let context = RuntimeContext::full();
            match ccos.validate_and_execute_plan(plan_obj, &context).await {
                Ok(exec_result) => {
                    println!("\n🏁 Execution Result:");
                    println!("   Success: {}", exec_result.success);
                    
                    // Format output nicely
                    let output_str = value_to_string(&exec_result.value);
                    println!("   Result: {}", output_str);
                    
                    if !exec_result.success {
                        if let Some(err) = exec_result.metadata.get("error") {
                            println!("   Error: {:?}", err);
                        }
                    }
                },
                Err(e) => {
                    println!("\n❌ Execution Failed: {}", e);
                }
            }
        }
    }
    
    println!("\n✅ Demo complete!");
    Ok(())
}

/// Helper to load config (copied from autonomous_agent_demo)
fn load_agent_config(config_path: &str) -> Result<AgentConfig, Box<dyn Error + Send + Sync>> {
    let path = std::path::Path::new(config_path);
    let actual_path = if path.exists() {
        path.to_path_buf()
    } else {
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
    
    let mut content = std::fs::read_to_string(&actual_path)
        .map_err(|e| format!("Failed to read config file '{}': {}", actual_path.display(), e))?;
    if content.starts_with("# RTFS") {
        content = content.lines().skip(1).collect::<Vec<_>>().join("\n");
    }
    toml::from_str(&content).map_err(|e| format!("failed to parse agent config: {}", e).into())
}

/// Convert RTFS value to string for display
fn value_to_string(v: &rtfs::runtime::values::Value) -> String {
    format!("{:?}", v)
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

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_pattern_decomposition() {
        use ccos::intent_graph::{IntentGraph, config::IntentGraphConfig};
        use std::sync::Mutex;

        let intent_graph = Arc::new(Mutex::new(
            IntentGraph::with_config(IntentGraphConfig::with_in_memory_storage()).unwrap()
        ));
        
        // Mock catalog for test (since we can't easily spin up CCOS here)
        struct MockCatalog;
        #[async_trait::async_trait(?Send)]
        impl CapabilityCatalog for MockCatalog {
            async fn list_capabilities(&self, _domain: Option<&str>) -> Vec<CapabilityInfo> { vec![] }
            async fn get_capability(&self, _id: &str) -> Option<CapabilityInfo> { None }
            async fn search(&self, _query: &str, _limit: usize) -> Vec<CapabilityInfo> { vec![] }
        }
        let catalog = Arc::new(MockCatalog);
        
        let mut planner = ModularPlanner::new(
            Box::new(PatternDecomposition::new()),
            Box::new(CatalogResolution::new(catalog)),
            intent_graph,
        );
        
        let result = planner.plan("list issues but ask me for page size").await.unwrap();
        
        assert_eq!(result.intent_ids.len(), 2);
        assert!(result.rtfs_plan.contains("ccos.user.ask"));
    }
}
