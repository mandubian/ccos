//! RTFS Governance Hints Demo
//!
//! This demo executes a pure RTFS plan that uses execution hints to test
//! the complete two-tier governance flow:
//! 1. Hints set via ^{:runtime.learning.*} metadata
//! 2. Hints validated by GovernanceKernel (Tier 1)
//! 3. Hints applied by Orchestrator (Tier 2) - retry, timeout, fallback
//!
//! Run with: cargo run --example governance_rtfs_demo

use ccos::types::{Plan, PlanBody};
use ccos::CCOS;
use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use rtfs::runtime::security::RuntimeContext;
use rtfs::runtime::values::Value;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

// Global counter for flaky capability
static FLAKY_CALL_COUNT: AtomicU32 = AtomicU32::new(0);

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║       RTFS Governance Hints Demo                             ║");
    println!("╚══════════════════════════════════════════════════════════════╝\n");

    // 1. Initialize CCOS
    let ccos = CCOS::new()
        .await
        .map_err(|e| format!("Failed to create CCOS: {:?}", e))?;
    let ccos = std::sync::Arc::new(ccos);

    // 2. Register test capabilities
    println!("📦 Registering test capabilities...\n");
    register_test_capabilities(&ccos).await?;

    // 3. Load and execute the RTFS plan
    let rtfs_code = include_str!("governance_hints_demo.rtfs");

    println!("📜 RTFS Plan:\n{}", rtfs_code);
    println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

    // 4. Create a Plan object
    let plan = Plan {
        plan_id: format!("governance-hints-demo-{}", uuid::Uuid::new_v4()),
        name: Some("Governance Hints Demo".to_string()),
        body: PlanBody::Rtfs(rtfs_code.to_string()),
        ..Default::default()
    };

    // 5. Execute via GovernanceKernel -> Orchestrator path
    println!("⚡ Executing plan through governance pipeline...\n");

    let context = RuntimeContext::full();
    match ccos.validate_and_execute_plan(plan, &context).await {
        Ok(result) => {
            println!("\n🏁 Execution Result:");
            println!("   Success: {}", result.success);
            println!("   Value: {:?}", result.value);

            if !result.metadata.is_empty() {
                println!("   Metadata:");
                for (k, v) in &result.metadata {
                    println!("     {}: {:?}", k, v);
                }
            }
        }
        Err(e) => {
            println!("\n❌ Execution failed: {}", e);
        }
    }

    // 6. Note about governance audit trail
    println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("📊 Governance Audit Trail");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");
    println!("   Note: Governance checkpoints are logged internally to the CausalChain.");
    println!("   In production, these would be persisted and queryable.");

    println!(
        "\n   Flaky capability was called {} times",
        FLAKY_CALL_COUNT.load(Ordering::SeqCst)
    );

    println!("\n✅ Demo complete!\n");
    Ok(())
}

/// Register test capabilities needed by the RTFS plan
async fn register_test_capabilities(ccos: &Arc<CCOS>) -> RuntimeResult<()> {
    let marketplace = ccos.get_capability_marketplace();

    // test.echo - always succeeds
    marketplace
        .register_local_capability(
            "test.echo".to_string(),
            "Echo".to_string(),
            "Returns the input message".to_string(),
            Arc::new(|args: &Value| {
                println!("   [test.echo] Got: {:?}", args);
                Ok(args.clone())
            }),
        )
        .await?;

    // test.flaky - fails first 2 times, succeeds on 3rd
    marketplace
        .register_local_capability(
            "test.flaky".to_string(),
            "Flaky Test".to_string(),
            "Simulates a flaky service that fails intermittently".to_string(),
            Arc::new(|_args: &Value| {
                let count = FLAKY_CALL_COUNT.fetch_add(1, Ordering::SeqCst);
                println!(
                    "   [test.flaky] Attempt {} (fails first 2 times)",
                    count + 1
                );
                if count < 2 {
                    Err(RuntimeError::Generic(format!(
                        "Simulated failure #{}",
                        count + 1
                    )))
                } else {
                    Ok(Value::String("Success after retries!".to_string()))
                }
            }),
        )
        .await?;

    // test.might-fail - always fails (for fallback testing)
    marketplace
        .register_local_capability(
            "test.might-fail".to_string(),
            "Might Fail".to_string(),
            "Always fails to trigger fallback".to_string(),
            Arc::new(|_args: &Value| {
                println!("   [test.might-fail] Intentionally failing");
                Err(RuntimeError::Generic(
                    "Primary capability failed".to_string(),
                ))
            }),
        )
        .await?;

    // test.fallback - backup capability
    marketplace
        .register_local_capability(
            "test.fallback".to_string(),
            "Fallback".to_string(),
            "Backup capability that always succeeds".to_string(),
            Arc::new(|_args: &Value| {
                println!("   [test.fallback] Fallback succeeded!");
                Ok(Value::String("Fallback handled it!".to_string()))
            }),
        )
        .await?;

    Ok(())
}
