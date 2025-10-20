//! Simple example demonstrating the missing capability resolution system
//!
//! This example shows the basic functionality without complex dependencies.

use rtfs_compiler::ast::MapKey;
use rtfs_compiler::ccos::{
    capabilities::registry::CapabilityRegistry,
    capability_marketplace::types::CapabilityMarketplace,
    checkpoint_archive::CheckpointArchive,
    synthesis::{
        feature_flags::MissingCapabilityConfig,
        mcp_registry_client::McpRegistryClient,
        missing_capability_resolver::{MissingCapabilityResolver, ResolverConfig},
    },
};
use rtfs_compiler::runtime::values::Value;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Simple example of missing capability resolution
#[tokio::main]
async fn main() {
    println!("🚀 Simple Missing Capability Resolution Example");
    println!("===============================================");
    println!();

    // Setup the resolution system
    let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
    let marketplace = Arc::new(CapabilityMarketplace::new(registry));
    let checkpoint_archive = Arc::new(CheckpointArchive::new());
    let config = ResolverConfig {
        max_attempts: 3,
        auto_resolve: true,
        verbose_logging: true,
    };
    let resolver = Arc::new(MissingCapabilityResolver::new(
        marketplace,
        checkpoint_archive,
        config,
        MissingCapabilityConfig::default(),
    ));

    // Example 1: Basic missing capability detection
    println!("📝 Example 1: Basic Missing Capability Detection");
    println!("-----------------------------------------------");

    let missing_capability_id = "external.api.weather";
    println!(
        "🔍 Attempting to use '{}' capability...",
        missing_capability_id
    );

    let result = resolver.handle_missing_capability(
        missing_capability_id.to_string(),
        vec![Value::String("Paris".to_string())],
        std::collections::HashMap::new(),
    );

    if result.is_ok() {
        println!("✅ Missing capability queued for resolution");
    } else {
        println!("❌ Failed to queue missing capability: {:?}", result.err());
    }

    // Check queue status
    let stats = resolver.get_stats();
    println!("📊 Queue Status:");
    println!("   ⏳ Pending: {}", stats.pending_count);
    println!("   🔄 In Progress: {}", stats.in_progress_count);
    println!("   ❌ Failed: {}", stats.failed_count);
    println!();

    // Example 2: MCP Registry discovery
    println!("📝 Example 2: MCP Registry Discovery");
    println!("-----------------------------------");

    let mcp_client = McpRegistryClient::new();
    println!("🔍 Searching MCP Registry for 'github' servers...");

    let servers = mcp_client.search_servers("github").await;
    match servers {
        Ok(servers) => {
            println!("✅ Found {} GitHub MCP server(s)", servers.len());
            for (i, server) in servers.iter().enumerate() {
                println!("   {}. {} - {}", i + 1, server.name, server.description);
            }

            // Try to convert first server to capability
            if let Some(server) = servers.first() {
                println!("🔧 Converting MCP server to capability...");
                let capability_result = mcp_client.convert_to_capability_manifest(server, "github");
                match capability_result {
                    Ok(capability) => {
                        println!("✅ Generated capability: {}", capability.id);
                        println!("   📋 Description: {}", capability.description);
                    }
                    Err(e) => {
                        println!("❌ Failed to convert server to capability: {:?}", e);
                    }
                }
            }
        }
        Err(e) => {
            println!("❌ MCP Registry search failed: {:?}", e);
        }
    }
    println!();

    // Example 3: Multiple concurrent missing capabilities
    println!("📝 Example 3: Concurrent Missing Capabilities");
    println!("---------------------------------------------");

    let missing_capabilities = vec![
        "weather.current",
        "maps.directions",
        "calendar.events.create",
        "email.send",
    ];

    println!(
        "🔍 Attempting to use {} missing capabilities concurrently...",
        missing_capabilities.len()
    );

    let mut handles = vec![];
    for (i, capability_id) in missing_capabilities.iter().enumerate() {
        let resolver_clone = resolver.clone();
        let capability_id = capability_id.clone();
        let handle = tokio::spawn(async move {
            resolver_clone.handle_missing_capability(
                capability_id.to_string(),
                vec![Value::String(format!("arg{}", i))],
                std::collections::HashMap::new(),
            )
        });
        handles.push(handle);
    }

    // Wait for all to complete
    let mut success_count = 0;
    for handle in handles {
        match handle.await {
            Ok(result) => {
                if result.is_ok() {
                    success_count += 1;
                }
            }
            Err(e) => {
                println!("❌ Task failed: {:?}", e);
            }
        }
    }

    println!(
        "✅ Successfully queued {}/{} capabilities",
        success_count,
        missing_capabilities.len()
    );

    // Final queue status
    let final_stats = resolver.get_stats();
    println!("📊 Final Queue Status:");
    println!("   ⏳ Pending: {}", final_stats.pending_count);
    println!("   🔄 In Progress: {}", final_stats.in_progress_count);
    println!("   ❌ Failed: {}", final_stats.failed_count);
    println!();

    // Example 4: Error handling
    println!("📝 Example 4: Error Handling");
    println!("---------------------------");

    // Test with empty capability ID
    println!("🔍 Testing with empty capability ID...");
    let result = resolver.handle_missing_capability(
        "".to_string(),
        vec![],
        std::collections::HashMap::new(),
    );
    match result {
        Ok(_) => println!("✅ Empty capability ID handled gracefully"),
        Err(e) => println!("❌ Empty capability ID failed: {:?}", e),
    }

    // Test with very long capability ID
    println!("🔍 Testing with very long capability ID...");
    let long_id = "a".repeat(1000);
    let result =
        resolver.handle_missing_capability(long_id, vec![], std::collections::HashMap::new());
    match result {
        Ok(_) => println!("✅ Long capability ID handled gracefully"),
        Err(e) => println!("❌ Long capability ID failed: {:?}", e),
    }

    // Test with complex arguments
    println!("🔍 Testing with complex arguments...");
    let complex_args = vec![
        Value::String("test".to_string()),
        Value::Float(42.0),
        Value::Map(std::collections::HashMap::from([
            (
                MapKey::String("key1".to_string()),
                Value::String("value1".to_string()),
            ),
            (
                MapKey::String("key2".to_string()),
                Value::String("value2".to_string()),
            ),
        ])),
    ];
    let result = resolver.handle_missing_capability(
        "test.complex".to_string(),
        complex_args,
        std::collections::HashMap::from([
            ("context1".to_string(), "value1".to_string()),
            ("context2".to_string(), "value2".to_string()),
        ]),
    );
    match result {
        Ok(_) => println!("✅ Complex arguments handled gracefully"),
        Err(e) => println!("❌ Complex arguments failed: {:?}", e),
    }

    println!();
    println!("🎉 Example completed successfully!");
    println!();
    println!("💡 Key Takeaways:");
    println!("   • Missing capabilities are automatically detected and queued");
    println!("   • MCP Registry provides server discovery capabilities");
    println!("   • The system handles concurrent operations efficiently");
    println!("   • Error handling is robust for edge cases");
    println!("   • Queue statistics provide visibility into resolution status");
}
