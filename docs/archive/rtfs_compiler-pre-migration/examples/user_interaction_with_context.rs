//! Example demonstrating CCOS/RTFS interaction with context passing between plans
//!
//! This example shows how to pass results from previous plan executions
//! as context to subsequent plan generations, enabling more modular plans.

use rtfs_compiler::ccos::arbiter::ArbiterEngine;
use rtfs_compiler::ccos::CCOS;
use rtfs_compiler::runtime::security::RuntimeContext;
use std::collections::HashMap;
use std::sync::Arc;
use yansi::Paint;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🚀 CCOS/RTFS Context Passing Example");
    println!("=====================================");
    println!();

    // Initialize CCOS with delegation enabled
    let ccos = Arc::new(CCOS::new().await?);
    let ctx = RuntimeContext::pure();

    // Simulate a multi-step planning scenario
    let scenarios = vec![
        "Plan a trip to Paris",
        "Create a detailed itinerary for the trip", // This should use context from previous plan
        "Add cultural activities to the itinerary", // This should use context from both previous plans
    ];

    let mut accumulated_context: HashMap<String, String> = HashMap::new();

    for (i, request) in scenarios.iter().enumerate() {
        println!("{}: {}", format!("Step {}", i + 1).cyan(), request);
        println!("Available context: {:?}", accumulated_context);
        println!();

        // Create a context for plan generation that includes previous results
        let mut plan_context = HashMap::new();
        for (key, value) in &accumulated_context {
            plan_context.insert(key.clone(), value.clone());
        }

        // For demonstration, we'll use the delegating arbiter directly
        // In a real implementation, this would be integrated into the CCOS flow
        if let Some(arbiter) = ccos.get_delegating_arbiter() {
            // Generate intent
            let intent = arbiter.natural_language_to_intent(request, None).await?;

            // Convert to storable intent
            let storable_intent = rtfs_compiler::ccos::types::StorableIntent {
                intent_id: intent.intent_id.clone(),
                name: intent.name.clone(),
                original_request: intent.original_request.clone(),
                rtfs_intent_source: "".to_string(),
                goal: intent.goal.clone(),
                constraints: intent
                    .constraints
                    .iter()
                    .map(|(k, v)| (k.clone(), v.to_string()))
                    .collect(),
                preferences: intent
                    .preferences
                    .iter()
                    .map(|(k, v)| (k.clone(), v.to_string()))
                    .collect(),
                success_criteria: intent.success_criteria.as_ref().map(|v| v.to_string()),
                parent_intent: None,
                child_intents: vec![],
                triggered_by: rtfs_compiler::ccos::types::TriggerSource::HumanRequest,
                generation_context: rtfs_compiler::ccos::types::GenerationContext {
                    arbiter_version: "delegating-1.0".to_string(),
                    generation_timestamp: intent.created_at,
                    input_context: HashMap::new(),
                    reasoning_trace: None,
                },
                status: intent.status.clone(),
                priority: 0,
                created_at: intent.created_at,
                updated_at: intent.updated_at,
                metadata: HashMap::new(),
            };

            // Generate plan (context passing not yet exposed in public API)
            let plan = arbiter.intent_to_plan(&intent).await?;

            println!("Generated plan: {}", plan.plan_id);
            if let Some(name) = &plan.name {
                println!("Plan name: {}", name);
            }
            println!();

            // Simulate plan execution (in real scenario, this would be executed)
            // For demonstration, we'll simulate some results based on the plan
            let simulated_results = simulate_plan_execution(&plan, &accumulated_context);

            // Update accumulated context with new results
            for (key, value) in simulated_results {
                accumulated_context.insert(key, value);
            }

            println!(
                "Plan execution completed. Updated context: {:?}",
                accumulated_context
            );
            println!("----------------------------------------");
            println!();
        }
    }

    println!("✅ Context passing demonstration completed!");
    println!("Final accumulated context: {:?}", accumulated_context);

    Ok(())
}

/// Simulate plan execution and return results that would be passed as context
fn simulate_plan_execution(
    plan: &rtfs_compiler::ccos::types::Plan,
    existing_context: &HashMap<String, String>,
) -> HashMap<String, String> {
    let mut results = HashMap::new();

    // Simulate different types of results based on plan content
    if let Some(name) = &plan.name {
        match name.as_str() {
            name if name.contains("trip") => {
                results.insert("trip/destination".to_string(), "Paris".to_string());
                results.insert("trip/duration".to_string(), "5 days".to_string());
                results.insert("trip/budget".to_string(), "€2000".to_string());
                results.insert("trip/arrival".to_string(), "2024-06-15".to_string());
                results.insert("trip/departure".to_string(), "2024-06-20".to_string());
            }
            name if name.contains("itinerary") => {
                results.insert(
                    "itinerary/activities".to_string(),
                    "museums, parks, restaurants".to_string(),
                );
                results.insert(
                    "itinerary/accommodation".to_string(),
                    "Hotel in Marais district".to_string(),
                );
                results.insert(
                    "itinerary/transport".to_string(),
                    "Metro and walking".to_string(),
                );
            }
            name if name.contains("cultural") => {
                results.insert(
                    "cultural/museums".to_string(),
                    "Louvre, Orsay, Pompidou".to_string(),
                );
                results.insert(
                    "cultural/art_preference".to_string(),
                    "classical and modern".to_string(),
                );
                results.insert("cultural/walking_tolerance".to_string(), "high".to_string());
            }
            _ => {
                results.insert("plan/type".to_string(), "general".to_string());
                results.insert("plan/status".to_string(), "completed".to_string());
            }
        }
    }

    // Add some context from previous executions
    for (key, value) in existing_context {
        if key.starts_with("trip/") {
            results.insert(key.clone(), value.clone());
        }
    }

    results
}
