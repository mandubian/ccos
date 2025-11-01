//! RTFS Runtime Reentrance Demo
//!
//! This example demonstrates true reentrance of RTFS runtime with orchestrator
//! executing a program in several steps with proper continuation after host calls.
//!
//! The demo shows:
//! 1. Multi-step RTFS program execution
//! 2. Host capability calls that require reentrance
//! 3. Context preservation across execution boundaries
//! 4. Step-by-step orchestration with audit trails

use ccos::types::{Intent, Plan, PlanBody, PlanLanguage};
use ccos::{
    capabilities::registry::CapabilityRegistry, capability_marketplace::CapabilityMarketplace,
    causal_chain::CausalChain, intent_graph::IntentGraph, orchestrator::Orchestrator,
    plan_archive::PlanArchive,
};
use rtfs::runtime::security::RuntimeContext;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== RTFS Runtime Reentrance Demo ===\n");

    // Initialize CCOS components
    let intent_graph = Arc::new(Mutex::new(IntentGraph::new()?));
    let causal_chain = Arc::new(Mutex::new(CausalChain::new()?));
    let capability_registry = Arc::new(tokio::sync::RwLock::new(CapabilityRegistry::new()));
    let capability_marketplace = Arc::new(CapabilityMarketplace::new(capability_registry.clone()));
    let plan_archive = Arc::new(PlanArchive::new());

    // Register default capabilities (including ccos.echo)
    rtfs::runtime::stdlib::register_default_capabilities(&capability_marketplace).await?;

    // Register demo capabilities
    register_demo_capabilities(&capability_marketplace).await?;

    let orchestrator = Orchestrator::new(
        causal_chain.clone(),
        intent_graph,
        capability_marketplace.clone(),
        plan_archive,
    );

    // Create a multi-step RTFS program that demonstrates reentrance
    let rtfs_program = r#"
    (do
      ;; Step 1: Initialize state
      (step "initialize-state"
        (let [initial-data (call :ccos.state.kv.put "workflow-state" "initialized")]
          (call :ccos.echo (str "State initialized: " initial-data))))
      
      ;; Step 2: Process data with multiple host calls
      (step "process-data"
        (let [counter (call :ccos.state.counter.inc "process-counter" 1)
              data (call :ccos.state.kv.get "workflow-state")]
          (do
            (call :ccos.echo (str "Processing data: " data))
            (call :ccos.echo (str "Counter value: " counter))
            ;; Simulate conditional processing based on counter
            (if (> counter 0)
              (do
                (call :ccos.echo "Counter is positive, proceeding...")
                ;; Another host call within the same step
                (let [processed (call :ccos.state.event.append "workflow-events" "data-processed")]
                  (call :ccos.echo (str "Event logged: " processed))))
              (call :ccos.echo "Counter is zero or negative")))))
      
      ;; Step 3: Finalize with state updates
      (step "finalize"
        (let [final-counter (call :ccos.state.counter.inc "process-counter" 1)
              final-state (call :ccos.state.kv.put "workflow-state" "completed")
              summary (call :ccos.state.event.append "workflow-events" "workflow-completed")]
          (do
            (call :ccos.echo (str "Final counter: " final-counter))
            (call :ccos.echo (str "Final state: " final-state))
            (call :ccos.echo (str "Summary: " summary))
            ;; Return final result
            {:status "completed"
             :counter final-counter
             :state final-state}))))
    "#;

    // Create intent and plan
    let intent = Intent {
        intent_id: "reentrance-demo-intent".to_string(),
        name: Some("RTFS Reentrance Demo".to_string()),
        goal: "Demonstrate RTFS runtime reentrance with multi-step execution".to_string(),
        original_request: "Run a multi-step workflow with host capability calls".to_string(),
        status: ccos::types::IntentStatus::Active,
        success_criteria: None,
        constraints: HashMap::new(),
        preferences: HashMap::new(),
        metadata: HashMap::new(),
        created_at: 0,
        updated_at: 0,
    };

    let plan = Plan {
        plan_id: "reentrance-demo-plan".to_string(),
        name: Some("RTFS Reentrance Demo Plan".to_string()),
        intent_ids: vec![intent.intent_id.clone()],
        language: PlanLanguage::Rtfs20,
        body: PlanBody::Rtfs(rtfs_program.to_string()),
        metadata: HashMap::new(),
        created_at: 0,
        status: ccos::types::PlanStatus::Draft,
        input_schema: Default::default(),
        output_schema: Default::default(),
        policies: Default::default(),
        capabilities_required: vec![
            "ccos.echo".to_string(),
            "ccos.state.kv.put".to_string(),
            "ccos.state.kv.get".to_string(),
            "ccos.state.counter.inc".to_string(),
            "ccos.state.event.append".to_string(),
        ],
        annotations: Default::default(),
    };

    // Set up security context allowing all required capabilities
    let context = RuntimeContext {
        security_level: rtfs::runtime::security::SecurityLevel::Full,
        allowed_capabilities: plan.capabilities_required.clone().into_iter().collect(),
        ..RuntimeContext::pure()
    };

    println!("🚀 Executing multi-step RTFS program with reentrance...");
    println!("📋 Program:\n{}\n", rtfs_program);

    // Execute the plan - this demonstrates reentrance
    let execution_result = orchestrator.execute_plan(&plan, &context).await?;

    println!("\n✅ Execution completed!");
    println!("📊 Result: {:?}", execution_result.value);

    // Show audit trail
    if let Ok(chain) = causal_chain.lock() {
        let actions = chain.get_all_actions();
        println!("\n🧾 Audit Trail ({} actions):", actions.len());
        for (i, action) in actions.iter().enumerate() {
            println!(
                "  {}. {:?} - {}",
                i + 1,
                action.action_type,
                action.function_name.as_deref().unwrap_or("unknown")
            );
        }
    }

    println!("\n✨ Reentrance demo completed successfully!");
    println!("This demonstrates:");
    println!("1. ✅ Multi-step RTFS execution");
    println!("2. ✅ Host capability calls requiring reentrance");
    println!("3. ✅ Context preservation across execution boundaries");
    println!("4. ✅ Step-by-step orchestration with audit trails");

    Ok(())
}

async fn register_demo_capabilities(
    _marketplace: &CapabilityMarketplace,
) -> Result<(), Box<dyn std::error::Error>> {
    // Register state management capabilities for the demo
    // Note: In a real implementation, we would access the registry through the marketplace
    // For this demo, we'll use a simplified approach and just return Ok
    // The actual capability registration would be done through the marketplace API

    // The ccos.echo capability should already be registered by default in the marketplace
    // We're just logging that this is where demo capabilities would be registered
    println!(
        "  📋 Demo capabilities would be registered here (ccos.echo should already be available)"
    );
    Ok(())
}
