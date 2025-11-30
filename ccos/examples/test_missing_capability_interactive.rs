use ccos::examples_common::builder::CcosEnvBuilder;
use ccos::planner::modular_planner::types::{DomainHint, IntentType, SubIntent};
use ccos::synthesis::continuous_resolution::{ContinuousResolutionLoop, ResolutionConfig};
use ccos::synthesis::missing_capability_resolver::ResolutionResult;
use ccos::synthesis::registration_flow::RegistrationFlow;
use rtfs::runtime::security::RuntimeContext;
use std::collections::HashMap;
use std::sync::Arc;

/// Test Missing Capability Resolution Flow - User Interaction/LLM
///
/// This example demonstrates the enhanced missing capability resolution flow
/// specifically for cases where the capability cannot be found or generated automatically,
/// forcing the system to fall back to user interaction or LLM synthesis.
///
/// Usage:
///   cargo run --example test_missing_capability_interactive
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    println!("🔍 Test Missing Capability Resolution Flow - Interactive/LLM Fallback");
    println!("===============================================================");

    // 1. Enable Feature Flags via Environment Variables
    std::env::set_var("CCOS_MISSING_CAPABILITY_ENABLED", "true");
    std::env::set_var("CCOS_AUTO_RESOLUTION_ENABLED", "true");
    std::env::set_var("CCOS_RUNTIME_DETECTION_ENABLED", "true");
    // We want to test the fallback flow, but still simulate approval for the demo
    std::env::set_var("CCOS_HUMAN_APPROVAL_REQUIRED", "false"); 
    std::env::set_var("CCOS_MCP_REGISTRY_ENABLED", "true");
    std::env::set_var("CCOS_OUTPUT_SCHEMA_INTROSPECTION_ENABLED", "true");
    std::env::set_var("CCOS_QUIET_RESOLVER", "true"); // Reduce noise

    // Ensure MCP Auth is available
    if std::env::var("MCP_AUTH_TOKEN").is_err() {
        eprintln!("⚠️ MCP_AUTH_TOKEN not set. Discovery might fail.");
    }

    // 2. Initialize CCOS via CcosEnvBuilder
    println!("Initializing CCOS via CcosEnvBuilder...");
    let env = CcosEnvBuilder::new()
        .with_config("config/agent_config.toml")
        .build()
        .await?;

    let ccos = env.ccos.clone();
    let marketplace = ccos.get_capability_marketplace();

    // 3. Test Case: Missing Capability that needs complex implementation (User/LLM)
    println!("\n🧪 Test Case: Complex Missing Capability");
    // Using a capability ID that implies complex logic not easily synthesizable via simple pure RTFS
    let cap_id = "custom.advanced_analysis.market_sentiment_predictor";
    println!("Testing capability '{}' that definitely doesn't exist", cap_id);

    // Create a test intent
    let test_intent = SubIntent::new(
        "Predict market sentiment based on complex aggregated data",
        IntentType::DataTransform {
            transform: ccos::planner::modular_planner::types::TransformType::Other("map".to_string()),
        },
    )
    .with_domain(DomainHint::Custom("financial_analysis".to_string()))
    .with_param("input_data", "step_0_result");

    println!("Intent: {}", test_intent.description);

    // 4. Start Continuous Resolution Loop
    println!("\n🔁 Starting Resolution Loop...");
    let resolution_loop = if let Some(resolver) = &ccos.missing_capability_resolver {
        let registration_flow = Arc::new(RegistrationFlow::new(Arc::clone(&marketplace)));
        // We configure the loop to allow user/LLM strategies
        let config = ResolutionConfig {
            auto_resolution_enabled: true,
            max_retry_attempts: 10, // Increased to ensure Manual/Interactive strategy is reached
            ..Default::default()
        };
        Arc::new(ContinuousResolutionLoop::new(
            Arc::clone(resolver),
            registration_flow,
            Arc::clone(&marketplace),
            config,
        ))
    } else {
        eprintln!("❌ Resolver not available!");
        return Ok(());
    };

    // 5. Create a Plan that uses the missing capability
    let plan_rtfs_template = r#"
        (do
            (println "Testing interactive missing capability resolution...")
            ;; This complex capability doesn't exist
            (let [market_data {:symbol "AAPL" :price 150.0 :volume 1000000}]
                     (let [sentiment (call "custom.advanced_analysis.market_sentiment_predictor"
                                                                                      {:data market_data})]
                    (println "Predicted Sentiment:" sentiment)
                    sentiment)))
        "#;
    let plan_rtfs = plan_rtfs_template.replace("custom.advanced_analysis.market_sentiment_predictor", cap_id);

    let plan = ccos::types::Plan {
        plan_id: "interactive-missing-cap-test".to_string(),
        name: Some("Interactive Missing Capability Test".to_string()),
        body: ccos::types::PlanBody::Rtfs(plan_rtfs.to_string()),
        intent_ids: vec![],
        ..Default::default()
    };

    println!("\n🚀 Executing Plan (Attempt 1 - Should Fail)...");
    let context = RuntimeContext::full();
    let first_success = match ccos.validate_and_execute_plan(plan.clone(), &context).await {
        Ok(result) => result.success,
        Err(err) => {
            println!("Expected failure occurred: {:?}", err);
            false
        }
    };

    if !first_success {
        println!("✅ Plan failed as expected (Capability not found).");
        println!("   Trap should have queued it for resolution.");
        
        // Manually trigger queueing if the automatic trap didn't catch it in this specific test setup
        // (In a full real run, the trap catches it, but here we explicitly ensure it's queued for the test logic)
        if let Some(resolver) = &ccos.missing_capability_resolver {
            let mut detection_context = HashMap::new();
            detection_context.insert(
                "description".to_string(),
                "Predict market sentiment based on complex aggregated data".to_string(),
            );
            // Hint that we want LLM resolution if pure RTFS fails
            detection_context.insert("resolution_strategy".to_string(), "llm_synthesis".to_string());
            
            if let Err(err) =
                resolver.handle_missing_capability(cap_id.to_string(), vec![], detection_context)
            {
                 // Ignore if already queued
                 if !err.to_string().contains("already pending") {
                    eprintln!("⚠️  Failed to queue missing capability: {}", err);
                 }
            }
        }
    }

    // 6. Drive resolution loop
    println!("\n🔁 Driving resolution loop (Simulating async background process)...");
    // We run for a few iterations. 
    // In a real interactive mode, this would block waiting for user input or LLM response.
    // For this test, we observe the state transitions.
    
    for i in 0..5 {
        println!("   Resolution cycle {}...", i + 1);
        if let Err(e) = resolution_loop.process_pending_resolutions().await {
            eprintln!("Resolution loop error: {}", e);
        }
        
        if let Some(resolver) = &ccos.missing_capability_resolver {
            let pending = resolver.list_pending_capabilities();
            if !pending.is_empty() {
                 println!("   Still pending: {:?}", pending);
            } else {
                 println!("   Queue empty (Resolution completed or failed permanently)");
            }
        }
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }

    // 7. Check final status
    if let Some(cap) = marketplace.get_capability(cap_id).await {
        println!("✅ Capability '{}' was resolved and created!", cap_id);
        println!("   Description: {}", cap.description);
        
        // 8. Execute plan again (should succeed)
        println!("\n🚀 Executing Plan (Attempt 2 - Should Succeed)...");
        let result2 = ccos.validate_and_execute_plan(plan, &context).await?;
        if result2.success {
            println!("🎉 Execution successful!");
        } else {
            println!("❌ Execution failed even after resolution.");
        }
    } else {
        println!("ℹ️ Capability '{}' was NOT created (Expected if no mock LLM is connected/configured to reply).", cap_id);
        println!("   In a real scenario with LLM backend, this would have produced a capability.");
    }

    Ok(())
}

