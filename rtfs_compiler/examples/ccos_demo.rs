use rtfs_compiler::ccos::arbiter::Arbiter;
// Use the legacy arbiter config type to match `legacy_arbiter::Arbiter::new` signature
use rtfs_compiler::ccos::arbiter::legacy_arbiter::ArbiterConfig as LegacyArbiterConfig;
use rtfs_compiler::ccos::intent_graph::IntentGraph;
use std::sync::{Arc, Mutex};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // CI-safe guard: skip creating reactive runtime in CI
    if std::env::var("CI").is_ok() {
        println!("ccos_demo: running in CI, skipping runtime demo (no external effects)");
        return Ok(());
    }

    // Create an arbiter with default configuration (no unwrap/expect)
    let ig = IntentGraph::new()?;
    let intent_graph = Arc::new(Mutex::new(ig));
    let _arbiter = Arbiter::new(LegacyArbiterConfig::default(), intent_graph);

    println!("=== CCOS + RTFS Cognitive Computing Demo ===\n");

    // Demo 1: Basic Arbiter Creation
    println!("✅ Arbiter created successfully with default configuration");
    println!("   - Intent graph initialized");
    println!("   - Default configuration applied");

    // Demo 2: Show configuration
    println!("\n📋 Arbiter Configuration:");
    println!("   - Default security context");
    println!("   - Intent graph ready for use");

    println!("\n🎯 Demo completed successfully!");
    println!("===================================");

    Ok(())
}
