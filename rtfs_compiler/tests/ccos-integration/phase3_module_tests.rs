//! Unit tests for Phase 3 modules (Importers, Wrappers, Synthesis)
//! 
//! This module provides comprehensive test coverage for all the Phase 3
//! components that handle capability discovery and generation.

use rtfs_compiler::ccos::synthesis::{
    auth_injector::{AuthInjector, AuthType, AuthConfig},
    openapi_importer::{OpenAPIImporter, OpenAPIOperation, OpenAPIParameter},
    graphql_importer::{GraphQLImporter, GraphQLOperation, GraphQLArgument},
    http_wrapper::{HTTPWrapper, HTTPEndpoint, HTTPAPIInfo},
    mcp_proxy_adapter::{MCPProxyAdapter, MCPProxyConfig, MCPToolProxy},
    capability_synthesizer::{CapabilitySynthesizer, SynthesisRequest},
    web_search_discovery::{WebSearchDiscovery, WebSearchResult},
    validation_harness::ValidationHarness,
    governance_policies::{GovernancePolicy, MaxParameterCountPolicy, EnterpriseSecurityPolicy},
    static_analyzers::{StaticAnalyzer, PerformanceAnalyzer, SecurityAnalyzer},
    registration_flow::RegistrationFlow,
    continuous_resolution::{ContinuousResolutionLoop, ResolutionMethod, ResolutionPriority},
};
use rtfs_compiler::ccos::{
    capability_marketplace::{marketplace::CapabilityMarketplace, types::CapabilityManifest},
    capabilities::registry::CapabilityRegistry,
    checkpoint_archive::CheckpointArchive,
    synthesis::missing_capability_resolver::{MissingCapabilityResolver, ResolutionConfig},
    runtime::values::Value,
};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Test AuthInjector functionality
#[tokio::test]
async fn test_auth_injector() {
    let injector = AuthInjector::new();
    
    // Test Bearer token auth
    let bearer_config = AuthConfig {
        auth_type: AuthType::Bearer,
        token: Some("test_token".to_string()),
        api_key: None,
        username: None,
        password: None,
        custom_headers: std::collections::HashMap::new(),
    };
    
    injector.register_auth_config("test.bearer", bearer_config);
    
    let injection_code = injector.generate_auth_injection_code("test.bearer");
    assert!(injection_code.contains("Bearer"), "Should generate Bearer auth code");
    
    // Test API key auth
    let api_key_config = AuthConfig {
        auth_type: AuthType::ApiKey,
        token: None,
        api_key: Some(("X-API-Key".to_string(), "test_key".to_string())),
        username: None,
        password: None,
        custom_headers: std::collections::HashMap::new(),
    };
    
    injector.register_auth_config("test.apikey", api_key_config);
    
    let injection_code = injector.generate_auth_injection_code("test.apikey");
    assert!(injection_code.contains("X-API-Key"), "Should generate API key auth code");
    
    // Test Basic auth
    let basic_config = AuthConfig {
        auth_type: AuthType::Basic,
        token: None,
        api_key: None,
        username: Some("testuser".to_string()),
        password: Some("testpass".to_string()),
        custom_headers: std::collections::HashMap::new(),
    };
    
    injector.register_auth_config("test.basic", basic_config);
    
    let injection_code = injector.generate_auth_injection_code("test.basic");
    assert!(injection_code.contains("Basic"), "Should generate Basic auth code");
}

/// Test OpenAPI importer with various operation types
#[tokio::test]
async fn test_openapi_importer_comprehensive() {
    let importer = OpenAPIImporter::new();
    
    // Test GET operation
    let get_operation = OpenAPIOperation {
        method: "GET".to_string(),
        path: "/api/users/{id}".to_string(),
        summary: Some("Get user by ID".to_string()),
        description: Some("Retrieves a user by their ID".to_string()),
        parameters: vec![
            OpenAPIParameter {
                name: "id".to_string(),
                param_type: "string".to_string(),
                required: true,
                description: Some("User ID".to_string()),
            }
        ],
        security: vec!["BearerAuth".to_string()],
    };
    
    let capability = importer.operation_to_capability(&get_operation);
    assert_eq!(capability.id, "api.users.get", "Should generate correct capability ID");
    assert!(capability.input_schema.is_some(), "Should have input schema");
    
    // Test POST operation
    let post_operation = OpenAPIOperation {
        method: "POST".to_string(),
        path: "/api/users".to_string(),
        summary: Some("Create user".to_string()),
        description: Some("Creates a new user".to_string()),
        parameters: vec![
            OpenAPIParameter {
                name: "user".to_string(),
                param_type: "object".to_string(),
                required: true,
                description: Some("User data".to_string()),
            }
        ],
        security: vec!["BearerAuth".to_string()],
    };
    
    let capability = importer.operation_to_capability(&post_operation);
    assert_eq!(capability.id, "api.users.post", "Should generate correct capability ID");
    
    // Test type conversion
    let rtfs_type = importer.json_schema_to_rtfs_type("string");
    assert_eq!(rtfs_type, ":string", "Should convert string to RTFS type");
    
    let rtfs_type = importer.json_schema_to_rtfs_type("integer");
    assert_eq!(rtfs_type, ":number", "Should convert integer to RTFS type");
    
    let rtfs_type = importer.json_schema_to_rtfs_type("boolean");
    assert_eq!(rtfs_type, ":boolean", "Should convert boolean to RTFS type");
}

/// Test GraphQL importer with various operation types
#[tokio::test]
async fn test_graphql_importer_comprehensive() {
    let importer = GraphQLImporter::new();
    
    // Test query operation
    let query_operation = GraphQLOperation {
        name: "GetUser".to_string(),
        operation_type: "query".to_string(),
        description: Some("Get user by ID".to_string()),
        arguments: vec![
            GraphQLArgument {
                name: "id".to_string(),
                arg_type: "ID!".to_string(),
                description: Some("User ID".to_string()),
            }
        ],
        return_type: "User".to_string(),
    };
    
    let capability = importer.operation_to_capability(&query_operation);
    assert_eq!(capability.id, "graphql.getuser.query", "Should generate correct capability ID");
    assert!(capability.input_schema.is_some(), "Should have input schema");
    
    // Test mutation operation
    let mutation_operation = GraphQLOperation {
        name: "CreateUser".to_string(),
        operation_type: "mutation".to_string(),
        description: Some("Create a new user".to_string()),
        arguments: vec![
            GraphQLArgument {
                name: "input".to_string(),
                arg_type: "UserInput!".to_string(),
                description: Some("User input data".to_string()),
            }
        ],
        return_type: "User".to_string(),
    };
    
    let capability = importer.operation_to_capability(&mutation_operation);
    assert_eq!(capability.id, "graphql.createuser.mutation", "Should generate correct capability ID");
    
    // Test type conversion
    let rtfs_type = importer.graphql_type_to_rtfs_type("String");
    assert_eq!(rtfs_type, ":string", "Should convert String to RTFS type");
    
    let rtfs_type = importer.graphql_type_to_rtfs_type("Int");
    assert_eq!(rtfs_type, ":number", "Should convert Int to RTFS type");
    
    let rtfs_type = importer.graphql_type_to_rtfs_type("Boolean");
    assert_eq!(rtfs_type, ":boolean", "Should convert Boolean to RTFS type");
}

/// Test HTTP wrapper with various endpoint patterns
#[tokio::test]
async fn test_http_wrapper_comprehensive() {
    let wrapper = HTTPWrapper::new();
    
    // Test REST endpoint
    let rest_endpoint = HTTPEndpoint {
        method: "GET".to_string(),
        path: "/api/v1/users/{userId}/posts/{postId}".to_string(),
        description: Some("Get a specific post by user".to_string()),
        parameters: vec!["userId".to_string(), "postId".to_string()],
        query_params: vec!["include".to_string(), "limit".to_string()],
    };
    
    let capability = wrapper.endpoint_to_capability(&rest_endpoint);
    assert!(capability.id.contains("users") && capability.id.contains("posts"), 
            "Should generate descriptive capability ID");
    assert!(capability.input_schema.is_some(), "Should have input schema");
    
    // Test parameter extraction
    let path_params = wrapper.extract_parameters_from_path("/api/{resource}/{id}");
    assert_eq!(path_params, vec!["resource", "id"], "Should extract path parameters");
    
    // Test type inference
    let types = wrapper.infer_parameter_types(&["userId", "limit", "include"]);
    assert_eq!(types.len(), 3, "Should infer types for all parameters");
}

/// Test MCP proxy adapter
#[tokio::test]
async fn test_mcp_proxy_adapter() {
    let adapter = MCPProxyAdapter::new();
    
    // Test tool discovery and proxying
    let tools = adapter.get_mock_tools();
    assert!(!tools.is_empty(), "Should have mock tools");
    
    if let Some(tool) = tools.first() {
        let proxy = adapter.create_tool_proxy(tool);
        assert!(!proxy.capability_id.is_empty(), "Should create proxy with capability ID");
        
        let capability = adapter.tool_to_capability_manifest(tool);
        assert!(!capability.id.is_empty(), "Should convert tool to capability");
        
        let rtfs_code = adapter.generate_rtfs_implementation(&proxy);
        assert!(rtfs_code.contains("call"), "Should generate RTFS call code");
    }
}

/// Test capability synthesizer with various scenarios
#[tokio::test]
async fn test_capability_synthesizer_comprehensive() {
    let synthesizer = CapabilitySynthesizer::new();
    
    // Test synthesis request
    let request = SynthesisRequest {
        capability_name: "weather.current".to_string(),
        description: "Get current weather for a location".to_string(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "location": {"type": "string"},
                "units": {"type": "string", "enum": ["celsius", "fahrenheit"]}
            },
            "required": ["location"]
        }),
        output_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "temperature": {"type": "number"},
                "condition": {"type": "string"}
            }
        }),
        context: "Weather API integration".to_string(),
    };
    
    // Test mock synthesis (since we don't have real LLM integration)
    let capability = synthesizer.generate_mock_capability("weather.current");
    assert_eq!(capability.id, "weather.current", "Should generate correct capability ID");
    assert!(capability.input_schema.is_some(), "Should have input schema");
    assert!(capability.output_schema.is_some(), "Should have output schema");
    
    // Test quality score calculation
    let score = synthesizer.calculate_quality_score(&capability);
    assert!(score >= 0.0 && score <= 1.0, "Quality score should be between 0 and 1");
    
    // Test static analysis
    let issues = synthesizer.run_static_analysis("(defn weather [location] {:temp 20 :condition \"sunny\"})");
    // Should have no issues for simple, safe code
    assert!(issues.is_empty(), "Simple code should have no static analysis issues");
}

/// Test web search discovery
#[tokio::test]
async fn test_web_search_discovery_comprehensive() {
    let discovery = WebSearchDiscovery::new();
    
    // Test search query building
    let results = discovery.search_for_api_specs("weather api documentation").await;
    assert!(results.is_ok(), "Search should succeed");
    
    let results = results.unwrap();
    assert!(!results.is_empty(), "Should return search results");
    
    // Test relevance scoring
    if let Some(result) = results.first() {
        assert!(result.relevance_score >= 0.0 && result.relevance_score <= 1.0,
                "Relevance score should be between 0 and 1");
        
        // Test different result types
        assert!(matches!(result.result_type, 
                        rtfs_compiler::ccos::synthesis::web_search_discovery::SearchResultType::ApiSpec |
                        rtfs_compiler::ccos::synthesis::web_search_discovery::SearchResultType::Documentation |
                        rtfs_compiler::ccos::synthesis::web_search_discovery::SearchResultType::GitHubRepo |
                        rtfs_compiler::ccos::synthesis::web_search_discovery::SearchResultType::Other),
                "Should have valid result type");
    }
    
    // Test result formatting
    let formatted = discovery.format_results_for_display(&results);
    assert!(!formatted.is_empty(), "Should format results for display");
}

/// Test validation harness with various capability types
#[tokio::test]
async fn test_validation_harness_comprehensive() {
    let harness = ValidationHarness::new();
    
    // Test well-formed capability
    let good_capability = create_test_capability();
    let result = harness.validate_capability(&good_capability, Some("(defn test [x] x)"));
    assert!(result.is_ok(), "Good capability should validate successfully");
    
    let validation_result = result.unwrap();
    assert!(validation_result.issues.is_empty(), "Good capability should have no issues");
    
    // Test capability with missing documentation
    let mut bad_capability = good_capability.clone();
    bad_capability.description = None;
    let result = harness.validate_capability(&bad_capability, Some("(defn test [x] x)"));
    assert!(result.is_ok(), "Validation should succeed even with issues");
    
    let validation_result = result.unwrap();
    // Should have issues for missing documentation
    assert!(!validation_result.issues.is_empty(), "Should have validation issues");
    
    // Test attestation creation
    let attestation = harness.create_attestation(&good_capability);
    assert!(!attestation.attestation_id.is_empty(), "Should create attestation with ID");
    
    // Test provenance creation
    let provenance = harness.create_provenance(&validation_result);
    assert!(provenance.version.is_some(), "Should have version in provenance");
}

/// Test governance policies with various scenarios
#[tokio::test]
async fn test_governance_policies_comprehensive() {
    // Test parameter count policy
    let param_policy = MaxParameterCountPolicy::new(3);
    let capability = create_capability_with_many_params();
    
    let result = param_policy.check_compliance(&capability, "(defn test [a b c d e] e)");
    assert!(!result.issues.is_empty(), "Should have issues for too many parameters");
    
    // Test enterprise security policy
    let security_policy = EnterpriseSecurityPolicy::new();
    let result = security_policy.check_compliance(&create_test_capability(), "(defn test [x] x)");
    assert!(result.is_ok(), "Security policy check should succeed");
    
    // Test policy metadata
    assert_eq!(param_policy.policy_name(), "MaxParameterCountPolicy");
    assert!(!param_policy.policy_description().is_empty());
}

/// Test static analyzers with various code patterns
#[tokio::test]
async fn test_static_analyzers_comprehensive() {
    // Test performance analyzer
    let perf_analyzer = PerformanceAnalyzer::new();
    
    let simple_code = "(defn simple [x] x)";
    let issues = perf_analyzer.analyze(&create_test_capability(), simple_code);
    assert!(issues.is_empty(), "Simple code should have no performance issues");
    
    let complex_code = "(defn complex [x] (if x (if x (if x (if x (if x x x) x) x) x) x))";
    let issues = perf_analyzer.analyze(&create_test_capability(), complex_code);
    // May have issues depending on nesting depth threshold
    
    // Test security analyzer
    let security_analyzer = SecurityAnalyzer::new();
    let safe_code = "(defn safe [x] x)";
    let issues = security_analyzer.analyze(&create_test_capability(), safe_code);
    assert!(issues.is_empty(), "Safe code should have no security issues");
    
    // Test analyzer metadata
    assert_eq!(perf_analyzer.name(), "PerformanceAnalyzer");
    assert!(!perf_analyzer.description().is_empty());
}

/// Test registration flow with various scenarios
#[tokio::test]
async fn test_registration_flow_comprehensive() {
    let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
    let marketplace = Arc::new(CapabilityMarketplace::new(registry));
    let registration_flow = RegistrationFlow::new(marketplace);
    
    // Test successful registration
    let capability = create_test_capability();
    let rtfs_code = "(defn test [x] x)";
    
    let result = registration_flow.register_capability(&capability, rtfs_code).await;
    assert!(result.is_ok(), "Registration should succeed");
    
    let registration_result = result.unwrap();
    assert!(registration_result.success, "Registration should be successful");
    assert!(!registration_result.capability_id.is_empty(), "Should return capability ID");
    
    // Test registration with validation issues
    let bad_capability = create_capability_with_many_params();
    let result = registration_flow.register_capability(&bad_capability, rtfs_code).await;
    // Should either succeed with warnings or fail gracefully
    assert!(result.is_ok(), "Registration should handle issues gracefully");
}

/// Test continuous resolution loop
#[tokio::test]
async fn test_continuous_resolution_loop() {
    let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
    let marketplace = Arc::new(CapabilityMarketplace::new(registry));
    let checkpoint_archive = Arc::new(CheckpointArchive::new());
    let config = ResolutionConfig {
        auto_resolve: true,
        verbose_logging: false,
    };
    let resolver = Arc::new(MissingCapabilityResolver::new(marketplace, checkpoint_archive, config));
    
    let loop_controller = ContinuousResolutionLoop::new(resolver);
    
    // Test processing with no pending items
    let result = loop_controller.process_pending_resolutions().await;
    assert!(result.is_ok(), "Should handle empty queue gracefully");
    
    // Test resolution statistics
    let stats = loop_controller.get_resolution_stats();
    assert!(stats.total_attempts >= 0, "Should have non-negative total attempts");
    
    // Test risk assessment
    let risk = loop_controller.assess_risk("test.capability", Some("test context")).await;
    assert!(risk.is_ok(), "Risk assessment should succeed");
    
    let risk_assessment = risk.unwrap();
    assert!(matches!(risk_assessment.priority, ResolutionPriority::Low | ResolutionPriority::Medium | ResolutionPriority::High),
            "Should have valid priority");
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

/// Helper function to create a capability with many parameters
fn create_capability_with_many_params() -> CapabilityManifest {
    CapabilityManifest {
        id: "test.many.params".to_string(),
        name: "Test Many Params".to_string(),
        description: Some("A test capability with many parameters".to_string()),
        input_schema: Some(serde_json::json!({
            "type": "object",
            "properties": {
                "param1": {"type": "string"},
                "param2": {"type": "string"},
                "param3": {"type": "string"},
                "param4": {"type": "string"},
                "param5": {"type": "string"}
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
            ("parameter_count".to_string(), "5".to_string())
        ]),
        agent_metadata: None,
    }
}

