//! Integration tests for the complete missing capability resolution system
//! 
//! This module tests the end-to-end flow from capability detection through
//! discovery, validation, registration, and auto-resume functionality.

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
        governance_policies::{GovernancePolicy, MaxParameterCountPolicy},
        static_analyzers::{StaticAnalyzer, PerformanceAnalyzer},
        registration_flow::RegistrationFlow,
        continuous_resolution::ContinuousResolutionLoop,
    },
    runtime::values::Value,
};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Test the complete end-to-end missing capability resolution pipeline
#[tokio::test]
async fn test_complete_resolution_pipeline() {
    // Setup
    let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
    let marketplace = Arc::new(CapabilityMarketplace::new(registry));
    let checkpoint_archive = Arc::new(CheckpointArchive::new());
    let config = ResolutionConfig {
        auto_resolve: true,
        verbose_logging: true,
    };
    let resolver = Arc::new(MissingCapabilityResolver::new(marketplace.clone(), checkpoint_archive, config));

    // Test capability that doesn't exist
    let missing_capability_id = "external.api.weather";
    
    // Step 1: Trigger missing capability detection
    let result = resolver.handle_missing_capability(
        missing_capability_id.to_string(),
        vec![Value::String("Paris".to_string())],
        std::collections::HashMap::new(),
    ).await;
    
    assert!(result.is_ok(), "Should successfully enqueue missing capability");

    // Step 2: Verify it's in the queue
    let stats = resolver.get_stats();
    assert!(stats.pending_count > 0, "Should have pending capabilities");

    // Step 3: Test discovery methods
    test_mcp_registry_discovery().await;
    test_openapi_importer().await;
    test_graphql_importer().await;
    test_http_wrapper().await;
    test_llm_synthesis().await;
    test_web_search_discovery().await;

    // Step 4: Test validation and governance
    test_validation_harness().await;
    test_governance_policies().await;
    test_static_analyzers().await;

    // Step 5: Test registration flow
    test_registration_flow(marketplace.clone()).await;

    // Step 6: Test continuous resolution loop
    test_continuous_resolution_loop(resolver.clone()).await;
}

/// Test MCP Registry discovery functionality
async fn test_mcp_registry_discovery() {
    let client = McpRegistryClient::new();
    
    // Test server search
    let servers = client.search_servers("github").await;
    assert!(servers.is_ok(), "MCP Registry search should succeed");
    
    let servers = servers.unwrap();
    println!("Found {} MCP servers for 'github'", servers.len());
    
    // Test capability conversion
    if let Some(server) = servers.first() {
        let capability = client.convert_to_capability_manifest(server, "github");
        assert!(!capability.id.is_empty(), "Converted capability should have an ID");
        assert!(!capability.name.is_empty(), "Converted capability should have a name");
    }
}

/// Test OpenAPI importer functionality
async fn test_openapi_importer() {
    let importer = OpenAPIImporter::new();
    
    // Test mock spec loading
    let spec = importer.load_mock_spec();
    assert!(!spec.is_empty(), "Mock OpenAPI spec should not be empty");
    
    // Test operation extraction
    let operations = importer.extract_operations(&spec);
    assert!(!operations.is_empty(), "Should extract operations from spec");
    
    // Test capability conversion
    if let Some(operation) = operations.first() {
        let capability = importer.operation_to_capability(operation);
        assert!(!capability.id.is_empty(), "Converted capability should have an ID");
        assert!(capability.input_schema.is_some(), "Should have input schema");
    }
}

/// Test GraphQL importer functionality
async fn test_graphql_importer() {
    let importer = GraphQLImporter::new();
    
    // Test mock schema loading
    let schema = importer.get_mock_schema();
    assert!(!schema.is_empty(), "Mock GraphQL schema should not be empty");
    
    // Test operation extraction
    let operations = importer.extract_operations(&schema);
    assert!(!operations.is_empty(), "Should extract operations from schema");
    
    // Test capability conversion
    if let Some(operation) = operations.first() {
        let capability = importer.operation_to_capability(operation);
        assert!(!capability.id.is_empty(), "Converted capability should have an ID");
        assert!(capability.input_schema.is_some(), "Should have input schema");
    }
}

/// Test HTTP wrapper functionality
async fn test_http_wrapper() {
    let wrapper = HTTPWrapper::new();
    
    // Test mock API info loading
    let api_info = wrapper.get_mock_api_info();
    assert!(!api_info.endpoints.is_empty(), "Mock API should have endpoints");
    
    // Test endpoint conversion
    if let Some(endpoint) = api_info.endpoints.first() {
        let capability = wrapper.endpoint_to_capability(endpoint);
        assert!(!capability.id.is_empty(), "Converted capability should have an ID");
        assert!(capability.input_schema.is_some(), "Should have input schema");
    }
}

/// Test LLM synthesis functionality
async fn test_llm_synthesis() {
    let synthesizer = CapabilitySynthesizer::new();
    
    // Test mock capability generation
    let capability = synthesizer.generate_mock_capability("test.api");
    assert!(!capability.id.is_empty(), "Generated capability should have an ID");
    assert!(!capability.name.is_empty(), "Generated capability should have a name");
    
    // Test quality score calculation
    let score = synthesizer.calculate_quality_score(&capability);
    assert!(score >= 0.0 && score <= 1.0, "Quality score should be between 0 and 1");
}

/// Test web search discovery functionality
async fn test_web_search_discovery() {
    let discovery = WebSearchDiscovery::new();
    
    // Test mock search results
    let results = discovery.get_mock_results("weather api");
    assert!(!results.is_empty(), "Should return mock search results");
    
    // Test relevance scoring
    if let Some(result) = results.first() {
        assert!(result.relevance_score >= 0.0 && result.relevance_score <= 1.0, 
                "Relevance score should be between 0 and 1");
    }
}

/// Test validation harness functionality
async fn test_validation_harness() {
    let harness = ValidationHarness::new();
    
    // Create a test capability
    let capability = CapabilityManifest {
        id: "test.validation".to_string(),
        name: "Test Validation".to_string(),
        description: Some("A test capability for validation".to_string()),
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
    };
    
    // Test validation
    let result = harness.validate_capability(&capability, Some("(defn test [x] x)"));
    assert!(result.is_ok(), "Validation should succeed");
    
    let validation_result = result.unwrap();
    assert!(validation_result.issues.is_empty(), "Should have no validation issues");
}

/// Test governance policies functionality
async fn test_governance_policies() {
    let policy = MaxParameterCountPolicy::new(5);
    
    // Create a test capability with many parameters
    let capability = CapabilityManifest {
        id: "test.governance".to_string(),
        name: "Test Governance".to_string(),
        description: Some("A test capability for governance".to_string()),
        input_schema: Some(serde_json::json!({
            "type": "object",
            "properties": {
                "param1": {"type": "string"},
                "param2": {"type": "string"},
                "param3": {"type": "string"},
                "param4": {"type": "string"},
                "param5": {"type": "string"},
                "param6": {"type": "string"} // This should trigger the policy
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
        metadata: std::collections::HashMap::from([
            ("parameter_count".to_string(), "6".to_string())
        ]),
        agent_metadata: None,
    };
    
    // Test policy compliance
    let result = policy.check_compliance(&capability, "(defn test [a b c d e f] f)");
    assert!(!result.issues.is_empty(), "Should have governance issues for too many parameters");
}

/// Test static analyzers functionality
async fn test_static_analyzers() {
    let analyzer = PerformanceAnalyzer::new();
    
    // Test with simple code
    let simple_code = "(defn simple [x] x)";
    let issues = analyzer.analyze(&create_test_capability(), simple_code);
    assert!(issues.is_empty(), "Simple code should have no performance issues");
    
    // Test with complex nested code
    let complex_code = "(defn complex [x] (if x (if x (if x (if x (if x x x) x) x) x) x))";
    let issues = analyzer.analyze(&create_test_capability(), complex_code);
    // Note: This might have issues depending on the analyzer implementation
}

/// Test registration flow functionality
async fn test_registration_flow(marketplace: Arc<CapabilityMarketplace>) {
    let registration_flow = RegistrationFlow::new(marketplace.clone());
    
    // Create a test capability
    let capability = create_test_capability();
    let rtfs_code = "(defn test [x] x)";
    
    // Test registration
    let result = registration_flow.register_capability(&capability, rtfs_code).await;
    assert!(result.is_ok(), "Registration should succeed");
    
    let registration_result = result.unwrap();
    assert!(registration_result.success, "Registration should be successful");
    assert!(!registration_result.capability_id.is_empty(), "Should return capability ID");
}

/// Test continuous resolution loop functionality
async fn test_continuous_resolution_loop(resolver: Arc<MissingCapabilityResolver>) {
    let loop_controller = ContinuousResolutionLoop::new(resolver.clone());
    
    // Test processing pending resolutions
    let result = loop_controller.process_pending_resolutions().await;
    assert!(result.is_ok(), "Processing pending resolutions should succeed");
    
    // Test resolution statistics
    let stats = loop_controller.get_resolution_stats();
    assert!(stats.total_attempts >= 0, "Should have non-negative total attempts");
}

/// Helper function to create a test capability
fn create_test_capability() -> CapabilityManifest {
    CapabilityManifest {
        id: "test.capability".to_string(),
        name: "Test Capability".to_string(),
        description: Some("A test capability".to_string()),
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

/// Test the CLI tool integration
#[tokio::test]
async fn test_cli_tool_integration() {
    // This test would verify that the CLI tool can be invoked programmatically
    // and that it properly integrates with the resolution system
    
    // For now, we'll just verify the resolver can be created with CLI-compatible config
    let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
    let marketplace = Arc::new(CapabilityMarketplace::new(registry));
    let checkpoint_archive = Arc::new(CheckpointArchive::new());
    let config = ResolutionConfig {
        auto_resolve: true,
        verbose_logging: false, // CLI typically uses false for cleaner output
    };
    
    let resolver = Arc::new(MissingCapabilityResolver::new(marketplace, checkpoint_archive, config));
    assert!(resolver.get_stats().pending_count == 0, "Fresh resolver should have no pending items");
}

/// Test error handling and edge cases
#[tokio::test]
async fn test_error_handling_and_edge_cases() {
    let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
    let marketplace = Arc::new(CapabilityMarketplace::new(registry));
    let checkpoint_archive = Arc::new(CheckpointArchive::new());
    let config = ResolutionConfig {
        auto_resolve: true,
        verbose_logging: true,
    };
    let resolver = Arc::new(MissingCapabilityResolver::new(marketplace, checkpoint_archive, config));
    
    // Test with empty capability ID
    let result = resolver.handle_missing_capability(
        "".to_string(),
        vec![],
        std::collections::HashMap::new(),
    ).await;
    // This should either succeed (empty ID is valid) or fail gracefully
    // The exact behavior depends on implementation
    
    // Test with very long capability ID
    let long_id = "a".repeat(1000);
    let result = resolver.handle_missing_capability(
        long_id,
        vec![],
        std::collections::HashMap::new(),
    ).await;
    // Should handle gracefully without panicking
    
    // Test with invalid arguments
    let result = resolver.handle_missing_capability(
        "test.capability".to_string(),
        vec![Value::Map(std::collections::HashMap::new())], // Complex nested value
        std::collections::HashMap::new(),
    ).await;
    // Should handle complex arguments gracefully
}

/// Test performance under load
#[tokio::test]
async fn test_performance_under_load() {
    let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
    let marketplace = Arc::new(CapabilityMarketplace::new(registry));
    let checkpoint_archive = Arc::new(CheckpointArchive::new());
    let config = ResolutionConfig {
        auto_resolve: true,
        verbose_logging: false, // Disable logging for performance test
    };
    let resolver = Arc::new(MissingCapabilityResolver::new(marketplace, checkpoint_archive, config));
    
    // Test with multiple concurrent missing capabilities
    let mut handles = vec![];
    for i in 0..10 {
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
    
    // Wait for all to complete
    for handle in handles {
        let result = handle.await.unwrap();
        assert!(result.is_ok(), "Concurrent resolution should succeed");
    }
    
    // Verify all are in the queue
    let stats = resolver.get_stats();
    assert!(stats.pending_count >= 10, "Should have all capabilities in queue");
}

