//! Minimal demo: executes a hardcoded plan with two `ccos.user.ask` prompts.
//! Run with `cargo run --example user_ask_two_prompts` and answer the questions.

use rtfs_compiler::ccos::capabilities::registry::CapabilityRegistry;
use rtfs_compiler::ccos::capability_marketplace::CapabilityMarketplace;
use rtfs_compiler::ccos::causal_chain::CausalChain;
use rtfs_compiler::ccos::event_sink::CausalChainIntentEventSink;
use rtfs_compiler::ccos::intent_graph::IntentGraph;
use rtfs_compiler::ccos::orchestrator::Orchestrator;
use rtfs_compiler::ccos::plan_archive::PlanArchive;
use rtfs_compiler::ccos::types::{Plan, PlanBody, PlanLanguage, PlanStatus};
use rtfs_compiler::runtime::security::{RuntimeContext, SecurityLevel};
use std::sync::{Arc, Mutex};
use tokio::sync::RwLock;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Core CCOS components for plan execution.
    let causal_chain = Arc::new(Mutex::new(CausalChain::new()?));
    let event_sink = Arc::new(CausalChainIntentEventSink::new(Arc::clone(&causal_chain)));
    let intent_graph = Arc::new(Mutex::new(IntentGraph::with_event_sink(event_sink)?));
    let capability_registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
    let marketplace = Arc::new(CapabilityMarketplace::with_causal_chain(
        Arc::clone(&capability_registry),
        Some(Arc::clone(&causal_chain)),
    ));
    marketplace.bootstrap().await?;
    let plan_archive = Arc::new(PlanArchive::new());

    let orchestrator = Orchestrator::new(
        Arc::clone(&causal_chain),
        Arc::clone(&intent_graph),
        Arc::clone(&marketplace),
        Arc::clone(&plan_archive),
    );

    // Hardcoded RTFS plan with two `ccos.user.ask` calls.
    let rtfs_plan = r#"(do
  (step "GatherName" (let [name (call :ccos.user.ask "What is your name?")] {:status "asked" :name name}))
  (step "GatherDestination" (let [destination (call :ccos.user.ask "Where do you want to travel?")] {:status "asked" :destination destination}))
  (step "Finalize" (call :ccos.echo {:message "Thanks for the information!"}))
)"#;

    let plan = Plan {
        plan_id: "demo-plan-two-asks".to_string(),
        name: Some("two-user-ask-demo".to_string()),
        intent_ids: vec!["demo-intent".to_string()],
        language: PlanLanguage::Rtfs20,
        body: PlanBody::Rtfs(rtfs_plan.to_string()),
        status: PlanStatus::Active,
        created_at: 0,
        metadata: Default::default(),
        input_schema: None,
        output_schema: None,
        policies: Default::default(),
        capabilities_required: vec!["ccos.user.ask".to_string()],
        annotations: Default::default(),
    };

    let context = RuntimeContext {
        security_level: SecurityLevel::Controlled,
        allowed_capabilities: vec!["ccos.echo".to_string(), "ccos.user.ask".to_string()]
            .into_iter()
            .collect(),
        ..RuntimeContext::pure()
    };

    println!("\nRunning hardcoded plan with two prompts...\n");
    let result = orchestrator.execute_plan(&plan, &context).await?;
    println!("Execution success: {}", result.success);
    println!("Final value: {}", result.value);

    Ok(())
}
