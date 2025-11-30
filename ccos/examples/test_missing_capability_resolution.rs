use ccos::examples_common::builder::CcosEnvBuilder;
use ccos::planner::modular_planner::types::{DomainHint, IntentType, SubIntent};
use ccos::synthesis::continuous_resolution::{ContinuousResolutionLoop, ResolutionConfig};
use ccos::synthesis::missing_capability_resolver::{MissingCapabilityResolver, ResolutionResult};
use ccos::synthesis::registration_flow::RegistrationFlow;
use rtfs::runtime::security::RuntimeContext;
/// Test Missing Capability Resolution Flow
///
/// This example demonstrates the enhanced missing capability resolution flow
/// that handles cases where no capability is found through discovery.
///
/// The flow includes:
/// 1. Pure RTFS generation
/// 2. User interaction for clarification
/// 3. External LLM hints for implementation
/// 4. Service discovery hints
///
/// Usage:
///   cargo run --example test_missing_capability_resolution
use std::collections::HashMap;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    println!("🔍 Test Missing Capability Resolution Flow");
    println!("==========================================");

    // 1. Enable Feature Flags via Environment Variables
    std::env::set_var("CCOS_MISSING_CAPABILITY_ENABLED", "true");
    std::env::set_var("CCOS_AUTO_RESOLUTION_ENABLED", "true");
    std::env::set_var("CCOS_RUNTIME_DETECTION_ENABLED", "true");
    std::env::set_var("CCOS_HUMAN_APPROVAL_REQUIRED", "false"); // Auto-approve for demo
    std::env::set_var("CCOS_MCP_REGISTRY_ENABLED", "true");
    std::env::set_var("CCOS_OUTPUT_SCHEMA_INTROSPECTION_ENABLED", "true");

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

    // `CcosEnvBuilder::build()` returns a `CcosEnv` containing an `Arc<CCOS>`
    let ccos = env.ccos.clone();
    let marketplace = ccos.get_capability_marketplace();

    // 3. Test Case: Missing Capability that needs pure RTFS generation
    println!("\n🧪 Test Case: Pure RTFS Generation");
    let cap_id = "custom.data_processing.filter_items";
    println!("Testing capability '{}' that doesn't exist", cap_id);

    // Create a test intent that would require this capability
    let test_intent = SubIntent::new(
        "Filter a list of items based on a predicate",
        IntentType::DataTransform {
            transform: ccos::planner::modular_planner::types::TransformType::Filter,
        },
    )
    .with_domain(DomainHint::Custom("data_processing".to_string()))
    .with_param("input_list", "step_0_result")
    .with_param("filter_predicate", "fn [item] (get item :active true)");

    println!("Intent: {}", test_intent.description);
    println!("Domain: {:?}", test_intent.domain_hint);
    println!("Params: {:?}", test_intent.extracted_params);

    // 4. Test the missing capability resolution flow
    if let Some(resolver) = &ccos.missing_capability_resolver {
        println!("\n🔧 Testing Missing Capability Resolution...");

        // Test the different strategies
        test_resolution_strategies(resolver).await?;
    }

    // 5. Start Continuous Resolution Loop
    println!("\n🔁 Starting Resolution Loop...");
    let resolution_loop = if let Some(resolver) = &ccos.missing_capability_resolver {
        let registration_flow = Arc::new(RegistrationFlow::new(Arc::clone(&marketplace)));
        let config = ResolutionConfig {
            auto_resolution_enabled: true,
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

    // 6. Create a Plan that uses a missing capability
    let plan_rtfs_template = r#"
        (do
            (println "Testing missing capability resolution...")
            ;; This capability doesn't exist yet
            (let [raw_data [{:id 1 :active true} {:id 2 :active false} {:id 3 :active true}]]
                     (let [filtered_data (call "custom.data_processing.filter_items"
                                                                                      {:data raw_data
                                                                                          :predicate (fn [item] (get item :active true))})]
                    (println "Filtered data:" filtered_data)
                    filtered_data)))
        "#;
    let plan_rtfs = plan_rtfs_template.replace("custom.data_processing.filter_items", cap_id);

    let plan = ccos::types::Plan {
        plan_id: "missing-cap-test".to_string(),
        name: Some("Missing Capability Test".to_string()),
        body: ccos::types::PlanBody::Rtfs(plan_rtfs.to_string()),
        intent_ids: vec![],
        ..Default::default()
    };

    println!("\n🚀 Executing Plan (Attempt 1 - Should Fail)...");
    let context = RuntimeContext::full();
    let first_success = match ccos.validate_and_execute_plan(plan.clone(), &context).await {
        Ok(result) => {
            if !result.success {
                println!("Execution returned without success flag set.");
            }
            result.success
        }
        Err(err) => {
            println!("Expected failure occurred: {:?}", err);
            false
        }
    };

    if !first_success {
        println!("✅ Plan failed as expected (Capability not found).");
        println!("   Trap should have queued it for resolution.");

        if let Some(resolver) = &ccos.missing_capability_resolver {
            let mut detection_context = HashMap::new();
            detection_context.insert(
                "description".to_string(),
                "Filter a list of items based on a predicate".to_string(),
            );
            if let Err(err) =
                resolver.handle_missing_capability(cap_id.to_string(), vec![], detection_context)
            {
                eprintln!("⚠️  Failed to queue missing capability: {}", err);
            }
            let pending = resolver.list_pending_capabilities();
            println!("   Pending queue size after failure: {}", pending.len());
        }
    }

    // 7. Drive resolution loop
    println!("\n🔁 Driving resolution loop for ~15 seconds...");
    for i in 0..5 {
        println!("   Resolution attempt {}...", i + 1);
        if let Err(e) = resolution_loop.process_pending_resolutions().await {
            eprintln!("Resolution loop error: {}", e);
        }
        if let Some(resolver) = &ccos.missing_capability_resolver {
            let pending = resolver.list_pending_capabilities();
            println!(
                "   Pending queue size after attempt {}: {}",
                i + 1,
                pending.len()
            );
        }
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    }

    // 8. Check if capability was created
    let cap_id = "custom.data_processing.filter_items";
    if let Some(cap) = marketplace.get_capability(cap_id).await {
        println!("✅ Capability '{}' was successfully created!", cap_id);
        println!("   Description: {}", cap.description);
    } else {
        println!("❌ Capability '{}' was not created.", cap_id);
    }

    // 9. Execute plan again (should succeed if resolution worked)
    println!("\n🚀 Executing Plan (Attempt 2 - Should Succeed)...");
    let result2 = ccos.validate_and_execute_plan(plan, &context).await?;

    println!("\n🏁 Execution Finished");
    println!("   Success: {}", result2.success);

    if result2.success {
        println!("🎉 The missing capability was successfully resolved and executed!");
    } else {
        println!("❌ Execution failed again.");
        if let Some(err) = result2.metadata.get("error") {
            println!("   Error: {:?}", err);
        }
    }

    Ok(())
}

/// Test the different resolution strategies
async fn test_resolution_strategies(
    resolver: &Arc<MissingCapabilityResolver>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    println!("\n🧪 Testing Resolution Strategies");

    // Test 1: Pure RTFS Generation
    println!("\n1️⃣  Pure RTFS Generation Strategy");
    println!("   This strategy generates capabilities using only RTFS standard library functions");
    println!("   Example: Generate a filter capability that doesn't depend on external services");

    // Test 2: User Interaction
    println!("\n2️⃣  User Interaction Strategy");
    println!("   This strategy asks the user for clarification when resolution fails");
    println!("   Example prompts:");
    println!("   - 'I couldn't find capability X. Did you mean Y or Z?'");
    println!("   - 'How would you like me to implement capability X?'");
    println!("   - 'What service/API should I use for this capability?'");

    // Test 3: External LLM Hint Strategy
    println!("\n3️⃣  External LLM Hint Strategy");
    println!("   This strategy queries external LLMs for implementation suggestions");
    println!("   Example LLM prompt:");
    println!(
        r#"   "How would you implement a capability called 'data.filter_items' in RTFS?
   Provide:
   1. Recommended implementation approach
   2. RTFS implementation code
   3. Input/output schemas
   4. Required permissions""#
    );

    // Test 4: Service Discovery Hint Strategy
    println!("\n4️⃣  Service Discovery Hint Strategy");
    println!("   This strategy asks the user for hints about where to find capabilities");
    println!("   Example prompts:");
    println!("   - 'I couldn't discover capability X. Which MCP servers should I check?'");
    println!("   - 'What APIs or services might provide this capability?'");
    println!("   - 'Should I look for generic filter capabilities instead?'");

    // Test the actual resolution pipeline
    println!("\n🔧 Testing Resolution Pipeline...");

    // Simulate a missing capability request
    let request = ccos::synthesis::missing_capability_resolver::MissingCapabilityRequest {
        capability_id: "custom.data_processing.filter_items".to_string(),
        arguments: vec![],
        context: {
            let mut ctx = std::collections::HashMap::new();
            ctx.insert(
                "description".to_string(),
                "Filter a list of items based on a predicate".to_string(),
            );
            ctx.insert("resolution_strategy".to_string(), "auto".to_string());
            ctx
        },
        requested_at: std::time::SystemTime::now(),
        attempt_count: 0,
    };

    println!("   Requesting resolution for: {}", request.capability_id);

    // Test the resolution
    let result = resolver.resolve_capability(&request).await?;

    match result {
        ResolutionResult::Resolved {
            capability_id,
            resolution_method,
            ..
        } => {
            println!(
                "   ✅ Successfully resolved: {} via {}",
                capability_id, resolution_method
            );
        }
        ResolutionResult::Failed {
            capability_id,
            reason,
            ..
        } => {
            println!("   ❌ Failed to resolve: {} - {}", capability_id, reason);
            println!("   This is expected if the capability doesn't exist and no fallback strategies are implemented yet");
        }
        ResolutionResult::PermanentlyFailed {
            capability_id,
            reason,
        } => {
            println!("   ❌ Permanently failed: {} - {}", capability_id, reason);
        }
    }

    Ok(())
}
