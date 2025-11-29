//! Missing Capability Resolution Demo
//!
//! This example demonstrates the "Just-in-Time" discovery and synthesis of a missing capability.
//!
//! Scenario:
//! 1. The capability `mcp.github.list_issues` is missing (deleted from disk).
//! 2. We attempt to execute a plan that requires this capability.
//! 3. The execution fails, but the "Runtime Trap" catches the missing capability error.
//! 4. The trap queues the capability for resolution.
//! 5. The `ContinuousResolutionLoop` (running in background) picks it up and:
//!    a. Discovers it in the MCP Registry/Server.
//!    b. Introspects and synthesizes the RTFS wrapper.
//!    c. Persists and registers it.
//! 6. We retry the plan, and it succeeds.

use ccos::capabilities::{MCPSessionHandler, SessionPoolManager};
use ccos::synthesis::continuous_resolution::{ContinuousResolutionLoop, ResolutionConfig};
use ccos::synthesis::registration_flow::RegistrationFlow;
use ccos::CCOS;
use rtfs::runtime::security::RuntimeContext;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🔍 Missing Capability Resolution Demo");
    println!("=====================================");

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

    // 2. Initialize CCOS
    println!("Initializing CCOS...");
    let ccos = Arc::new(
        CCOS::new_with_agent_config_and_configs_and_debug_callback(
            ccos::intent_graph::config::IntentGraphConfig::default(),
            None,
            Some(rtfs::config::types::AgentConfig::default()),
            None,
        )
        .await?,
    );

    let marketplace = ccos.get_capability_marketplace();

    // Configure Session Pool for MCP
    let mut session_pool_mgr = SessionPoolManager::new();
    session_pool_mgr.register_handler("mcp", Arc::new(MCPSessionHandler::new()));
    let session_pool = Arc::new(session_pool_mgr);
    marketplace.set_session_pool(session_pool.clone()).await;

    // Verify list_issues is NOT present
    {
        let cap = marketplace.get_capability("mcp.github.list_issues").await;
        if cap.is_some() {
            eprintln!("❌ Error: mcp.github.list_issues is already present! Delete it first.");
            return Ok(());
        } else {
            println!("✅ Verified: mcp.github.list_issues is MISSING from registry.");
        }
    }

    // 3. Start Continuous Resolution Loop (since CCOS doesn't start it automatically yet)
    println!("Starting Resolution Loop...");
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

    // 4. Create a Plan that uses the missing capability (wrapped in do to execute)
    let plan_rtfs = r#"
    (do
      (println "Attempting to call missing capability...")
      (let [result (call "mcp.github.list_issues" 
                     {:owner "mandubian" 
                      :repo "ccos" 
                      :state "OPEN" 
                      :per_page 1})]
        (println "✅ SUCCESS! Result received:")
        (let [content (get result :content [])]
            (let [first_item (get content 0 {})]
                (println (get first_item :text "No text content"))
            )
        )
        result
      )
    )
    "#;

    let plan = ccos::types::Plan {
        plan_id: "missing-cap-demo".to_string(),
        name: Some("Missing Capability Demo".to_string()),
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
    } else {
        println!("❓ Plan succeeded unexpectedly? Maybe it was found immediately?");
    }

    println!("\n🔁 Driving resolution loop for ~15 seconds...");
    for _ in 0..5 {
        if let Err(e) = resolution_loop.process_pending_resolutions().await {
            eprintln!("Resolution loop error: {}", e);
        }
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    }

    // Check if file exists
    let path = std::path::Path::new("capabilities/discovered/mcp/github/list_issues.rtfs");
    if path.exists() {
        println!("✅ File recreated: {:?}", path);
    } else {
        println!("❌ File NOT found. Automated resolution failed.");
    }

    println!("\n🚀 Executing Plan (Attempt 2 - Should Succeed)...");
    let result2 = ccos.validate_and_execute_plan(plan, &context).await?;

    println!("\n🏁 Execution Finished");
    println!("   Success: {}", result2.success);

    if result2.success {
        println!(
            "🎉 The missing capability was successfully discovered, synthesized, and executed!"
        );
    } else {
        println!("❌ Execution failed again.");
        if let Some(err) = result2.metadata.get("error") {
            println!("   Error: {:?}", err);
        }
    }

    Ok(())
}
