//! RTFS Execution Demo
//!
//! Verifies the `ccos.execute.rtfs` capability by running various RTFS snippets.

use ccos::capabilities::registry::CapabilityRegistry;
use ccos::capability_marketplace::CapabilityMarketplace;
use ccos::causal_chain::CausalChain;
use ccos::chat::{register_chat_capabilities, InMemoryQuarantineStore, QuarantineStore};
use ccos::config::types::{CodingAgentsConfig, SandboxConfig};
use rtfs::ast::MapKey;
use rtfs::runtime::values::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🚀 Starting RTFS Execution Demo");
    println!("===============================");

    // 1. Setup Marketplace & Registry
    let registry = Arc::new(tokio::sync::RwLock::new(CapabilityRegistry::new()));
    let marketplace = Arc::new(CapabilityMarketplace::new(registry.clone()));

    // 2. Mock dependencies for register_chat_capabilities
    let quarantine: Arc<dyn QuarantineStore> = Arc::new(InMemoryQuarantineStore::new());
    let causal_chain = Arc::new(Mutex::new(CausalChain::new()?));
    let resource_store = ccos::chat::new_shared_resource_store();

    register_chat_capabilities(
        marketplace.clone(),
        quarantine,
        causal_chain,
        None,
        resource_store,
        None,
        None,
        None,
        None,
        SandboxConfig::default(),
        CodingAgentsConfig::default(),
    )
    .await?;

    println!("✅ Capabilities registered (including ccos.execute.rtfs)");

    // 3. Test Cases
    let test_cases = vec![
        ("Basic Arithmetic", "(+ 10 (* 5 2))"),
        ("Map Manipulation", "(assoc {:a 1} :b 2)"),
        ("Standard Library (Math)", "(sqrt 16)"),
        ("Standard Library (String)", "(string-upper \"hello rtfs\")"),
        ("JSON Parsing", "(tool/parse-json \"{\\\"x\\\": 123}\")"),
    ];

    for (name, code) in test_cases {
        println!("\n[Test] {}", name);
        println!("  Code: {}", code);

        let mut inputs = HashMap::new();
        inputs.insert(
            MapKey::String("code".to_string()),
            Value::String(code.to_string()),
        );

        match marketplace
            .execute_capability("ccos.execute.rtfs", &Value::Map(inputs))
            .await
        {
            Ok(result) => println!("  Result: {}", result),
            Err(e) => println!("  ❌ Error: {:?}", e),
        }
    }

    println!("\n✅ RTFS Execution Demo Complete!");
    Ok(())
}
