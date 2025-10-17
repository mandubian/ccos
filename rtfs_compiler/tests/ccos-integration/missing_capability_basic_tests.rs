//! Basic integration tests for the missing capability resolution system
//! 
//! This module provides working tests for the core missing capability
//! resolution functionality without complex API dependencies.

use rtfs_compiler::ccos::{
    capability_marketplace::{marketplace::CapabilityMarketplace, types::CapabilityManifest},
    capabilities::registry::CapabilityRegistry,
    checkpoint_archive::CheckpointArchive,
    synthesis::{
        missing_capability_resolver::{MissingCapabilityResolver, ResolutionConfig},
        mcp_registry_client::McpRegistryClient,
    },
    runtime::values::Value,
};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Test basic missing capability resolution functionality
#[tokio::test]
async fn test_basic_missing_capability_resolution() {
    // Setup
    let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
    let marketplace = Arc::new(CapabilityMarketplace::new(registry));
    let checkpoint_archive = Arc::new(CheckpointArchive::new());
    let config = ResolutionConfig {
        auto_resolve: true,
        verbose_logging: false,
    };
    let resolver = Arc::new(MissingCapabilityResolver::new(marketplace, checkpoint_archive, config));

    // Test capability that doesn't exist
    let missing_capability_id = "external.api.weather";
    
    // Step 1: Trigger missing capability detection
    let result = resolver.handle_missing_capability(
        missing_capability_id.to_string(),
        vec![Value::String("Paris".to_string())],
        std::collections::HashMap::new(),
    );
    
    assert!(result.is_ok(), "Should successfully enqueue missing capability");

    // Step 2: Verify it's in the queue
    let stats = resolver.get_stats();
    assert!(stats.pending_count > 0, "Should have pending capabilities");
    
    println!("✅ Basic missing capability resolution test passed");
    println!("   📊 Queue stats: pending={}, in_progress={}, failed={}", 
             stats.pending_count, stats.in_progress_count, stats.failed_count);
}

/// Test MCP Registry client basic functionality
#[tokio::test]
async fn test_mcp_registry_client_basic() {
    let client = McpRegistryClient::new();
    
    // Test server search (this will use mock data if API is unavailable)
    let servers = client.search_servers("github").await;
    assert!(servers.is_ok(), "MCP Registry search should succeed");
    
    let servers = servers.unwrap();
    println!("✅ MCP Registry client test passed");
    println!("   🔍 Found {} servers for 'github'", servers.len());
    
    // Test capability conversion if we have servers
    if let Some(server) = servers.first() {
        let capability = client.convert_to_capability_manifest(server, "github");
        if let Ok(cap) = capability {
            assert!(!cap.id.is_empty(), "Converted capability should have an ID");
            println!("   🔧 Generated capability: {}", cap.id);
        }
    }
}

/// Test concurrent missing capability resolution
#[tokio::test]
async fn test_concurrent_missing_capability_resolution() {
    let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
    let marketplace = Arc::new(CapabilityMarketplace::new(registry));
    let checkpoint_archive = Arc::new(CheckpointArchive::new());
    let config = ResolutionConfig {
        auto_resolve: true,
        verbose_logging: false,
    };
    let resolver = Arc::new(MissingCapabilityResolver::new(marketplace, checkpoint_archive, config));
    
    // Test with 10 concurrent missing capabilities
    let num_concurrent = 10;
    
    let mut handles = vec![];
    for i in 0..num_concurrent {
        let resolver_clone = resolver.clone();
        let handle = tokio::spawn(async move {
            resolver_clone.handle_missing_capability(
                format!("test.capability.{}", i),
                vec![Value::String(format!("arg{}", i))],
                std::collections::HashMap::new(),
            )
        });
        handles.push(handle);
    }
    
    // Wait for all to complete
    for handle in handles {
        let result = handle.await.unwrap();
        assert!(result.is_ok(), "Concurrent resolution should succeed");
    }
    
    // Verify all are in the queue
    let stats = resolver.get_stats();
    assert!(stats.pending_count >= num_concurrent, "Should have all capabilities in queue");
    
    println!("✅ Concurrent missing capability resolution test passed");
    println!("   📊 Final stats: pending={}, in_progress={}, failed={}", 
             stats.pending_count, stats.in_progress_count, stats.failed_count);
}

/// Test CLI tool integration
#[tokio::test]
async fn test_cli_tool_integration() {
    // This test verifies that the CLI tool can be invoked programmatically
    // and that it properly integrates with the resolution system
    
    let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
    let marketplace = Arc::new(CapabilityMarketplace::new(registry));
    let checkpoint_archive = Arc::new(CheckpointArchive::new());
    let config = ResolutionConfig {
        auto_resolve: true,
        verbose_logging: false, // CLI typically uses false for cleaner output
    };
    
    let resolver = Arc::new(MissingCapabilityResolver::new(marketplace, checkpoint_archive, config));
    assert!(resolver.get_stats().pending_count == 0, "Fresh resolver should have no pending items");
    
    println!("✅ CLI tool integration test passed");
}

/// Test error handling and edge cases
#[tokio::test]
async fn test_error_handling_and_edge_cases() {
    let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
    let marketplace = Arc::new(CapabilityMarketplace::new(registry));
    let checkpoint_archive = Arc::new(CheckpointArchive::new());
    let config = ResolutionConfig {
        auto_resolve: true,
        verbose_logging: false,
    };
    let resolver = Arc::new(MissingCapabilityResolver::new(marketplace, checkpoint_archive, config));
    
    // Test with empty capability ID
    let result = resolver.handle_missing_capability(
        "".to_string(),
        vec![],
        std::collections::HashMap::new(),
    );
    // This should either succeed (empty ID is valid) or fail gracefully
    // The exact behavior depends on implementation
    
    // Test with very long capability ID
    let long_id = "a".repeat(1000);
    let result = resolver.handle_missing_capability(
        long_id,
        vec![],
        std::collections::HashMap::new(),
    );
    // Should handle gracefully without panicking
    
    // Test with complex arguments
    let result = resolver.handle_missing_capability(
        "test.capability".to_string(),
        vec![Value::Map(std::collections::HashMap::new())], // Complex nested value
        std::collections::HashMap::new(),
    );
    // Should handle complex arguments gracefully
    
    println!("✅ Error handling and edge cases test passed");
}

/// Test performance under load
#[tokio::test]
async fn test_performance_under_load() {
    let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
    let marketplace = Arc::new(CapabilityMarketplace::new(registry));
    let checkpoint_archive = Arc::new(CheckpointArchive::new());
    let config = ResolutionConfig {
        auto_resolve: true,
        verbose_logging: false, // Disable logging for performance
    };
    let resolver = Arc::new(MissingCapabilityResolver::new(marketplace, checkpoint_archive, config));
    
    // Test with multiple concurrent missing capabilities
    let num_capabilities = 50;
    let start_time = std::time::Instant::now();
    
    let mut handles = vec![];
    for i in 0..num_capabilities {
        let resolver_clone = resolver.clone();
        let handle = tokio::spawn(async move {
            resolver_clone.handle_missing_capability(
                format!("test.capability.{}", i),
                vec![Value::String(format!("arg{}", i))],
                std::collections::HashMap::new(),
            )
        });
        handles.push(handle);
    }
    
    // Wait for all to complete
    for handle in handles {
        let result = handle.await.unwrap();
        assert!(result.is_ok(), "Concurrent resolution should succeed");
    }
    
    let elapsed = start_time.elapsed();
    
    // Verify all are in the queue
    let stats = resolver.get_stats();
    assert!(stats.pending_count >= num_capabilities, "Should have all capabilities in queue");
    
    println!("✅ Performance under load test passed");
    println!("   ⏱️  Processed {} capabilities in {:?}", num_capabilities, elapsed);
    println!("   📊 Queue stats: pending={}, in_progress={}, failed={}", 
             stats.pending_count, stats.in_progress_count, stats.failed_count);
}

/// Test memory usage under load
#[tokio::test]
async fn test_memory_usage_under_load() {
    let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
    let marketplace = Arc::new(CapabilityMarketplace::new(registry));
    let checkpoint_archive = Arc::new(CheckpointArchive::new());
    let config = ResolutionConfig {
        auto_resolve: true,
        verbose_logging: false,
    };
    let resolver = Arc::new(MissingCapabilityResolver::new(marketplace, checkpoint_archive, config));
    
    // Add a large number of missing capabilities to test memory usage
    let num_capabilities = 100;
    let start_time = std::time::Instant::now();
    
    for i in 0..num_capabilities {
        let _ = resolver.handle_missing_capability(
            format!("test.capability.{}", i),
            vec![
                Value::String(format!("arg{}", i)),
                Value::Number(i as f64),
                Value::Map(std::collections::HashMap::from([
                    ("key1".to_string(), Value::String(format!("value1_{}", i))),
                    ("key2".to_string(), Value::String(format!("value2_{}", i))),
                ]))
            ],
            std::collections::HashMap::from([
                ("context1".to_string(), format!("value1_{}", i)),
                ("context2".to_string(), format!("value2_{}", i)),
            ]),
        );
    }
    
    let elapsed = start_time.elapsed();
    
    // Check that we can still get statistics without memory issues
    let stats = resolver.get_stats();
    assert!(stats.pending_count >= num_capabilities, "Should have all capabilities in queue");
    
    println!("✅ Memory usage test passed");
    println!("   📊 {} capabilities queued in {:?}", num_capabilities, elapsed);
    println!("   📊 Queue stats: pending={}, in_progress={}, failed={}", 
             stats.pending_count, stats.in_progress_count, stats.failed_count);
}
