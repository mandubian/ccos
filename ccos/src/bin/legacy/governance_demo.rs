//! Two-Tier Governance Demo:
//! Tests the complete execution hints governance flow:
//! 1. Valid hints pass validation
//! 2. Policy-violating hints are rejected
//! 3. Governance checkpoints logged to CausalChain
//!
//! Run with: cargo run --bin governance_demo

use ccos::capability_marketplace::CapabilityMarketplace;
use ccos::causal_chain::CausalChain;
use ccos::governance_kernel::ExecutionHintPolicies;
use ccos::types::{Action, ActionType};
use rtfs::ast::{Keyword, MapKey};
use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use rtfs::runtime::execution_outcome::CallMetadata;
use rtfs::runtime::values::Value;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::RwLock;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║       Two-Tier Governance Demo                               ║");
    println!("╚══════════════════════════════════════════════════════════════╝\n");

    // Initialize components
    let chain = Arc::new(Mutex::new(CausalChain::new().expect("chain")));
    let marketplace = Arc::new(CapabilityMarketplace::new(Arc::new(RwLock::new(
        ccos::capabilities::registry::CapabilityRegistry::new(),
    ))));

    // Register test capabilities
    register_test_capabilities(&marketplace).await?;

    println!("📦 Registered test capabilities\n");

    // Get the default policies for testing
    let policies = ExecutionHintPolicies::default();
    println!("📜 Default Hint Policies:");
    println!("   - max_retries: {}", policies.max_retries);
    println!(
        "   - max_timeout_multiplier: {}",
        policies.max_timeout_multiplier
    );
    println!(
        "   - max_absolute_timeout_ms: {}",
        policies.max_absolute_timeout_ms
    );
    println!();

    // =========================================================================
    // TEST 1: Valid retry hint (within policy limits)
    // =========================================================================
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!(
        "🧪 TEST 1: Valid Retry Hint (max-retries: 3, within limit of {})",
        policies.max_retries
    );
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

    let hints = create_retry_hint(3, 100); // 3 retries, 100ms backoff
    match validate_hints_against_policies(&hints, &policies) {
        Ok(_) => println!("   ✅ Hint validation PASSED (as expected)"),
        Err(e) => println!("   ❌ Unexpected error: {}", e),
    }

    // =========================================================================
    // TEST 2: Policy-violating retry hint (exceeds max_retries)
    // =========================================================================
    println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!(
        "🧪 TEST 2: Invalid Retry Hint (max-retries: 10, exceeds limit of {})",
        policies.max_retries
    );
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

    let hints = create_retry_hint(10, 100); // 10 retries - should fail!
    match validate_hints_against_policies(&hints, &policies) {
        Ok(_) => println!("   ❌ Validation should have FAILED but passed"),
        Err(e) => println!("   ✅ Correctly rejected: {}", e),
    }

    // =========================================================================
    // TEST 3: Policy-violating timeout hint (exceeds multiplier)
    // =========================================================================
    println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!(
        "🧪 TEST 3: Invalid Timeout Hint (multiplier: 20.0, exceeds limit of {})",
        policies.max_timeout_multiplier
    );
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

    let hints = create_timeout_hint(20.0, 60_000); // 20x multiplier - should fail!
    match validate_hints_against_policies(&hints, &policies) {
        Ok(_) => println!("   ❌ Validation should have FAILED but passed"),
        Err(e) => println!("   ✅ Correctly rejected: {}", e),
    }

    // =========================================================================
    // TEST 4: Execute with hints on flaky capability
    // =========================================================================
    println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("🧪 TEST 4: Execute Flaky Capability with CallMetadata");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

    // Reset the flaky counter
    FLAKY_CALL_COUNT.store(0, Ordering::SeqCst);

    let hints = create_retry_hint(3, 50); // 3 retries, 50ms backoff
    let metadata = create_call_metadata(hints);

    println!("   Calling test.flaky with execution hints...");
    println!("   (Marketplace passes hints to Orchestrator which applies retry)");

    let result = marketplace
        .execute_capability_enhanced("test.flaky", &Value::Nil, Some(&metadata))
        .await;

    match result {
        Ok(v) => println!("   ✅ Success: {:?}", v),
        Err(e) => println!("   ❌ Failed: {}", e),
    }

    println!(
        "   Total calls made to flaky capability: {}",
        FLAKY_CALL_COUNT.load(Ordering::SeqCst)
    );

    // =========================================================================
    // TEST 5: Governance checkpoint logging
    // =========================================================================
    println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("🧪 TEST 5: Governance Checkpoint Logging to CausalChain");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

    // Log governance actions (simulating what Orchestrator does)
    {
        let mut chain_guard = chain.lock().unwrap();
        chain_guard.append(
            &Action::new(
                ActionType::GovernanceCheckpointDecision,
                "demo-plan".to_string(),
                "demo-intent".to_string(),
            )
            .with_metadata("security_level", "medium")
            .with_metadata("decision", "approved"),
        )?;

        chain_guard.append(
            &Action::new(
                ActionType::HintApplied,
                "demo-plan".to_string(),
                "demo-intent".to_string(),
            )
            .with_metadata("hint", "retry:1/3"),
        )?;

        chain_guard.append(
            &Action::new(
                ActionType::GovernanceCheckpointOutcome,
                "demo-plan".to_string(),
                "demo-intent".to_string(),
            )
            .with_metadata("outcome", "success"),
        )?;
    }

    // Read back actions
    let chain_guard = chain.lock().unwrap();
    let all_actions = chain_guard.get_all_actions();
    let governance_actions: Vec<_> = all_actions
        .iter()
        .filter(|a| {
            matches!(
                a.action_type,
                ActionType::GovernanceCheckpointDecision
                    | ActionType::GovernanceCheckpointOutcome
                    | ActionType::HintApplied
            )
        })
        .collect();

    println!(
        "   Found {} governance-related actions in CausalChain:",
        governance_actions.len()
    );
    for action in &governance_actions {
        println!("   - {:?}", action.action_type);
        for (k, v) in &action.metadata {
            println!("     └── {}: {:?}", k, v);
        }
    }

    // =========================================================================
    // Summary
    // =========================================================================
    println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("📊 SUMMARY: Two-Tier Governance Architecture");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("✅ Tier 1 (Global): Hints validated against ExecutionHintPolicies");
    println!("✅ Tier 2 (Atomic): Governance checkpoints logged to CausalChain");
    println!(
        "✅ ActionTypes: GovernanceCheckpointDecision, HintApplied, GovernanceCheckpointOutcome"
    );
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

    Ok(())
}

// =============================================================================
// Helper Functions
// =============================================================================

/// Validate hints against policies (mirrors GovernanceKernel.validate_execution_hints)
fn validate_hints_against_policies(
    hints: &HashMap<String, Value>,
    policies: &ExecutionHintPolicies,
) -> RuntimeResult<()> {
    // Validate retry hints
    if let Some(retry_value) = hints.get("runtime.learning.retry") {
        if let Some(max_retries) = extract_u32_from_map(retry_value, "max-retries")
            .or_else(|| extract_u32_from_map(retry_value, "max"))
        {
            if max_retries > policies.max_retries {
                return Err(RuntimeError::Generic(format!(
                    "Execution hint violated: retry max-retries={} exceeds policy limit of {}",
                    max_retries, policies.max_retries
                )));
            }
        }
    }

    // Validate timeout hints
    if let Some(timeout_value) = hints.get("runtime.learning.timeout") {
        if let Some(multiplier) = extract_f64_from_map(timeout_value, "multiplier") {
            if multiplier > policies.max_timeout_multiplier {
                return Err(RuntimeError::Generic(format!(
                    "Execution hint violated: timeout multiplier={} exceeds policy limit of {}",
                    multiplier, policies.max_timeout_multiplier
                )));
            }
        }

        if let Some(absolute_ms) = extract_u64_from_map(timeout_value, "absolute-ms") {
            if absolute_ms > policies.max_absolute_timeout_ms {
                return Err(RuntimeError::Generic(format!(
                    "Execution hint violated: timeout absolute-ms={} exceeds policy limit of {}",
                    absolute_ms, policies.max_absolute_timeout_ms
                )));
            }
        }
    }

    Ok(())
}

/// Helper to extract u32 from a RTFS map value
fn extract_u32_from_map(value: &Value, key: &str) -> Option<u32> {
    if let Value::Map(map) = value {
        for (k, v) in map {
            let key_str = match k {
                MapKey::Keyword(kw) => &kw.0,
                MapKey::String(s) => s,
                _ => continue,
            };
            if key_str == key {
                return match v {
                    Value::Integer(i) => Some(*i as u32),
                    Value::Float(f) => Some(*f as u32),
                    _ => None,
                };
            }
        }
    }
    None
}

/// Helper to extract u64 from a RTFS map value
fn extract_u64_from_map(value: &Value, key: &str) -> Option<u64> {
    if let Value::Map(map) = value {
        for (k, v) in map {
            let key_str = match k {
                MapKey::Keyword(kw) => &kw.0,
                MapKey::String(s) => s,
                _ => continue,
            };
            if key_str == key {
                return match v {
                    Value::Integer(i) => Some(*i as u64),
                    Value::Float(f) => Some(*f as u64),
                    _ => None,
                };
            }
        }
    }
    None
}

/// Helper to extract f64 from a RTFS map value
fn extract_f64_from_map(value: &Value, key: &str) -> Option<f64> {
    if let Value::Map(map) = value {
        for (k, v) in map {
            let key_str = match k {
                MapKey::Keyword(kw) => &kw.0,
                MapKey::String(s) => s,
                _ => continue,
            };
            if key_str == key {
                return match v {
                    Value::Float(f) => Some(*f),
                    Value::Integer(i) => Some(*i as f64),
                    _ => None,
                };
            }
        }
    }
    None
}

/// Create retry hint map
fn create_retry_hint(max_retries: i64, backoff_ms: i64) -> HashMap<String, Value> {
    let mut hints = HashMap::new();
    let mut retry_map = HashMap::new();
    retry_map.insert(
        MapKey::Keyword(Keyword("max-retries".to_string())),
        Value::Integer(max_retries),
    );
    retry_map.insert(
        MapKey::Keyword(Keyword("backoff-ms".to_string())),
        Value::Integer(backoff_ms),
    );
    hints.insert("runtime.learning.retry".to_string(), Value::Map(retry_map));
    hints
}

/// Create timeout hint map
fn create_timeout_hint(multiplier: f64, absolute_ms: i64) -> HashMap<String, Value> {
    let mut hints = HashMap::new();
    let mut timeout_map = HashMap::new();
    timeout_map.insert(
        MapKey::Keyword(Keyword("multiplier".to_string())),
        Value::Float(multiplier),
    );
    timeout_map.insert(
        MapKey::Keyword(Keyword("absolute-ms".to_string())),
        Value::Integer(absolute_ms),
    );
    hints.insert(
        "runtime.learning.timeout".to_string(),
        Value::Map(timeout_map),
    );
    hints
}

/// Create CallMetadata with hints
fn create_call_metadata(hints: HashMap<String, Value>) -> CallMetadata {
    CallMetadata {
        execution_hints: hints,
        ..Default::default()
    }
}

// Global counter for flaky capability
static FLAKY_CALL_COUNT: AtomicU32 = AtomicU32::new(0);

/// Register test capabilities
async fn register_test_capabilities(marketplace: &CapabilityMarketplace) -> RuntimeResult<()> {
    // Flaky capability - fails first 2 times, succeeds on 3rd
    marketplace
        .register_local_capability(
            "test.flaky".to_string(),
            "Flaky Test".to_string(),
            "Fails first 2 times, succeeds on 3rd".to_string(),
            Arc::new(|_args: &Value| {
                let count = FLAKY_CALL_COUNT.fetch_add(1, Ordering::SeqCst);
                if count < 2 {
                    Err(RuntimeError::Generic(format!(
                        "Simulated failure #{}",
                        count + 1
                    )))
                } else {
                    Ok(Value::String("Success on attempt 3!".to_string()))
                }
            }),
        )
        .await?;

    // Echo capability - always succeeds
    marketplace
        .register_local_capability(
            "test.echo".to_string(),
            "Echo".to_string(),
            "Returns input".to_string(),
            Arc::new(|args: &Value| Ok(args.clone())),
        )
        .await?;

    Ok(())
}
