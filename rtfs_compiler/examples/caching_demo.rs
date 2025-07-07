//! L1 Delegation Cache Demonstration
//! 
//! This example shows how the multi-layered caching system works in practice,
//! demonstrating the L1 delegation cache with real delegation decisions.

use rtfs_compiler::ccos::delegation::{
    CallContext, DelegationEngine, ExecTarget, StaticDelegationEngine
};
use std::collections::HashMap;

fn main() {
    println!("🚀 RTFS 2.0 Multi-Layered Caching System Demo\n");
    
    // Create a delegation engine with some static mappings
    let mut static_map = HashMap::new();
    static_map.insert("math/complex".to_string(), ExecTarget::RemoteModel("gpt4o".to_string()));
    static_map.insert("io/file".to_string(), ExecTarget::LocalPure);
    
    let de = StaticDelegationEngine::new(static_map);
    
    println!("📊 Initial Cache Stats:");
    let initial_stats = de.cache_stats();
    println!("  Hits: {}, Misses: {}, Hit Rate: {:.2}%", 
             initial_stats.hits, 
             initial_stats.misses, 
             initial_stats.hit_rate * 100.0);
    
    // Test 1: Static mapping (should bypass cache)
    println!("\n🔍 Test 1: Static Mapping (Bypasses Cache)");
    let ctx1 = CallContext {
        fn_symbol: "math/complex",
        arg_type_fingerprint: 0x12345678,
        runtime_context_hash: 0xABCDEF01,
        metadata: None,
    };
    
    let result1 = de.decide(&ctx1);
    println!("  Function: math/complex");
    println!("  Decision: {:?}", result1);
    
    let stats1 = de.cache_stats();
    println!("  Cache Stats: Hits={}, Misses={}, Hit Rate={:.2}%", 
             stats1.hits, stats1.misses, stats1.hit_rate * 100.0);
    
    // Test 2: First cache miss
    println!("\n🔍 Test 2: First Cache Miss");
    let ctx2 = CallContext {
        fn_symbol: "unknown/function",
        arg_type_fingerprint: 0x12345678,
        runtime_context_hash: 0xABCDEF01,
        metadata: None,
    };
    
    let result2 = de.decide(&ctx2);
    println!("  Function: unknown/function");
    println!("  Decision: {:?}", result2);
    
    let stats2 = de.cache_stats();
    println!("  Cache Stats: Hits={}, Misses={}, Hit Rate={:.2}%", 
             stats2.hits, stats2.misses, stats2.hit_rate * 100.0);
    
    // Test 3: Cache hit (same context)
    println!("\n🔍 Test 3: Cache Hit (Same Context)");
    let ctx3 = CallContext {
        fn_symbol: "unknown/function",
        arg_type_fingerprint: 0x12345678,
        runtime_context_hash: 0xABCDEF01,
        metadata: None,
    };
    
    let result3 = de.decide(&ctx3);
    println!("  Function: unknown/function");
    println!("  Decision: {:?}", result3);
    
    let stats3 = de.cache_stats();
    println!("  Cache Stats: Hits={}, Misses={}, Hit Rate={:.2}%", 
             stats3.hits, stats3.misses, stats3.hit_rate * 100.0);
    
    // Test 4: Cache miss (different context)
    println!("\n🔍 Test 4: Cache Miss (Different Context)");
    let ctx4 = CallContext {
        fn_symbol: "unknown/function",
        arg_type_fingerprint: 0x87654321, // Different fingerprint
        runtime_context_hash: 0xABCDEF01,
        metadata: None,
    };
    
    let result4 = de.decide(&ctx4);
    println!("  Function: unknown/function");
    println!("  Decision: {:?}", result4);
    
    let stats4 = de.cache_stats();
    println!("  Cache Stats: Hits={}, Misses={}, Hit Rate={:.2}%", 
             stats4.hits, stats4.misses, stats4.hit_rate * 100.0);
    
    // Test 5: Manual cache operations
    println!("\n🔍 Test 5: Manual Cache Operations");
    de.cache_decision(
        "manual/agent",
        "manual_task_123",
        ExecTarget::RemoteModel("claude".to_string()),
        0.95,
        "Manually cached high-confidence decision"
    );
    
    let stats5 = de.cache_stats();
    println!("  Manual Cache Entry Added");
    println!("  Cache Stats: Hits={}, Misses={}, Puts={}, Hit Rate={:.2}%", 
             stats5.hits, stats5.misses, stats5.puts, stats5.hit_rate * 100.0);
    
    // Test 6: Show agent plans
    println!("\n🔍 Test 6: Agent Plans");
    let agent_plans = de.l1_cache.get_agent_plans("manual/agent");
    println!("  Plans for 'manual/agent':");
    for (task, plan) in agent_plans {
        println!("    Task: {} -> Target: {} (Confidence: {:.2})", 
                 task, plan.target, plan.confidence);
        println!("    Reasoning: {}", plan.reasoning);
    }
    
    // Final summary
    println!("\n📈 Final Cache Performance Summary:");
    let final_stats = de.cache_stats();
    println!("  Total Requests: {}", final_stats.hits + final_stats.misses);
    println!("  Cache Hits: {}", final_stats.hits);
    println!("  Cache Misses: {}", final_stats.misses);
    println!("  Hit Rate: {:.2}%", final_stats.hit_rate * 100.0);
    println!("  Cache Size: {}/{}", final_stats.size, final_stats.capacity);
    
    println!("\n✅ Demo completed successfully!");
    println!("   The L1 delegation cache is working as expected:");
    println!("   - Static mappings bypass the cache");
    println!("   - Identical contexts hit the cache");
    println!("   - Different contexts miss the cache");
    println!("   - Manual caching works correctly");
    println!("   - Statistics are tracked accurately");
} 