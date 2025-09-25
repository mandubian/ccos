use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use rtfs_compiler::ccos::{
    arbiter::{ArbiterConfig, ArbiterFactory},
    causal_chain::CausalChain,
    intent_graph::{IntentGraph, IntentGraphConfig},
};
use rtfs_compiler::runtime::values::Value;

/// Example of a standalone arbiter that can be configured and tested independently.
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🚀 CCOS Standalone Arbiter Example");
    println!("==================================");

    // Initialize core components
    let intent_graph = Arc::new(Mutex::new(
        IntentGraph::new_async(IntentGraphConfig::default()).await?,
    ));
    let causal_chain = Arc::new(Mutex::new(CausalChain::new()?));

    // Create arbiter from environment variables or use default
    let arbiter = if let Ok(arbiter) =
        ArbiterFactory::create_arbiter_from_env(intent_graph.clone(), None).await
    {
        println!("✅ Created arbiter from environment configuration");
        arbiter
    } else {
        println!("⚠️  Using default dummy arbiter configuration");
        let config = ArbiterConfig::default();
        ArbiterFactory::create_arbiter(config, intent_graph.clone(), None).await?
    };

    // Test scenarios
    let test_scenarios = vec![
        "Analyze user sentiment from recent interactions",
        "Optimize system performance for better response times",
        "Hello, how are you today?",
        "Generate a report on system health",
    ];

    println!("\n🧪 Running test scenarios...");
    println!("=============================");

    for (i, scenario) in test_scenarios.iter().enumerate() {
        println!("\n📝 Test {}: {}", i + 1, scenario);
        println!("{}", "-".repeat(50));

        // Process the request
        let start = std::time::Instant::now();
        let result = arbiter.process_natural_language(scenario, None).await;
        let duration = start.elapsed();

        match result {
            Ok(execution_result) => {
                println!("✅ Success!");
                println!("   Duration: {:?}", duration);
                println!("   Success: {}", execution_result.success);
                println!("   Result: {:?}", execution_result.value);

                // Record success in causal chain
                {
                    let mut chain = causal_chain
                        .lock()
                        .map_err(|_| "Failed to lock causal chain")?;

                    let mut metadata = HashMap::new();
                    metadata.insert("test_id".to_string(), Value::Integer(i as i64));
                    metadata.insert(
                        "duration_ms".to_string(),
                        Value::Integer(duration.as_millis() as i64),
                    );
                    metadata.insert(
                        "success".to_string(),
                        Value::Boolean(execution_result.success),
                    );

                    // Use a dummy intent ID for the example
                    let dummy_intent_id = format!("test_intent_{}", i);
                    chain.record_delegation_event(
                        &dummy_intent_id,
                        "arbiter.test_success",
                        metadata,
                    );
                }
            }
            Err(error) => {
                println!("❌ Error: {}", error);

                // Record error in causal chain
                {
                    let mut chain = causal_chain
                        .lock()
                        .map_err(|_| "Failed to lock causal chain")?;

                    let mut metadata = HashMap::new();
                    metadata.insert("test_id".to_string(), Value::Integer(i as i64));
                    metadata.insert("error".to_string(), Value::String(error.to_string()));

                    // Use a dummy intent ID for the example
                    let dummy_intent_id = format!("test_intent_{}", i);
                    chain.record_delegation_event(&dummy_intent_id, "arbiter.test_error", metadata);
                }
            }
        }
    }

    // Display causal chain summary
    println!("\n📊 Causal Chain Summary");
    println!("=======================");
    {
        let chain = causal_chain
            .lock()
            .map_err(|_| "Failed to lock causal chain")?;

        // Get the chain length as a simple summary
        println!("Causal chain initialized successfully");
        println!("Total actions: {}", chain.get_all_actions().len());
    }

    // Display intent graph summary
    println!("\n🧠 Intent Graph Summary");
    println!("=======================");
    {
        let graph = intent_graph
            .lock()
            .map_err(|_| "Failed to lock intent graph")?;

        // Get intents from storage
        let intents = graph
            .storage
            .list_intents(rtfs_compiler::ccos::intent_storage::IntentFilter::default())
            .await?;
        println!("Total intents stored: {}", intents.len());

        for intent in intents.iter().take(3) {
            println!(
                "  - {}: {}",
                intent.name.as_deref().unwrap_or("unnamed"),
                intent.goal
            );
        }

        if intents.len() > 3 {
            println!("  ... and {} more intents", intents.len() - 3);
        }
    }

    println!("\n🎉 Standalone arbiter example completed successfully!");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_standalone_arbiter_basic() {
        let intent_graph = Arc::new(Mutex::new(
            IntentGraph::new_async(IntentGraphConfig::default())
                .await
                .unwrap(),
        ));
        let causal_chain = Arc::new(Mutex::new(CausalChain::new().unwrap()));

        let arbiter = ArbiterFactory::create_dummy_arbiter(intent_graph);

        let result = arbiter.process_natural_language("Hello", None).await;
        assert!(result.is_ok());

        let execution_result = result.unwrap();
        assert!(execution_result.success);
    }

    #[tokio::test]
    async fn test_standalone_arbiter_sentiment() {
        let intent_graph = Arc::new(Mutex::new(
            IntentGraph::new_async(IntentGraphConfig::default())
                .await
                .unwrap(),
        ));
        let causal_chain = Arc::new(Mutex::new(CausalChain::new().unwrap()));

        let arbiter = ArbiterFactory::create_dummy_arbiter(intent_graph);

        let result = arbiter
            .process_natural_language("Analyze sentiment", None)
            .await;
        assert!(result.is_ok());

        let execution_result = result.unwrap();
        assert!(execution_result.success);
    }
}
