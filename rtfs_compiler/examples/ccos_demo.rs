<<<<<<< HEAD
use rtfs_compiler::ccos::arbiter::Arbiter;
// Use the legacy arbiter config type to match `legacy_arbiter::Arbiter::new` signature
use rtfs_compiler::ccos::arbiter::legacy_arbiter::ArbiterConfig as LegacyArbiterConfig;
=======
use rtfs_compiler::ccos::arbiter::legacy_arbiter::{Arbiter as LegacyArbiter, ArbiterConfig as LegacyArbiterConfig};
>>>>>>> d3d4c9a (Fix: treat unknown string escapes as parse errors; normalize :keys destructuring for AST and IR; update integration test expectation)
use rtfs_compiler::ccos::intent_graph::IntentGraph;
use std::sync::{Arc, Mutex};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create an arbiter with default configuration
    let intent_graph = Arc::new(Mutex::new(IntentGraph::new().unwrap()));
<<<<<<< HEAD
    let arbiter = Arbiter::new(LegacyArbiterConfig::default(), intent_graph);
=======
    let arbiter = LegacyArbiter::new(LegacyArbiterConfig::default(), intent_graph);
>>>>>>> d3d4c9a (Fix: treat unknown string escapes as parse errors; normalize :keys destructuring for AST and IR; update integration test expectation)

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
