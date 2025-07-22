use rtfs_compiler::ccos::arbiter::{Arbiter, ArbiterConfig};
use rtfs_compiler::ccos::intent_graph::IntentGraph;
use std::sync::{Arc, Mutex};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create an arbiter with default configuration
    let intent_graph = Arc::new(Mutex::new(IntentGraph::new().unwrap()));
    let arbiter = Arbiter::new(ArbiterConfig::default(), intent_graph);

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
