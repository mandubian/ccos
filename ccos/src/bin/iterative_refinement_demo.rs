//! Iterative Refinement Demo
//!
//! Showcases the full cycle:
//! LLM generation -> Sandboxed execution -> Automatic error classification -> Refinement loop.

use ccos::capabilities::registry::CapabilityRegistry;
use ccos::capability_marketplace::CapabilityMarketplace;
use ccos::causal_chain::CausalChain;
use ccos::chat::{register_chat_capabilities, InMemoryQuarantineStore, QuarantineStore};
use ccos::config::types::{CodingAgentsConfig, SandboxConfig};
use rtfs::ast::{Keyword, MapKey};
use rtfs::runtime::values::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🚀 Starting Iterative Refinement Demo");
    println!("=====================================");

    // 1. Setup Marketplace & Registry
    let registry = Arc::new(tokio::sync::RwLock::new(CapabilityRegistry::new()));
    let marketplace = Arc::new(CapabilityMarketplace::new(registry.clone()));

    // 2. Mock dependencies for register_chat_capabilities
    let quarantine: Arc<dyn QuarantineStore> = Arc::new(InMemoryQuarantineStore::new());
    let causal_chain = Arc::new(Mutex::new(CausalChain::new()?));
    let resource_store = ccos::chat::new_shared_resource_store();

    let sandbox_cfg = SandboxConfig::default();
    let mut coding_cfg = CodingAgentsConfig::default();
    // Use a model that is likely available and free on OpenRouter for the demo
    if let Some(profile) = coding_cfg.profiles.get_mut(0) {
        profile.model = "google/gemini-2.0-flash-001".to_string();
    }

    // 3. Register Chat capabilities (includes ccos.execute.python and ccos.code.refined_execute)
    register_chat_capabilities(
        marketplace.clone(),
        quarantine,
        causal_chain,
        None, // approval_queue
        resource_store,
        None, // connector
        None, // connector_handle
        None, // gateway_url
        None, // internal_secret
        sandbox_cfg,
        coding_cfg,
    )
    .await?;

    println!("✅ Capabilities registered (ccos.execute.python, ccos.code.refined_execute)");

    // 4. Trigger Refined Execution with a "Hard" Task
    // We'll ask for something that might require a dependency or have a subtle trap.
    // Task: "Calculate the 10th Fibonacci number using a recursive function, but intentionally miss-spell 'fibonacci' once in the code to trigger a NameError, then fix it."
    // Actually, we want the LLM to fail once and then fix it.

    println!("\n[Demo] Triggering Refinement Loop...");
    let mut inputs = HashMap::new();
    inputs.insert(MapKey::String("task".to_string()), Value::String(
        "Generate a python script that calculates the 10th Fibonacci number. \
         CRITICAL: Your FIRST attempt MUST fail with a 'NameError' by calling a function that doesn't exist \
         (e.g., call 'calc_fib' before defining it, or use 'fibonaci'). \
         I need to see the refinement process, so please DO NOT get it right on the first try.".to_string()
    ));
    inputs.insert(MapKey::String("max_turns".to_string()), Value::Float(3.0));

    let result = marketplace
        .execute_capability("ccos.code.refined_execute", &Value::Map(inputs))
        .await?;

    // 5. Inspect Results
    if let Value::Map(m) = result {
        if let Some(history) = m.get(&MapKey::Keyword(Keyword("refinement_history".to_string()))) {
            if let Value::Vector(turns) = history {
                println!("\n📊 Refinement History ({} turns):", turns.len());
                for (i, turn) in turns.iter().enumerate() {
                    if let Value::Map(turn_map) = turn {
                        let success = turn_map
                            .get(&MapKey::String("success".to_string()))
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false);
                        let error = turn_map
                            .get(&MapKey::String("error".to_string()))
                            .and_then(|v| v.as_string())
                            .unwrap_or("");

                        println!(
                            "  Turn #{}: Success={}, Error={}",
                            i + 1,
                            success,
                            if error.is_empty() {
                                "None"
                            } else {
                                error.lines().next().unwrap_or("")
                            }
                        );
                    }
                }
            }
        }

        if let Some(code) = m.get(&MapKey::String("code".to_string())) {
            println!(
                "\n🏆 Final Code:\n```python\n{}\n```",
                code.as_string().unwrap_or("")
            );
        }

        if let Some(output) = m.get(&MapKey::String("stdout".to_string())) {
            println!("\n🚀 Final Output: {}", output.as_string().unwrap_or(""));
        }
    }

    println!("\n✅ Iterative Refinement Demo Complete!");
    Ok(())
}
