//! Test the Unified MCP Discovery Service
//!
//! This example demonstrates how to use the unified MCP discovery service
//! to discover tools from MCP servers, convert them to capability manifests,
//! and register them in the marketplace and catalog.
//!
//! Usage:
//!   cargo run --example test_unified_mcp_discovery
//!
//! Environment variables:
//!   - MCP_AUTH_TOKEN: Optional auth token for MCP servers
//!   - GITHUB_MCP_ENDPOINT: Optional GitHub MCP server endpoint

use ccos::capability_marketplace::CapabilityMarketplace;
use ccos::capability_marketplace::mcp_discovery::MCPServerConfig;
use ccos::catalog::CatalogService;
use ccos::mcp::core::MCPDiscoveryService;
use ccos::mcp::types::DiscoveryOptions;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🔍 Testing Unified MCP Discovery Service\n");
    println!("{}", "═".repeat(80));

    // Step 1: Create the unified discovery service
    println!("\n📦 Step 1: Creating Unified MCP Discovery Service");
    println!("{}", "─".repeat(80));
    
    let unified_service = Arc::new(MCPDiscoveryService::new());
    println!("✅ Unified service created");

    // Step 2: Test listing known servers
    println!("\n📋 Step 2: Listing Known Servers");
    println!("{}", "─".repeat(80));
    
    let servers = unified_service.list_known_servers();
    println!("Found {} configured servers:", servers.len());
    for server in &servers {
        println!("  - {} ({})", server.name, server.endpoint);
    }

    if servers.is_empty() {
        println!("\n⚠️  No servers configured. You can:");
        println!("  1. Add servers to config/overrides.json");
        println!("  2. Set environment variables like GITHUB_MCP_ENDPOINT");
        println!("  3. Use a test server endpoint below");
        
        // Use a test server if available
        let test_config = MCPServerConfig {
            name: "test-server".to_string(),
            endpoint: std::env::var("GITHUB_MCP_ENDPOINT")
                .unwrap_or_else(|_| "https://api.github.com".to_string()),
            auth_token: std::env::var("MCP_AUTH_TOKEN").ok(),
            timeout_seconds: 30,
            protocol_version: "2024-11-05".to_string(),
        };
        
        test_discovery_with_config(&unified_service, &test_config).await?;
        return Ok(());
    }

    // Step 3: Test discovery for each server
    println!("\n🔍 Step 3: Testing Tool Discovery");
    println!("{}", "─".repeat(80));
    
    for server_config in servers {
        println!("\n  Testing server: {} ({})", server_config.name, server_config.endpoint);
        test_discovery_with_config(&unified_service, &server_config).await?;
    }

    // Step 4: Test with marketplace and catalog
    println!("\n🏪 Step 4: Testing with Marketplace and Catalog");
    println!("{}", "─".repeat(80));
    
    test_with_marketplace_and_catalog().await?;

    println!("\n{}", "═".repeat(80));
    println!("✅ All tests completed successfully!");
    
    Ok(())
}

async fn test_discovery_with_config(
    unified_service: &Arc<MCPDiscoveryService>,
    server_config: &MCPServerConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    let options = DiscoveryOptions {
        introspect_output_schemas: false, // Set to true to introspect output schemas (requires auth)
        use_cache: true,
        register_in_marketplace: false,
        export_to_rtfs: false, // Can enable to auto-export discovered capabilities
        export_directory: None,
        auth_headers: server_config.auth_token.as_ref().map(|token| {
            let mut headers = std::collections::HashMap::new();
            headers.insert("Authorization".to_string(), format!("Bearer {}", token));
            headers
        }),
    };

    println!("    🔍 Discovering tools...");
    match unified_service.discover_tools(server_config, &options).await {
        Ok(tools) => {
            println!("    ✅ Discovered {} tools:", tools.len());
            for tool in &tools {
                println!("      - {}: {}", tool.tool_name, 
                    tool.description.as_ref().unwrap_or(&"No description".to_string()));
            }

            // Test tool-to-manifest conversion
            if !tools.is_empty() {
                println!("    🔄 Converting first tool to manifest...");
                let manifest = unified_service.tool_to_manifest(&tools[0], server_config);
                println!("    ✅ Created manifest: {}", manifest.id);
                println!("      Name: {}", manifest.name);
                println!("      Provider: {:?}", manifest.provider);
                if let Some(input_schema) = &manifest.input_schema {
                    println!("      Has input schema: ✅");
                } else {
                    println!("      Has input schema: ❌");
                }
            }
        }
        Err(e) => {
            println!("    ❌ Discovery failed: {}", e);
            println!("    💡 This is normal if the server is not accessible or requires authentication");
        }
    }

    Ok(())
}

async fn test_with_marketplace_and_catalog() -> Result<(), Box<dyn std::error::Error>> {
    // Create marketplace and catalog
    use ccos::capabilities::registry::CapabilityRegistry;
    use tokio::sync::RwLock;
    
    let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
    let marketplace = Arc::new(CapabilityMarketplace::new(registry));
    let catalog = Arc::new(CatalogService::new());

    // Create unified service with marketplace and catalog
    let unified_service = Arc::new(
        MCPDiscoveryService::new()
            .with_marketplace(Arc::clone(&marketplace))
            .with_catalog(Arc::clone(&catalog))
    );

    println!("  ✅ Created service with marketplace and catalog");

    // Try to discover from first available server
    let servers = unified_service.list_known_servers();
    if let Some(server_config) = servers.first() {
        println!("\n  🔍 Discovering and registering tools from: {}", server_config.name);
        
        let options = DiscoveryOptions {
            introspect_output_schemas: false,
            use_cache: true,
            register_in_marketplace: true, // Enable auto-registration
            export_to_rtfs: true, // Enable auto-export to RTFS files
            export_directory: Some("capabilities/discovered".to_string()),
            auth_headers: server_config.auth_token.as_ref().map(|token| {
                let mut headers = std::collections::HashMap::new();
                headers.insert("Authorization".to_string(), format!("Bearer {}", token));
                headers
            }),
        };

        // Use discover_and_export_tools which handles registration and export automatically
        match unified_service.discover_and_export_tools(server_config, &options).await {
            Ok(manifests) => {
                println!("  ✅ Discovered {} tools", manifests.len());
                
                // Tools are already registered and exported by discover_and_export_tools
                for manifest in &manifests {
                    println!("    ✅ Registered: {}", manifest.id);
                }

                // Verify in marketplace
                let all_capabilities = marketplace.list_capabilities().await;
                println!("\n  📊 Marketplace now has {} total capabilities", all_capabilities.len());
                
                // Verify in catalog
                let catalog_results = catalog.search_keyword(
                    &server_config.name,
                    None,
                    10,
                );
                println!("  📚 Catalog search for '{}' returned {} results", 
                    server_config.name, catalog_results.len());
            }
            Err(e) => {
                println!("  ⚠️  Discovery failed: {}", e);
            }
        }
    } else {
        println!("  ⚠️  No servers configured for marketplace/catalog test");
    }

    Ok(())
}

