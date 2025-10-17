//! Performance tests for the missing capability resolution system
//! 
//! This module tests the system's performance under various load conditions
//! and ensures it can handle concurrent operations efficiently.

use rtfs_compiler::ccos::{
    capability_marketplace::{marketplace::CapabilityMarketplace, types::CapabilityManifest},
    capabilities::registry::CapabilityRegistry,
    checkpoint_archive::CheckpointArchive,
    synthesis::{
        missing_capability_resolver::{MissingCapabilityResolver, ResolutionConfig},
        mcp_registry_client::McpRegistryClient,
        openapi_importer::OpenAPIImporter,
        graphql_importer::GraphQLImporter,
        http_wrapper::HTTPWrapper,
        capability_synthesizer::CapabilitySynthesizer,
        web_search_discovery::WebSearchDiscovery,
        validation_harness::ValidationHarness,
        registration_flow::RegistrationFlow,
        continuous_resolution::ContinuousResolutionLoop,
    },
    runtime::values::Value,
};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tokio::time::timeout;

/// Test concurrent missing capability resolution
#[tokio::test]
async fn test_concurrent_resolution_performance() {
    let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
    let marketplace = Arc::new(CapabilityMarketplace::new(registry));
    let checkpoint_archive = Arc::new(CheckpointArchive::new());
    let config = ResolutionConfig {
        auto_resolve: true,
        verbose_logging: false, // Disable logging for performance
    };
    let resolver = Arc::new(MissingCapabilityResolver::new(marketplace, checkpoint_archive, config));
    
    // Test with 100 concurrent missing capabilities
    let num_concurrent = 100;
    let start_time = Instant::now();
    
    let mut handles = vec![];
    for i in 0..num_concurrent {
        let resolver_clone = resolver.clone();
        let handle = tokio::spawn(async move {
            resolver_clone.handle_missing_capability(
                format!("test.capability.{}", i),
                vec![Value::String(format!("arg{}", i))],
                std::collections::HashMap::new(),
            ).await
        });
        handles.push(handle);
    }
    
    // Wait for all to complete with timeout
    let result = timeout(Duration::from_secs(30), async {
        for handle in handles {
            let result = handle.await.unwrap();
            assert!(result.is_ok(), "Concurrent resolution should succeed");
        }
    }).await;
    
    let elapsed = start_time.elapsed();
    
    assert!(result.is_ok(), "All concurrent operations should complete within timeout");
    assert!(elapsed < Duration::from_secs(30), "Should complete within 30 seconds");
    
    // Verify all are in the queue
    let stats = resolver.get_stats();
    assert!(stats.pending_count >= num_concurrent, "Should have all capabilities in queue");
    
    println!("✅ Concurrent resolution: {} capabilities in {:?}", num_concurrent, elapsed);
}

/// Test MCP Registry client performance
#[tokio::test]
async fn test_mcp_registry_performance() {
    let client = McpRegistryClient::new();
    
    // Test multiple concurrent searches
    let search_queries = vec![
        "github", "weather", "calendar", "email", "maps",
        "database", "analytics", "monitoring", "logging", "auth"
    ];
    
    let start_time = Instant::now();
    
    let mut handles = vec![];
    for query in search_queries {
        let client_clone = client.clone();
        let handle = tokio::spawn(async move {
            client_clone.search_servers(query).await
        });
        handles.push(handle);
    }
    
    // Wait for all searches to complete
    let results = timeout(Duration::from_secs(60), async {
        let mut all_results = vec![];
        for handle in handles {
            let result = handle.await.unwrap();
            all_results.push(result);
        }
        all_results
    }).await;
    
    let elapsed = start_time.elapsed();
    
    assert!(results.is_ok(), "All MCP searches should complete within timeout");
    
    let search_results = results.unwrap();
    let total_servers: usize = search_results.iter()
        .map(|r| r.as_ref().map(|s| s.len()).unwrap_or(0))
        .sum();
    
    println!("✅ MCP Registry performance: {} searches, {} total servers in {:?}", 
             search_queries.len(), total_servers, elapsed);
}

/// Test OpenAPI importer performance with large specs
#[tokio::test]
async fn test_openapi_importer_performance() {
    let importer = OpenAPIImporter::new();
    
    // Test with multiple large specs
    let num_specs = 10;
    let start_time = Instant::now();
    
    let mut handles = vec![];
    for i in 0..num_specs {
        let importer_clone = importer.clone();
        let handle = tokio::spawn(async move {
            let spec = importer_clone.load_mock_spec();
            let operations = importer_clone.extract_operations(&spec);
            
            // Convert each operation to capability
            let mut capabilities = vec![];
            for operation in operations {
                let capability = importer_clone.operation_to_capability(&operation);
                capabilities.push(capability);
            }
            capabilities
        });
        handles.push(handle);
    }
    
    // Wait for all processing to complete
    let results = timeout(Duration::from_secs(30), async {
        let mut all_capabilities = vec![];
        for handle in handles {
            let capabilities = handle.await.unwrap();
            all_capabilities.extend(capabilities);
        }
        all_capabilities
    }).await;
    
    let elapsed = start_time.elapsed();
    
    assert!(results.is_ok(), "All OpenAPI processing should complete within timeout");
    
    let capabilities = results.unwrap();
    println!("✅ OpenAPI importer performance: {} specs, {} capabilities in {:?}", 
             num_specs, capabilities.len(), elapsed);
}

/// Test GraphQL importer performance
#[tokio::test]
async fn test_graphql_importer_performance() {
    let importer = GraphQLImporter::new();
    
    // Test with multiple schemas
    let num_schemas = 10;
    let start_time = Instant::now();
    
    let mut handles = vec![];
    for i in 0..num_schemas {
        let importer_clone = importer.clone();
        let handle = tokio::spawn(async move {
            let schema = importer_clone.get_mock_schema();
            let operations = importer_clone.extract_operations(&schema);
            
            // Convert each operation to capability
            let mut capabilities = vec![];
            for operation in operations {
                let capability = importer_clone.operation_to_capability(&operation);
                capabilities.push(capability);
            }
            capabilities
        });
        handles.push(handle);
    }
    
    // Wait for all processing to complete
    let results = timeout(Duration::from_secs(30), async {
        let mut all_capabilities = vec![];
        for handle in handles {
            let capabilities = handle.await.unwrap();
            all_capabilities.extend(capabilities);
        }
        all_capabilities
    }).await;
    
    let elapsed = start_time.elapsed();
    
    assert!(results.is_ok(), "All GraphQL processing should complete within timeout");
    
    let capabilities = results.unwrap();
    println!("✅ GraphQL importer performance: {} schemas, {} capabilities in {:?}", 
             num_schemas, capabilities.len(), elapsed);
}

/// Test capability synthesizer performance
#[tokio::test]
async fn test_capability_synthesizer_performance() {
    let synthesizer = CapabilitySynthesizer::new();
    
    // Test generating many capabilities
    let num_capabilities = 50;
    let start_time = Instant::now();
    
    let mut handles = vec![];
    for i in 0..num_capabilities {
        let synthesizer_clone = synthesizer.clone();
        let handle = tokio::spawn(async move {
            let capability = synthesizer_clone.generate_mock_capability(&format!("test.capability.{}", i));
            let quality_score = synthesizer_clone.calculate_quality_score(&capability);
            (capability, quality_score)
        });
        handles.push(handle);
    }
    
    // Wait for all synthesis to complete
    let results = timeout(Duration::from_secs(30), async {
        let mut all_results = vec![];
        for handle in handles {
            let result = handle.await.unwrap();
            all_results.push(result);
        }
        all_results
    }).await;
    
    let elapsed = start_time.elapsed();
    
    assert!(results.is_ok(), "All capability synthesis should complete within timeout");
    
    let synthesis_results = results.unwrap();
    let avg_quality: f64 = synthesis_results.iter()
        .map(|(_, score)| *score)
        .sum::<f64>() / synthesis_results.len() as f64;
    
    println!("✅ Capability synthesizer performance: {} capabilities, avg quality {:.2} in {:?}", 
             num_capabilities, avg_quality, elapsed);
}

/// Test web search discovery performance
#[tokio::test]
async fn test_web_search_discovery_performance() {
    let discovery = WebSearchDiscovery::new();
    
    // Test multiple concurrent searches
    let search_queries = vec![
        "weather api", "calendar api", "email api", "maps api", "database api",
        "analytics api", "monitoring api", "logging api", "auth api", "payment api"
    ];
    
    let start_time = Instant::now();
    
    let mut handles = vec![];
    for query in search_queries {
        let discovery_clone = discovery.clone();
        let handle = tokio::spawn(async move {
            discovery_clone.search_for_api_specs(query).await
        });
        handles.push(handle);
    }
    
    // Wait for all searches to complete
    let results = timeout(Duration::from_secs(60), async {
        let mut all_results = vec![];
        for handle in handles {
            let result = handle.await.unwrap();
            all_results.push(result);
        }
        all_results
    }).await;
    
    let elapsed = start_time.elapsed();
    
    assert!(results.is_ok(), "All web searches should complete within timeout");
    
    let search_results = results.unwrap();
    let total_results: usize = search_results.iter()
        .map(|r| r.as_ref().map(|s| s.len()).unwrap_or(0))
        .sum();
    
    println!("✅ Web search discovery performance: {} searches, {} total results in {:?}", 
             search_queries.len(), total_results, elapsed);
}

/// Test validation harness performance
#[tokio::test]
async fn test_validation_harness_performance() {
    let harness = ValidationHarness::new();
    
    // Test validating many capabilities
    let num_capabilities = 100;
    let start_time = Instant::now();
    
    let mut handles = vec![];
    for i in 0..num_capabilities {
        let harness_clone = harness.clone();
        let handle = tokio::spawn(async move {
            let capability = create_test_capability(&format!("test.capability.{}", i));
            let rtfs_code = format!("(defn test{} [x] x)", i);
            harness_clone.validate_capability(&capability, Some(&rtfs_code))
        });
        handles.push(handle);
    }
    
    // Wait for all validation to complete
    let results = timeout(Duration::from_secs(30), async {
        let mut all_results = vec![];
        for handle in handles {
            let result = handle.await.unwrap();
            all_results.push(result);
        }
        all_results
    }).await;
    
    let elapsed = start_time.elapsed();
    
    assert!(results.is_ok(), "All validation should complete within timeout");
    
    let validation_results = results.unwrap();
    let total_issues: usize = validation_results.iter()
        .map(|r| r.as_ref().map(|v| v.issues.len()).unwrap_or(0))
        .sum();
    
    println!("✅ Validation harness performance: {} capabilities, {} total issues in {:?}", 
             num_capabilities, total_issues, elapsed);
}

/// Test registration flow performance
#[tokio::test]
async fn test_registration_flow_performance() {
    let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
    let marketplace = Arc::new(CapabilityMarketplace::new(registry));
    let registration_flow = RegistrationFlow::new(marketplace);
    
    // Test registering many capabilities
    let num_capabilities = 50;
    let start_time = Instant::now();
    
    let mut handles = vec![];
    for i in 0..num_capabilities {
        let flow_clone = registration_flow.clone();
        let handle = tokio::spawn(async move {
            let capability = create_test_capability(&format!("test.capability.{}", i));
            let rtfs_code = format!("(defn test{} [x] x)", i);
            flow_clone.register_capability(&capability, &rtfs_code).await
        });
        handles.push(handle);
    }
    
    // Wait for all registrations to complete
    let results = timeout(Duration::from_secs(60), async {
        let mut all_results = vec![];
        for handle in handles {
            let result = handle.await.unwrap();
            all_results.push(result);
        }
        all_results
    }).await;
    
    let elapsed = start_time.elapsed();
    
    assert!(results.is_ok(), "All registrations should complete within timeout");
    
    let registration_results = results.unwrap();
    let successful_registrations = registration_results.iter()
        .filter(|r| r.as_ref().map(|reg| reg.success).unwrap_or(false))
        .count();
    
    println!("✅ Registration flow performance: {} capabilities, {} successful in {:?}", 
             num_capabilities, successful_registrations, elapsed);
}

/// Test continuous resolution loop performance
#[tokio::test]
async fn test_continuous_resolution_loop_performance() {
    let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
    let marketplace = Arc::new(CapabilityMarketplace::new(registry));
    let checkpoint_archive = Arc::new(CheckpointArchive::new());
    let config = ResolutionConfig {
        auto_resolve: true,
        verbose_logging: false,
    };
    let resolver = Arc::new(MissingCapabilityResolver::new(marketplace, checkpoint_archive, config));
    
    // Add many missing capabilities
    let num_capabilities = 100;
    for i in 0..num_capabilities {
        let _ = resolver.handle_missing_capability(
            format!("test.capability.{}", i),
            vec![Value::String(format!("arg{}", i))],
            std::collections::HashMap::new(),
        ).await;
    }
    
    // Test continuous resolution loop performance
    let loop_controller = ContinuousResolutionLoop::new(resolver.clone());
    let start_time = Instant::now();
    
    // Run the loop multiple times
    let num_iterations = 10;
    for _ in 0..num_iterations {
        let _ = loop_controller.process_pending_resolutions().await;
    }
    
    let elapsed = start_time.elapsed();
    
    // Get final statistics
    let stats = loop_controller.get_resolution_stats();
    
    println!("✅ Continuous resolution loop performance: {} iterations in {:?}", 
             num_iterations, elapsed);
    println!("   📊 Total attempts: {}", stats.total_attempts);
    println!("   ✅ Successful: {}", stats.successful_resolutions);
    println!("   ❌ Failed: {}", stats.failed_resolutions);
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
    let num_capabilities = 1000;
    let start_time = Instant::now();
    
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
        ).await;
    }
    
    let elapsed = start_time.elapsed();
    
    // Check that we can still get statistics without memory issues
    let stats = resolver.get_stats();
    assert!(stats.pending_count >= num_capabilities, "Should have all capabilities in queue");
    
    println!("✅ Memory usage test: {} capabilities queued in {:?}", 
             num_capabilities, elapsed);
    println!("   📊 Queue stats: pending={}, in_progress={}, failed={}", 
             stats.pending_count, stats.in_progress_count, stats.failed_count);
}

/// Helper function to create test capabilities
fn create_test_capability(id: &str) -> CapabilityManifest {
    CapabilityManifest {
        id: id.to_string(),
        name: format!("Test Capability {}", id),
        description: Some(format!("A test capability for {}", id)),
        input_schema: Some(serde_json::json!({
            "type": "object",
            "properties": {
                "param1": {"type": "string"}
            }
        })),
        output_schema: Some(serde_json::json!({
            "type": "string"
        })),
        provider: rtfs_compiler::ccos::capability_marketplace::types::ProviderType::Local(
            rtfs_compiler::ccos::capability_marketplace::types::LocalCapability {
                handler: Arc::new(|_args| Ok(Value::String("test".to_string()))),
            }
        ),
        metadata: std::collections::HashMap::new(),
        agent_metadata: None,
    }
}

