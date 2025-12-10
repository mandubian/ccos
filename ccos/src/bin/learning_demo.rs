//! Learning Loop Demo:
//! - Registers demo capabilities (some that fail)
//! - Executes capabilities and logs failures to CausalChain
//! - Uses learning capabilities to analyze failures and suggest improvements
//! - Demonstrates the full learning feedback loop

use ccos::capability_marketplace::CapabilityMarketplace;
use ccos::causal_chain::CausalChain;
use ccos::learning::capabilities::{
    register_learning_capabilities, AnalyzeFailureInput, GetFailureStatsInput, GetFailuresInput,
};
use ccos::types::{Action, ActionType, ExecutionResult};
use ccos::utils::value_conversion::rtfs_value_to_json;
use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use rtfs::runtime::values::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::RwLock;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║           CCOS Learning Loop Demo                            ║");
    println!("╚══════════════════════════════════════════════════════════════╝\n");

    // Initialize components
    let chain = Arc::new(Mutex::new(CausalChain::new().expect("chain")));
    let marketplace = Arc::new(CapabilityMarketplace::new(Arc::new(RwLock::new(
        ccos::capabilities::registry::CapabilityRegistry::new(),
    ))));

    // Register demo capabilities (including ones that fail)
    register_demo_capabilities(&marketplace).await?;

    // Register learning capabilities
    register_learning_capabilities(Arc::clone(&marketplace), Arc::clone(&chain)).await?;

    println!("📦 Registered demo capabilities + learning capabilities\n");

    // =========================================================================
    // Phase 1: Execute capabilities (some succeed, some fail)
    // =========================================================================
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("📋 PHASE 1: Execute Capabilities (generating failures)");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

    let plan_id = "learning-demo-plan";
    let intent_id = "learning-demo-intent";

    // Success: echo capability
    execute_and_record(
        &chain,
        &marketplace,
        plan_id,
        intent_id,
        "demo.echo",
        make_map(vec![("message", Value::String("Hello!".to_string()))]),
    )
    .await;

    // Failure: missing field (schema error)
    execute_and_record(
        &chain,
        &marketplace,
        plan_id,
        intent_id,
        "demo.validate_schema",
        Value::Map(HashMap::new()), // Missing required 'name' field
    )
    .await;

    // Failure: timeout simulation
    execute_and_record(
        &chain,
        &marketplace,
        plan_id,
        intent_id,
        "demo.slow_operation",
        Value::Map(HashMap::new()),
    )
    .await;

    // Failure: missing capability
    execute_and_record(
        &chain,
        &marketplace,
        plan_id,
        intent_id,
        "demo.nonexistent",
        Value::Map(HashMap::new()),
    )
    .await;

    // Failure: network error simulation
    execute_and_record(
        &chain,
        &marketplace,
        plan_id,
        intent_id,
        "demo.network_call",
        make_map(vec![(
            "url",
            Value::String("http://invalid.local".to_string()),
        )]),
    )
    .await;

    // Success: add numbers
    execute_and_record(
        &chain,
        &marketplace,
        plan_id,
        intent_id,
        "demo.add",
        make_map(vec![("a", Value::Integer(5)), ("b", Value::Integer(3))]),
    )
    .await;

    println!();

    // =========================================================================
    // Phase 2: Query failures using learning capabilities
    // =========================================================================
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("📊 PHASE 2: Query Failures (learning.get_failures)");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

    let input_json = serde_json::to_value(GetFailuresInput {
        capability_id: None,
        error_category: None,
        limit: Some(10),
    })?;
    let input_value = ccos::utils::value_conversion::json_to_rtfs_value(&input_json)?;

    let result = marketplace
        .execute_capability_enhanced("learning.get_failures", &input_value, None)
        .await?;
    let result_json = rtfs_value_to_json(&result)?;
    println!("Failures found:");
    println!("{}\n", serde_json::to_string_pretty(&result_json)?);

    // =========================================================================
    // Phase 3: Get failure statistics
    // =========================================================================
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("📈 PHASE 3: Failure Statistics (learning.get_failure_stats)");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

    let stats_input = serde_json::to_value(GetFailureStatsInput {
        capability_id: None,
    })?;
    let stats_value = ccos::utils::value_conversion::json_to_rtfs_value(&stats_input)?;

    let stats = marketplace
        .execute_capability_enhanced("learning.get_failure_stats", &stats_value, None)
        .await?;
    let stats_json = rtfs_value_to_json(&stats)?;
    println!("Failure statistics:");
    println!("{}\n", serde_json::to_string_pretty(&stats_json)?);

    // =========================================================================
    // Phase 4: Analyze specific failure patterns
    // =========================================================================
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("🔍 PHASE 4: Analyze Failure (learning.analyze_failure)");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

    let error_messages = vec![
        "Schema validation failed: missing field 'name'",
        "Unknown capability: demo.nonexistent not found in marketplace",
        "Connection timeout after 30000ms",
        "Network error: failed to connect to http://invalid.local",
    ];

    for msg in error_messages {
        let analyze_input = serde_json::to_value(AnalyzeFailureInput {
            error_message: msg.to_string(),
            capability_id: None,
        })?;
        let analyze_value = ccos::utils::value_conversion::json_to_rtfs_value(&analyze_input)?;

        let analysis = marketplace
            .execute_capability_enhanced("learning.analyze_failure", &analyze_value, None)
            .await?;
        let analysis_json = rtfs_value_to_json(&analysis)?;
        println!("Error: \"{}\"", msg);
        println!(
            "Analysis: {}\n",
            serde_json::to_string_pretty(&analysis_json)?
        );
    }

    // =========================================================================
    // Phase 5: Summary - What the system learned
    // =========================================================================
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("🧠 PHASE 5: Learning Summary");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

    println!("The learning loop has:");
    println!("  1. ✅ Executed capabilities and recorded results to CausalChain");
    println!("  2. ✅ Classified errors by category (Schema, Missing, Timeout, Network)");
    println!("  3. ✅ Provided actionable suggestions for each error type");
    println!("  4. ✅ Aggregated statistics to identify problem areas");
    println!();
    println!("In production, 'learning.suggest_improvement' would use an LLM to:");
    println!("  • Analyze failure patterns across multiple executions");
    println!("  • Suggest schema fixes for frequently failing capabilities");
    println!("  • Recommend retry strategies for transient errors");
    println!("  • Propose new capabilities to fill missing gaps");
    println!();

    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║           Demo Complete - Learning Loop Working!              ║");
    println!("╚══════════════════════════════════════════════════════════════╝");

    Ok(())
}

async fn register_demo_capabilities(marketplace: &CapabilityMarketplace) -> RuntimeResult<()> {
    // Echo capability - always succeeds
    marketplace
        .register_local_capability(
            "demo.echo".to_string(),
            "Echo".to_string(),
            "Echo the input message".to_string(),
            Arc::new(|input: &Value| Ok(input.clone())),
        )
        .await?;

    // Add capability - always succeeds
    marketplace
        .register_local_capability(
            "demo.add".to_string(),
            "Add".to_string(),
            "Add two numbers".to_string(),
            Arc::new(|input: &Value| {
                use rtfs::ast::MapKey;
                if let Value::Map(map) = input {
                    let a = map
                        .get(&MapKey::String("a".to_string()))
                        .and_then(|v| {
                            if let Value::Integer(i) = v {
                                Some(*i)
                            } else {
                                None
                            }
                        })
                        .unwrap_or(0);
                    let b = map
                        .get(&MapKey::String("b".to_string()))
                        .and_then(|v| {
                            if let Value::Integer(i) = v {
                                Some(*i)
                            } else {
                                None
                            }
                        })
                        .unwrap_or(0);
                    return Ok(Value::Integer(a + b));
                }
                Ok(Value::Integer(0))
            }),
        )
        .await?;

    // Schema validator - fails if 'name' is missing
    marketplace
        .register_local_capability(
            "demo.validate_schema".to_string(),
            "Validate Schema".to_string(),
            "Validates input has required 'name' field".to_string(),
            Arc::new(|input: &Value| {
                use rtfs::ast::MapKey;
                if let Value::Map(map) = input {
                    if map.get(&MapKey::String("name".to_string())).is_none() {
                        return Err(RuntimeError::Generic(
                            "Schema validation failed: missing field 'name'".to_string(),
                        ));
                    }
                    return Ok(Value::Boolean(true));
                }
                Err(RuntimeError::Generic(
                    "Schema validation failed: expected map input".to_string(),
                ))
            }),
        )
        .await?;

    // Slow operation - simulates timeout
    marketplace
        .register_local_capability(
            "demo.slow_operation".to_string(),
            "Slow Operation".to_string(),
            "Simulates a slow operation that times out".to_string(),
            Arc::new(|_input: &Value| {
                Err(RuntimeError::Generic(
                    "Connection timeout after 30000ms".to_string(),
                ))
            }),
        )
        .await?;

    // Network call - simulates network error
    marketplace
        .register_local_capability(
            "demo.network_call".to_string(),
            "Network Call".to_string(),
            "Simulates a network call that fails".to_string(),
            Arc::new(|input: &Value| {
                use rtfs::ast::MapKey;
                let url = if let Value::Map(map) = input {
                    map.get(&MapKey::String("url".to_string()))
                        .and_then(|v| {
                            if let Value::String(s) = v {
                                Some(s.clone())
                            } else {
                                None
                            }
                        })
                        .unwrap_or_else(|| "unknown".to_string())
                } else {
                    "unknown".to_string()
                };
                Err(RuntimeError::Generic(format!(
                    "Network error: failed to connect to {}",
                    url
                )))
            }),
        )
        .await?;

    Ok(())
}

async fn execute_and_record(
    chain: &Arc<Mutex<CausalChain>>,
    marketplace: &Arc<CapabilityMarketplace>,
    plan_id: &str,
    intent_id: &str,
    capability_id: &str,
    args: Value,
) {
    print!("  Calling {} ... ", capability_id);

    let action = Action::new(
        ActionType::CapabilityCall,
        plan_id.to_string(),
        intent_id.to_string(),
    )
    .with_name(capability_id);

    // Record the call
    let _ = chain.lock().unwrap().append(&action);

    // Execute
    let exec_result = marketplace
        .execute_capability_enhanced(capability_id, &args, None)
        .await;

    match exec_result {
        Ok(value) => {
            println!("✅ Success: {:?}", value);
            let result = ExecutionResult {
                success: true,
                value,
                metadata: HashMap::new(),
            };
            let _ = chain.lock().unwrap().record_result(action, result);
        }
        Err(e) => {
            let error_msg = format!("{}", e);
            println!("❌ Failed: {}", error_msg);

            // Classify the error
            let error_category = classify_error(&error_msg);

            let mut meta = HashMap::new();
            meta.insert("error".to_string(), Value::String(error_msg));
            meta.insert("error_category".to_string(), Value::String(error_category));

            let result = ExecutionResult {
                success: false,
                value: Value::Nil,
                metadata: meta,
            };
            let _ = chain.lock().unwrap().record_result(action, result);
        }
    }
}

/// Helper to create Value::Map with MapKey::String keys
fn make_map(entries: Vec<(&str, Value)>) -> Value {
    use rtfs::ast::MapKey;
    let map: HashMap<MapKey, Value> = entries
        .into_iter()
        .map(|(k, v)| (MapKey::String(k.to_string()), v))
        .collect();
    Value::Map(map)
}

fn classify_error(msg: &str) -> String {
    let lower = msg.to_lowercase();
    if lower.contains("schema")
        || lower.contains("validation failed")
        || lower.contains("missing field")
    {
        "SchemaError".to_string()
    } else if lower.contains("unknown capability") || lower.contains("not found") {
        "MissingCapability".to_string()
    } else if lower.contains("timeout") || lower.contains("timed out") {
        "TimeoutError".to_string()
    } else if lower.contains("network") || lower.contains("connection") {
        "NetworkError".to_string()
    } else {
        "RuntimeError".to_string()
    }
}
