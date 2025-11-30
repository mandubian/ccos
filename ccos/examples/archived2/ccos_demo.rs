use ccos::arbiter::delegating_arbiter::DelegatingArbiter;
use ccos::arbiter::arbiter_config::{DelegationConfig, LlmConfig, LlmProviderType, RetryConfig};
use ccos::intent_graph::IntentGraph;
use ccos::capability_marketplace::CapabilityMarketplace;
use rtfs::runtime::capabilities::registry::CapabilityRegistry;
use std::sync::Arc;
use tokio::sync::RwLock;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // CI-safe guard: skip creating reactive runtime in CI
    if std::env::var("CI").is_ok() {
        println!("ccos_demo: running in CI, skipping runtime demo (no external effects)");
        return Ok(());
    }

    // Create an intent graph
    let ig = IntentGraph::new()?;
    let intent_graph = Arc::new(std::sync::Mutex::new(ig));

    // Create a minimal capability marketplace using tokio::sync::RwLock
    let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
    let marketplace = Arc::new(CapabilityMarketplace::new(registry));

    // Create LLM configuration with stub provider
    let llm_config = LlmConfig {
        provider_type: LlmProviderType::Stub,
        model: "stub-model".to_string(),
        api_key: None,
        base_url: None,
        max_tokens: Some(1000),
        temperature: Some(0.7),
        timeout_seconds: Some(30),
        prompts: None,
        retry_config: RetryConfig::default(),
    };

    // Create delegation configuration
    let delegation_config = DelegationConfig {
        enabled: true,
        threshold: 0.65,
        max_candidates: 3,
        min_skill_hits: Some(1),
        agent_registry: ccos::arbiter::arbiter_config::AgentRegistryConfig::default(),
        adaptive_threshold: None,
        print_extracted_intent: None,
        print_extracted_plan: None,
    };

    // Create delegating arbiter
    let _arbiter = DelegatingArbiter::new(
        llm_config,
        delegation_config,
        marketplace,
        intent_graph,
    ).await?;

    println!("=== CCOS + RTFS Cognitive Computing Demo ===\n");

    // Demo 1: Basic Arbiter Creation
    println!("✅ DelegatingArbiter created successfully with default configuration");
    println!("   - Intent graph initialized");
    println!("   - Delegation configuration applied");
    println!("   - Uses LLM-driven intent classification");

    // Demo 2: Show configuration
    println!("\n📋 DelegatingArbiter Configuration:");
    println!("   - LLM provider: Stub (fallback)");
    println!("   - Intent graph ready for use");
    println!("   - Enhanced delegation capabilities");

    println!("\n🎯 Demo completed successfully!");
    println!("===================================");

    Ok(())
}
