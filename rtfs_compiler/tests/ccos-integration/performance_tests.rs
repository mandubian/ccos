//! Performance tests for the missing capability resolution system
//!
//! This module tests the system's performance under various load conditions
//! and ensures it can handle concurrent operations efficiently.

use rtfs_compiler::ccos::{
    capabilities::registry::CapabilityRegistry,
    capability_marketplace::{types::CapabilityManifest, CapabilityMarketplace},
    checkpoint_archive::CheckpointArchive,
    synthesis::{
        capability_synthesizer::CapabilitySynthesizer,
        continuous_resolution::ContinuousResolutionLoop,
        graphql_importer::GraphQLImporter,
        http_wrapper::HTTPWrapper,
        mcp_registry_client::McpRegistryClient,
        missing_capability_resolver::{MissingCapabilityResolver, ResolverConfig},
        openapi_importer::OpenAPIImporter,
        registration_flow::RegistrationFlow,
        validation_harness::ValidationHarness,
        web_search_discovery::WebSearchDiscovery,
    },
};
use rtfs_compiler::runtime::values::Value;
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
    let config = ResolverConfig {
        max_attempts: 3,
        auto_resolve: true,
        verbose_logging: false, // Disable logging for performance
        ..ResolverConfig::default()
    };
    let resolver = Arc::new(MissingCapabilityResolver::new(
        marketplace,
        checkpoint_archive,
        config,
        rtfs_compiler::ccos::synthesis::feature_flags::MissingCapabilityConfig::testing(),
    ));

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
            )
        });
        handles.push(handle);
    }

    // Wait for all to complete with timeout
    let result = timeout(Duration::from_secs(30), async {
        for handle in handles {
            let result = handle.await.unwrap();
            assert!(result.is_ok(), "Concurrent resolution should succeed");
        }
    })
    .await;

    let elapsed = start_time.elapsed();
    assert!(
        result.is_ok(),
        "All concurrent operations should complete within timeout"
    );

    assert!(
        elapsed < Duration::from_secs(30),
        "Should complete within 30 seconds"
    );

    let stats = resolver.get_stats();
    assert!(
        stats.pending_count >= num_concurrent,
        "Should have all capabilities in queue"
    );

    println!(
        "✅ Concurrent resolution: {} capabilities in {:?}",
        num_concurrent, elapsed
    );

    println!(
        "✅ Concurrent resolution: {} capabilities in {:?}",
        num_concurrent, elapsed
    );
}

/// Test MCP Registry client performance
#[tokio::test]
async fn test_mcp_registry_performance() {
    let client = Arc::new(McpRegistryClient::new());

    // Test multiple concurrent searches
    let search_queries = vec![
        "github",
        "weather",
        "calendar",
        "email",
        "maps",
        "database",
        "analytics",
        "monitoring",
        "logging",
        "auth",
    ];

    let start_time = Instant::now();

    let mut handles = vec![];
    for query in &search_queries {
        let client_clone = client.clone();
        let q = query.to_string();
        let handle = tokio::spawn(async move { client_clone.search_servers(&q).await });
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
    })
    .await;

    let elapsed = start_time.elapsed();

    assert!(
        results.is_ok(),
        "All MCP searches should complete within timeout"
    );

    let search_results = results.unwrap();
    let total_servers: usize = search_results
        .iter()
        .map(|r| r.as_ref().map(|v| v.len()).unwrap_or(0))
        .sum();

    println!(
        "✅ MCP Registry performance: {} searches, {} total servers in {:?}",
        search_queries.len(),
        total_servers,
        elapsed
    );
}

/// Test OpenAPI importer performance with large specs
#[tokio::test]
async fn test_openapi_importer_performance() {
    let importer = OpenAPIImporter::mock("https://example.com".to_string());

    // Test with multiple large specs sequentially
    let num_specs = 10;
    let start_time = Instant::now();

    let mut all_capabilities = vec![];
    for _ in 0..num_specs {
        let spec = importer.load_spec("mock").await.expect("load mock spec");
        let operations = importer
            .extract_operations(&spec)
            .expect("extract operations");
        for operation in operations {
            let cap = importer
                .operation_to_capability(&operation, "example")
                .expect("operation to capability");
            all_capabilities.push(cap);
        }
    }

    let elapsed = start_time.elapsed();

    println!(
        "✅ OpenAPI importer performance: {} specs, {} capabilities in {:?}",
        num_specs,
        all_capabilities.len(),
        elapsed
    );
}

/// Test GraphQL importer performance
#[tokio::test]
async fn test_graphql_importer_performance() {
    let importer = GraphQLImporter::mock("https://example.com/graphql".to_string());

    // Test with multiple schemas sequentially
    let num_schemas = 10;
    let start_time = Instant::now();
    let mut capabilities = vec![];
    for _ in 0..num_schemas {
        let schema = importer
            .introspect_schema()
            .await
            .expect("introspect schema");
        let ops = importer
            .extract_operations(&schema)
            .expect("extract operations");
        for op in ops {
            let cap = importer
                .operation_to_capability(&op, "example")
                .expect("operation to capability");
            capabilities.push(cap);
        }
    }

    let elapsed = start_time.elapsed();
    println!(
        "✅ GraphQL importer performance: {} schemas, {} capabilities in {:?}",
        num_schemas,
        capabilities.len(),
        elapsed
    );
}

/// Test capability synthesizer performance
#[tokio::test]
#[ignore = "LLM synthesis not yet implemented"]
async fn test_capability_synthesizer_performance() {
    let synthesizer = CapabilitySynthesizer::new();

    // Test generating many capabilities sequentially
    let num_capabilities = 50;
    let start_time = Instant::now();
    let mut synthesis_results = vec![];
    for i in 0..num_capabilities {
        let request = rtfs_compiler::ccos::synthesis::capability_synthesizer::SynthesisRequest {
            capability_name: format!("test.capability.{}", i),
            input_schema: None,
            output_schema: None,
            requires_auth: false,
            auth_provider: None,
            description: Some("Performance test synth".to_string()),
            context: None,
        };
        let result = synthesizer
            .synthesize_capability(&request)
            .await
            .expect("synthesize mock");
        let quality_score = synthesizer.calculate_quality_score(&request, &result);
        synthesis_results.push((result.capability, quality_score));
    }

    let elapsed = start_time.elapsed();
    let avg_quality: f64 =
        synthesis_results.iter().map(|(_, s)| *s).sum::<f64>() / synthesis_results.len() as f64;
    println!(
        "✅ Capability synthesizer performance: {} capabilities, avg quality {:.2} in {:?}",
        num_capabilities, avg_quality, elapsed
    );
}

/// Test web search discovery performance
#[tokio::test]
async fn test_web_search_discovery_performance() {
    let mut discovery = WebSearchDiscovery::mock();

    // Test multiple concurrent searches
    let search_queries = vec![
        "weather api",
        "calendar api",
        "email api",
        "maps api",
        "database api",
        "analytics api",
        "monitoring api",
        "logging api",
        "auth api",
        "payment api",
    ];

    let start_time = Instant::now();

    // Run searches sequentially (mock discovery is inexpensive in tests)
    let mut all_results: Vec<
        rtfs_compiler::runtime::error::RuntimeResult<
            Vec<rtfs_compiler::ccos::synthesis::web_search_discovery::WebSearchResult>,
        >,
    > = Vec::new();
    for query in &search_queries {
        let res = discovery.search_for_api_specs(query).await;
        all_results.push(res);
    }

    let elapsed = start_time.elapsed();

    // Count total results (treat errors as zero results)
    let total_results: usize = all_results
        .iter()
        .map(|r| r.as_ref().map(|v| v.len()).unwrap_or(0))
        .sum();

    println!(
        "✅ Web search discovery performance: {} searches, {} total results in {:?}",
        search_queries.len(),
        total_results,
        elapsed
    );
}

/// Test validation harness performance
#[tokio::test]
async fn test_validation_harness_performance() {
    let harness = Arc::new(ValidationHarness::new());

    // Test validating many capabilities
    let num_capabilities = 100;
    let start_time = Instant::now();

    let mut validation_results = vec![];
    for i in 0..num_capabilities {
        let id = format!("test.capability.{}", i);
        let code = format!("(defn test{} [x] x)", i);
        let res = harness.validate_capability(&create_test_capability(&id), &code);
        validation_results.push(res);
    }

    let elapsed = start_time.elapsed();

    let total_issues: usize = validation_results.iter().map(|v| v.issues.len()).sum();

    println!(
        "✅ Validation harness performance: {} capabilities, {} total issues in {:?}",
        num_capabilities, total_issues, elapsed
    );
}

/// Test registration flow performance
#[tokio::test]
async fn test_registration_flow_performance() {
    let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
    let marketplace = Arc::new(CapabilityMarketplace::new(registry));
    let registration_flow = Arc::new(RegistrationFlow::new(marketplace));

    // Test registering many capabilities
    let num_capabilities = 50;
    let start_time = Instant::now();

    let mut registration_results = vec![];
    for i in 0..num_capabilities {
        let id = format!("test.capability.{}", i);
        let code = format!("(defn test{} [x] x)", i);
        let res = registration_flow
            .register_capability(create_test_capability(&id), Some(code.as_str()))
            .await;
        registration_results.push(res);
    }

    let elapsed = start_time.elapsed();

    let successful_registrations = registration_results
        .iter()
        .filter(|r| r.as_ref().map(|reg| reg.success).unwrap_or(false))
        .count();

    println!(
        "✅ Registration flow performance: {} capabilities, {} successful in {:?}",
        num_capabilities, successful_registrations, elapsed
    );
}

/// Test continuous resolution loop performance
#[tokio::test]
async fn test_continuous_resolution_loop_performance() {
    let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
    let marketplace = Arc::new(CapabilityMarketplace::new(registry));
    let checkpoint_archive = Arc::new(CheckpointArchive::new());
    let resolver_cfg = ResolverConfig {
        max_attempts: 3,
        auto_resolve: true,
        verbose_logging: false,
        ..ResolverConfig::default()
    };
    let resolver = Arc::new(MissingCapabilityResolver::new(
        marketplace.clone(),
        checkpoint_archive,
        resolver_cfg,
        rtfs_compiler::ccos::synthesis::feature_flags::MissingCapabilityConfig::testing(),
    ));

    // Add many missing capabilities
    let num_capabilities = 100;
    for i in 0..num_capabilities {
        let _ = resolver.handle_missing_capability(
            format!("test.capability.{}", i),
            vec![Value::String(format!("arg{}", i))],
            std::collections::HashMap::new(),
        );
    }

    // Test continuous resolution loop performance
    let registration_flow = Arc::new(RegistrationFlow::new(marketplace.clone()));
    let loop_cfg =
        rtfs_compiler::ccos::synthesis::continuous_resolution::ResolutionConfig::default();
    let loop_controller = ContinuousResolutionLoop::new(
        resolver.clone(),
        registration_flow,
        marketplace.clone(),
        loop_cfg,
    );
    let start_time = Instant::now();

    // Run the loop multiple times
    let num_iterations = 10;
    for _ in 0..num_iterations {
        let _ = loop_controller.process_pending_resolutions().await;
    }

    let elapsed = start_time.elapsed();

    // Get final statistics
    let stats = loop_controller
        .get_resolution_stats()
        .await
        .expect("get stats");

    println!(
        "✅ Continuous resolution loop performance: {} iterations in {:?}",
        num_iterations, elapsed
    );
    println!("   📊 Total capabilities: {}", stats.total_capabilities);
    println!("   ✅ Successful: {}", stats.successful_resolutions);
    println!("   ❌ Failed: {}", stats.failed_resolutions);
}

/// Test memory usage under load
#[tokio::test]
async fn test_memory_usage_under_load() {
    let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
    let marketplace = Arc::new(CapabilityMarketplace::new(registry));
    let checkpoint_archive = Arc::new(CheckpointArchive::new());
    let config = ResolverConfig {
        max_attempts: 3,
        auto_resolve: true,
        verbose_logging: false,
    };
    let resolver = Arc::new(MissingCapabilityResolver::new(
        marketplace,
        checkpoint_archive,
        config,
        rtfs_compiler::ccos::synthesis::feature_flags::MissingCapabilityConfig::testing(),
    ));

    // Add a large number of missing capabilities to test memory usage
    let num_capabilities = 1000;
    let start_time = Instant::now();

    for i in 0..num_capabilities {
        let _ = resolver.handle_missing_capability(
            format!("test.capability.{}", i),
            vec![
                Value::String(format!("arg{}", i)),
                Value::Float(i as f64),
                Value::Map(std::collections::HashMap::from([
                    (
                        rtfs_compiler::ast::MapKey::String("key1".to_string()),
                        Value::String(format!("value1_{}", i)),
                    ),
                    (
                        rtfs_compiler::ast::MapKey::String("key2".to_string()),
                        Value::String(format!("value2_{}", i)),
                    ),
                ])),
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
    assert!(
        stats.pending_count >= num_capabilities,
        "Should have all capabilities in queue"
    );

    println!(
        "✅ Memory usage test: {} capabilities queued in {:?}",
        num_capabilities, elapsed
    );
    println!(
        "   📊 Queue stats: pending={}, in_progress={}, failed={}",
        stats.pending_count, stats.in_progress_count, stats.failed_count
    );
}

/// Helper function to create test capabilities
fn create_test_capability(id: &str) -> CapabilityManifest {
    CapabilityManifest {
        id: id.to_string(),
        name: format!("Test Capability {}", id),
        description: format!("A test capability for {}", id),
        input_schema: None,
        output_schema: None,
        provider: rtfs_compiler::ccos::capability_marketplace::types::ProviderType::Local(
            rtfs_compiler::ccos::capability_marketplace::types::LocalCapability {
                handler: Arc::new(|_args| Ok(Value::String("test".to_string()))),
            },
        ),
        version: "0.1.0".to_string(),
        provenance: None,
        attestation: None,
        permissions: vec![],
        effects: vec![],
        metadata: std::collections::HashMap::new(),
        agent_metadata: None,
    }
}
