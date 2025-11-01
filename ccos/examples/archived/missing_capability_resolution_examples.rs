//! Example scenarios demonstrating the missing capability resolution system
//!
//! This module provides real-world examples of how the missing capability
//! resolution system works in practice, from detection through discovery
//! to registration and auto-resume.

use ccos::{
    capabilities::registry::CapabilityRegistry,
    capability_marketplace::types::CapabilityMarketplace,
    checkpoint_archive::CheckpointArchive,
    synthesis::{
        capability_synthesizer::{CapabilitySynthesizer, SynthesisRequest},
        feature_flags::MissingCapabilityConfig,
        graphql_importer::GraphQLImporter,
        mcp_registry_client::McpRegistryClient,
        missing_capability_resolver::{MissingCapabilityResolver, ResolverConfig},
        openapi_importer::OpenAPIImporter,
        validation_harness::ValidationHarness,
    },
};
use rtfs::runtime::values::Value;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Example 1: Weather API Integration
///
/// This example demonstrates how the system discovers and integrates
/// a weather API capability that was missing from the marketplace.
fn example_weather_api_integration() {
    println!("🌤️  Example 1: Weather API Integration");
    println!("=====================================");

    // Setup the resolution system
    let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
    let marketplace = Arc::new(CapabilityMarketplace::new(registry));
    let checkpoint_archive = Arc::new(CheckpointArchive::new());
    let config = ResolverConfig {
        max_attempts: 3,
        auto_resolve: true,
        verbose_logging: true,
    };
    let feature_config = MissingCapabilityConfig::default();
    let resolver = Arc::new(MissingCapabilityResolver::new(
        marketplace.clone(),
        checkpoint_archive,
        config,
        feature_config,
    ));

    // Scenario: A user tries to use a weather capability that doesn't exist
    println!("📝 User attempts to use 'weather.current' capability...");

    let result = resolver.handle_missing_capability(
        "weather.current".to_string(),
        vec![Value::String("New York".to_string())],
        std::collections::HashMap::new(),
    );

    match result {
        Ok(_) => println!("✅ Missing capability queued for resolution"),
        Err(e) => println!("❌ Failed to queue capability: {}", e),
    }

    // Show queue status
    let queue_stats = resolver.get_stats();
    println!("📊 Queue Status:");
    println!("   ⏳ Pending: {}", queue_stats.pending_count);
    println!("   🔄 In Progress: {}", queue_stats.in_progress_count);
    println!("   ❌ Failed: {}", queue_stats.failed_count);

    println!();
}

/// Example 2: GitHub MCP Integration
///
/// This example shows how the system discovers GitHub MCP servers
/// and converts them to CCOS capabilities.
fn example_github_mcp_integration() {
    println!("🐙 Example 2: GitHub MCP Integration");
    println!("===================================");

    // Setup the resolution system
    let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
    let marketplace = Arc::new(CapabilityMarketplace::new(registry));
    let checkpoint_archive = Arc::new(CheckpointArchive::new());
    let config = ResolverConfig {
        max_attempts: 3,
        auto_resolve: true,
        verbose_logging: true,
    };
    let feature_config = MissingCapabilityConfig::default();
    let resolver = Arc::new(MissingCapabilityResolver::new(
        marketplace.clone(),
        checkpoint_archive,
        config,
        feature_config,
    ));

    // Create MCP Registry client
    let mcp_client = McpRegistryClient::new();

    println!("🔍 Searching MCP Registry for 'github' servers...");

    // Search for GitHub MCP servers
    let servers = mcp_client.search_servers("github");

    // Note: In a real async context, you would await this
    // For this example, we'll use a blocking approach
    let servers_result = futures::executor::block_on(servers);

    match servers_result {
        Ok(servers) => {
            println!("✅ Found {} GitHub MCP server(s)", servers.len());

            // Show first few servers
            for (i, server) in servers.iter().take(5).enumerate() {
                println!(
                    "   {}. {} - {}",
                    i + 1,
                    server.name,
                    server.description.chars().take(100).collect::<String>()
                );
            }

            // Convert first server to capability
            if let Some(server) = servers.first() {
                println!("🔧 Converting MCP server to capability...");
                let capability = mcp_client.convert_to_capability_manifest(server, "github");

                match capability {
                    Ok(cap) => {
                        println!("✅ Generated capability: {}", cap.id);
                        println!("   📋 Description: {}", cap.description);
                    }
                    Err(e) => println!("❌ Failed to convert: {}", e),
                }
            }
        }
        Err(e) => println!("❌ Failed to search MCP Registry: {}", e),
    }

    println!();
}

/// Example 3: Custom API with OpenAPI
///
/// This example demonstrates importing a custom API using OpenAPI specs.
fn example_custom_api_openapi() {
    println!("📋 Example 3: Custom API with OpenAPI");
    println!("====================================");

    // Setup the resolution system
    let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
    let marketplace = Arc::new(CapabilityMarketplace::new(registry));
    let checkpoint_archive = Arc::new(CheckpointArchive::new());
    let config = ResolverConfig {
        max_attempts: 3,
        auto_resolve: true,
        verbose_logging: true,
    };
    let feature_config = MissingCapabilityConfig::default();
    let resolver = Arc::new(MissingCapabilityResolver::new(
        marketplace.clone(),
        checkpoint_archive,
        config,
        feature_config,
    ));

    // Create OpenAPI importer
    let openapi_importer = OpenAPIImporter::new("https://api.example.com".to_string());

    println!("🔍 Loading OpenAPI specification...");

    // Load mock spec (in real scenario, this would fetch from URL)
    let mock_spec = serde_json::json!({
        "openapi": "3.0.0",
        "info": {
            "title": "Example API",
            "version": "1.0.0"
        },
        "paths": {
            "/users": {
                "get": {
                    "summary": "Get users",
                    "parameters": [
                        {
                            "name": "limit",
                            "in": "query",
                            "schema": {"type": "integer"}
                        }
                    ]
                }
            }
        }
    });

    // Extract operations
    let operations = openapi_importer.extract_operations(&mock_spec);

    match operations {
        Ok(ops) => {
            println!("   🔧 Extracted {} operations", ops.len());

            // Show operations
            for operation in &ops {
                println!(
                    "      • {} {} - {}",
                    operation.method,
                    operation.path,
                    operation.summary.as_deref().unwrap_or("No summary")
                );
            }

            // Convert first operation to capability
            if let Some(operation) = ops.first() {
                let capability = openapi_importer.operation_to_capability(operation, "example");

                match capability {
                    Ok(cap) => {
                        println!("         → Generated capability: {}", cap.id);
                    }
                    Err(e) => println!("         ❌ Failed to convert: {}", e),
                }
            }
        }
        Err(e) => println!("❌ Failed to extract operations: {}", e),
    }

    println!();
}

/// Example 4: GraphQL Integration
///
/// This example shows how to import GraphQL APIs.
fn example_graphql_integration() {
    println!("🔗 Example 4: GraphQL Integration");
    println!("================================");

    // Setup the resolution system
    let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
    let marketplace = Arc::new(CapabilityMarketplace::new(registry));
    let checkpoint_archive = Arc::new(CheckpointArchive::new());
    let config = ResolverConfig {
        max_attempts: 3,
        auto_resolve: true,
        verbose_logging: true,
    };
    let feature_config = MissingCapabilityConfig::default();
    let resolver = Arc::new(MissingCapabilityResolver::new(
        marketplace.clone(),
        checkpoint_archive,
        config,
        feature_config,
    ));

    // Create GraphQL importer
    let graphql_importer = GraphQLImporter::new("https://api.example.com/graphql".to_string());

    println!("🔍 Introspecting GraphQL schema...");

    // Mock schema (in real scenario, this would introspect the endpoint)
    let mock_schema = serde_json::json!({
        "data": {
            "__schema": {
                "queryType": {
                    "name": "Query"
                },
                "types": [
                    {
                        "name": "Query",
                        "fields": [
                            {
                                "name": "user",
                                "type": {"name": "User"},
                                "args": [
                                    {
                                        "name": "id",
                                        "type": {"name": "ID"}
                                    }
                                ]
                            }
                        ]
                    }
                ]
            }
        }
    });

    // Convert JSON to GraphQLSchema (simplified for example)
    let mock_graphql_schema = ccos::synthesis::graphql_importer::GraphQLSchema {
        schema_content: serde_json::Value::String("type Query { user(id: ID!): User }".to_string()),
        endpoint_url: "https://api.example.com/graphql".to_string(),
        introspection_result: Some(mock_schema),
    };

    // Extract operations
    let operations = graphql_importer.extract_operations(&mock_graphql_schema);

    match operations {
        Ok(ops) => {
            println!("   🔧 Extracted {} operations", ops.len());

            // Show operations
            for operation in &ops {
                println!(
                    "      • {} {} - {}",
                    operation.operation_type,
                    operation.name,
                    operation.description.as_deref().unwrap_or("No description")
                );
            }

            // Convert first operation to capability
            if let Some(operation) = ops.first() {
                let capability = graphql_importer.operation_to_capability(operation, "example");

                match capability {
                    Ok(cap) => {
                        println!("         → Generated capability: {}", cap.id);
                    }
                    Err(e) => println!("         ❌ Failed to convert: {}", e),
                }
            }
        }
        Err(e) => println!("❌ Failed to extract operations: {}", e),
    }

    println!();
}

/// Example 5: LLM Synthesis
///
/// This example demonstrates LLM-driven capability synthesis.
fn example_llm_synthesis() {
    println!("🤖 Example 5: LLM Synthesis");
    println!("===========================");

    // Setup the resolution system
    let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
    let marketplace = Arc::new(CapabilityMarketplace::new(registry));
    let checkpoint_archive = Arc::new(CheckpointArchive::new());
    let config = ResolverConfig {
        max_attempts: 3,
        auto_resolve: true,
        verbose_logging: true,
    };
    let feature_config = MissingCapabilityConfig::default();
    let resolver = Arc::new(MissingCapabilityResolver::new(
        marketplace.clone(),
        checkpoint_archive.clone(),
        config,
        feature_config,
    ));

    // Create capability synthesizer
    let synthesizer = CapabilitySynthesizer::new();

    println!("🔍 Synthesizing capability from description...");

    // Create synthesis request
    let request = SynthesisRequest {
        capability_name: "sentiment.analyze".to_string(),
        description: Some("Analyze sentiment of text input".to_string()),
        input_schema: Some(serde_json::json!({
            "type": "object",
            "properties": {
                "text": {"type": "string"}
            },
            "required": ["text"]
        })),
        output_schema: Some(serde_json::json!({
            "type": "object",
            "properties": {
                "sentiment": {"type": "string"},
                "confidence": {"type": "number"}
            }
        })),
        requires_auth: false,
        auth_provider: None,
        context: Some("Analyze sentiment of text input".to_string()),
    };

    // Synthesize capability (mock implementation)
    let capability = synthesizer.synthesize_capability(&request);

    // Note: In a real async context, you would await this
    let capability_result = futures::executor::block_on(capability);

    match capability_result {
        Ok(ref result) => {
            println!("✅ Synthesized capability successfully");
            println!("   📋 Description: {}", result.capability.description);
            println!("   🔧 RTFS Code: {}", result.implementation_code);
        }
        Err(ref e) => println!("❌ Failed to synthesize: {}", e),
    }

    // Validate the synthesized capability (only if synthesis succeeded)
    if let Ok(ref result) = capability_result {
        let validation_harness = ValidationHarness::new();
        let validation_result = validation_harness.validate_capability(
            &result.capability,
            "(defn analyze [text] {:sentiment \"positive\" :confidence 0.95})",
        );

        println!("🔍 Validation Results:");
        println!("   ✅ Status: {:?}", validation_result.status);
        println!("   📊 Issues: {}", validation_result.issues.len());
    } else {
        println!("🔍 Skipping validation due to synthesis failure");
    }

    println!();
}

/// Example 6: Complete Workflow with Auto-Resume
///
/// This example demonstrates the complete workflow including
/// checkpoint creation and auto-resume functionality.
fn example_complete_workflow_with_auto_resume() {
    println!("🔄 Example 6: Complete Workflow with Auto-Resume");
    println!("===============================================");

    // Setup the resolution system
    let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
    let marketplace = Arc::new(CapabilityMarketplace::new(registry));
    let checkpoint_archive = Arc::new(CheckpointArchive::new());
    let config = ResolverConfig {
        max_attempts: 3,
        auto_resolve: true,
        verbose_logging: true,
    };
    let feature_config = MissingCapabilityConfig::default();
    let resolver = Arc::new(MissingCapabilityResolver::new(
        marketplace.clone(),
        checkpoint_archive.clone(),
        config,
        feature_config,
    ));

    println!("📝 Simulating plan execution with missing capabilities...");

    // Simulate multiple missing capabilities
    let missing_capabilities = vec![
        "external.api.weather".to_string(),
        "external.api.maps".to_string(),
        "external.api.translation".to_string(),
    ];

    for capability_id in &missing_capabilities {
        let result = resolver.handle_missing_capability(
            capability_id.clone(),
            vec![Value::String("test".to_string())],
            std::collections::HashMap::new(),
        );

        match result {
            Ok(_) => println!("   ✅ Queued: {}", capability_id),
            Err(e) => println!("   ❌ Failed to queue {}: {}", capability_id, e),
        }
    }

    // Show final queue status
    let queue_stats = resolver.get_stats();
    println!("📊 Final Queue Status:");
    println!("   ⏳ Pending: {}", queue_stats.pending_count);
    println!("   🔄 In Progress: {}", queue_stats.in_progress_count);
    println!("   ❌ Failed: {}", queue_stats.failed_count);

    // Show checkpoints waiting for capabilities
    let pending_checkpoints = checkpoint_archive.get_pending_auto_resume_checkpoints();
    println!(
        "📋 Checkpoints waiting for resolution: {}",
        pending_checkpoints.len()
    );

    println!();
}

/// Main function to run all examples
#[tokio::main]
async fn main() {
    println!("🚀 Missing Capability Resolution Examples");
    println!("==========================================");
    println!();

    // Run all examples
    example_weather_api_integration();
    example_github_mcp_integration();
    example_custom_api_openapi();
    example_graphql_integration();
    example_llm_synthesis();
    example_complete_workflow_with_auto_resume();

    println!("🎉 All examples completed successfully!");
    println!();
    println!("💡 Key Takeaways:");
    println!("   • Missing capabilities are automatically detected and queued");
    println!("   • MCP Registry provides server discovery capabilities");
    println!("   • OpenAPI and GraphQL specs can be imported as capabilities");
    println!("   • LLM synthesis can generate capabilities from descriptions");
    println!("   • The system supports checkpoint/resume for long-running workflows");
    println!("   • Auto-resume functionality restarts execution when capabilities are resolved");
}
