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
use ccos::planner::modular_planner::resolution::{
    CompositeResolution, McpResolution, CatalogConfig, ScoringMethod,
};
use ccos::synthesis::mcp_session::MCPSessionManager;
use ccos::planner::modular_planner::resolution::semantic::{CapabilityCatalog, CapabilityInfo};
use ccos::planner::modular_planner::orchestrator::{PlanResult, TraceEvent};
use ccos::capabilities::{SessionPoolManager, MCPSessionHandler};
use ccos::discovery::embedding_service::EmbeddingService;
use rtfs::runtime::security::RuntimeContext;
use rtfs::config::types::{AgentConfig, LlmProfile};
use ccos::arbiter::llm_provider::{LlmProviderConfig, LlmProviderType, LlmProviderFactory};
use ccos::planner::modular_planner::decomposition::llm_adapter::CcosLlmAdapter;
use ccos::planner::modular_planner::decomposition::HybridDecomposition;

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
    
    /// Show LLM prompts and responses (verbose LLM debugging)
    #[arg(long)]
    verbose_llm: bool,

    /// Discover tools from MCP servers (requires GITHUB_TOKEN)
    #[arg(long)]
    discover_mcp: bool,

    /// Path to agent config file
    #[arg(long, default_value = "config/agent_config.toml")]
    config: String,
    
    /// Execute the plan after generation
    #[arg(long, default_value_t = true)]
    execute: bool,

    /// Force pure LLM decomposition (skip patterns)
    #[arg(long)]
    pure_llm: bool,
    
    /// Use embedding-based scoring (default: true, use --no-embeddings to disable)
    #[arg(long, default_value_t = true)]
    use_embeddings: bool,
    
    /// Disable tool cache (force fresh MCP discovery)
    #[arg(long)]
    no_cache: bool,
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
        // Use keyword search which has been improved for tokenized matching
        let hits = self.catalog.search_keyword(query, None, limit);
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
    
    // CRITICAL: Refresh the catalog with all registered capabilities
    ccos.get_catalog().ingest_marketplace(&ccos.get_capability_marketplace()).await;
    
    // Wrap the real CCOS catalog
    let catalog = Arc::new(CcosCatalogAdapter::new(ccos.get_catalog()));
    
    // 3. Create decomposition strategy (Hybrid: Pattern + LLM)
    println!("\n📐 Setting up decomposition strategy...");
    
    let decomposition: Box<dyn DecompositionStrategy> = {
        // Use profile from config
        let profile_name = "openrouter_free:balanced";
        
        if let Some(profile) = find_llm_profile(&agent_config, profile_name) {
            println!("   ✅ Found LLM profile '{}', enabling HybridDecomposition", profile_name);
            
            // Resolve API key
            let api_key = profile.api_key.clone()
                .or_else(|| profile.api_key_env.as_ref().and_then(|env| std::env::var(env).ok()));
                
            if let Some(key) = api_key {
                // Map provider string to enum
                let provider_type = match profile.provider.as_str() {
                    "openai" => LlmProviderType::OpenAI,
                    "anthropic" => LlmProviderType::Anthropic,
                    "stub" => LlmProviderType::Stub,
                    "local" => LlmProviderType::Local,
                    "openrouter" => LlmProviderType::OpenAI, // OpenRouter uses OpenAI client
                    _ => {
                        println!("   ⚠️ Unknown provider '{}', defaulting to OpenAI", profile.provider);
                        LlmProviderType::OpenAI
                    }
                };
                
                let provider_config = LlmProviderConfig {
                    provider_type,
                    model: profile.model,
                    api_key: Some(key),
                    base_url: profile.base_url,
                    max_tokens: profile.max_tokens.or(Some(4096)),
                    temperature: profile.temperature.or(Some(0.0)),
                    timeout_seconds: Some(60),
                    retry_config: Default::default(),
                };
                
                match LlmProviderFactory::create_provider(provider_config).await {
                    Ok(provider) => {
                        let adapter = Arc::new(CcosLlmAdapter::new(provider));
                        let mut hybrid = HybridDecomposition::new().with_llm(adapter);
                        
                        // If pure LLM requested, configure hybrid to skip patterns
                        if args.pure_llm {
                            println!("   🤖 Pure LLM mode enabled (skipping patterns)");
                            // Set pattern threshold > 1.0 to ensure patterns never match
                            hybrid = hybrid.with_config(ccos::planner::modular_planner::decomposition::hybrid::HybridConfig {
                                pattern_confidence_threshold: 2.0, 
                                prefer_grounded: true,
                                max_grounded_tools: 20,
                            });
                        }
                        
                        Box::new(hybrid)
                    },
                    Err(e) => {
                        println!("   ⚠️ Failed to create LLM provider: {}. Falling back to Pattern.", e);
                        Box::new(PatternDecomposition::new())
                    }
                }
            } else {
                println!("   ⚠️ No API key found for profile '{}'. Using PatternDecomposition only.", profile_name);
                Box::new(PatternDecomposition::new())
            }
        } else {
            println!("   ⚠️ Profile '{}' not found in config. Using PatternDecomposition only.", profile_name);
            Box::new(PatternDecomposition::new())
        }
    };
    
    // 4. Create resolution strategy (Composite: Catalog + MCP)
    let mut composite_resolution = CompositeResolution::new();
    
    // A. Catalog Resolution (for local/builtin) - disable schema validation for mock capabilities
    let catalog_config = CatalogConfig {
        validate_schema: false,
        allow_adaptation: true,
        scoring_method: if args.use_embeddings { ScoringMethod::Hybrid } else { ScoringMethod::Heuristic },
        embedding_threshold: 0.5,
    };
    
    // Create catalog resolution, optionally with embedding service
    let catalog_resolution = {
        let base = CatalogResolution::new(catalog.clone()).with_config(catalog_config);
        
        if args.use_embeddings {
            if let Some(embedding_service) = EmbeddingService::from_env() {
                println!("   ✅ Using embedding-based scoring ({})", embedding_service.provider_description());
                base.with_embedding_service(embedding_service)
            } else {
                println!("   ⚠️ --use-embeddings specified but no embedding provider configured.");
                println!("      Set LOCAL_EMBEDDING_URL (e.g., http://localhost:11434/api) for Ollama");
                println!("      Or OPENROUTER_API_KEY for remote embeddings");
                println!("      Falling back to heuristic scoring.");
                base
            }
        } else {
            println!("   📊 Using heuristic-based scoring (use --use-embeddings for semantic matching)");
            base
        }
    };
    
    composite_resolution.add_strategy(Box::new(catalog_resolution));
    
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
    
    // Setup cache directory for MCP tools
    let cache_dir = std::path::PathBuf::from("capabilities/discovered/mcp");
    let mcp_resolution = McpResolution::new(mcp_discovery)
        .with_cache_dir(cache_dir)
        .with_no_cache(args.no_cache);
    
    if args.discover_mcp {
        println!("   ✅ Enabled MCP Resolution (cache: capabilities/discovered/mcp/)");
        if args.no_cache {
            println!("   🔄 Cache disabled, will refresh from server");
        }
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
        verbose_llm: args.verbose_llm,
        eager_discovery: true,
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

/// Find an LLM profile in the agent config by name (e.g. "gpt4" or "openrouter:fast")
fn find_llm_profile(config: &AgentConfig, profile_name: &str) -> Option<LlmProfile> {
    let profiles_config = config.llm_profiles.as_ref()?;
    
    // 1. Check explicit profiles
    if let Some(profile) = profiles_config.profiles.iter().find(|p| p.name == profile_name) {
        return Some(profile.clone());
    }
    
    // 2. Check model sets (format: "set_name:spec_name")
    if let Some(model_sets) = &profiles_config.model_sets {
        if let Some((set_name, spec_name)) = profile_name.split_once(':') {
            if let Some(set) = model_sets.iter().find(|s| s.name == set_name) {
                if let Some(spec) = set.models.iter().find(|m| m.name == spec_name) {
                    // Construct synthetic profile
                    return Some(LlmProfile {
                        name: profile_name.to_string(),
                        provider: set.provider.clone(),
                        model: spec.model.clone(),
                        base_url: set.base_url.clone(),
                        api_key_env: set.api_key_env.clone(),
                        api_key: set.api_key.clone(),
                        temperature: None, // Could be added to spec if needed
                        max_tokens: spec.max_output_tokens,
                    });
                }
            }
        }
    }
    
    None
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
