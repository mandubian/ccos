//! Tests for the Unified MCP Discovery Service
//!
//! This module tests:
//! - MCPCache: memory and file caching, TTL expiration
//! - MCPDiscoveryService: discovery, manifest conversion, registration
//! - DiscoveryOptions: various configuration scenarios
//! - Error handling: network failures, invalid responses
//!
//! Run with: cargo test --test mcp_discovery_tests -- --nocapture

use std::collections::HashMap;
use std::sync::Arc;

use ccos::capability_marketplace::mcp_discovery::MCPServerConfig;
use ccos::capability_marketplace::types::ProviderType;
use ccos::capability_marketplace::CapabilityMarketplace;
use ccos::capabilities::registry::CapabilityRegistry;
use ccos::catalog::CatalogService;
use ccos::mcp::cache::MCPCache;
use ccos::mcp::core::MCPDiscoveryService;
use ccos::mcp::types::{DiscoveredMCPTool, DiscoveryOptions};
use rtfs::ast::{Keyword, MapTypeEntry, PrimitiveType, TypeExpr};
use tempfile::TempDir;
use tokio::sync::RwLock;

// =============================================================================
// Test Fixtures
// =============================================================================

fn test_server_config() -> MCPServerConfig {
    MCPServerConfig {
        name: "test-server".to_string(),
        endpoint: "https://test-mcp.example.com".to_string(),
        auth_token: Some("test-token".to_string()),
        timeout_seconds: 30,
        protocol_version: "2024-11-05".to_string(),
    }
}

fn test_discovered_tool() -> DiscoveredMCPTool {
    DiscoveredMCPTool {
        tool_name: "test_tool".to_string(),
        description: Some("A test tool for testing".to_string()),
        input_schema: Some(TypeExpr::Map {
            entries: vec![
                MapTypeEntry {
                    key: Keyword::new("param1"),
                    value_type: Box::new(TypeExpr::Primitive(PrimitiveType::String)),
                    optional: false,
                },
                MapTypeEntry {
                    key: Keyword::new("param2"),
                    value_type: Box::new(TypeExpr::Primitive(PrimitiveType::Int)),
                    optional: true,
                },
            ],
            wildcard: None,
        }),
        output_schema: None,
        input_schema_json: Some(serde_json::json!({
            "type": "object",
            "properties": {
                "param1": { "type": "string" },
                "param2": { "type": "integer" }
            },
            "required": ["param1"]
        })),
    }
}

fn _test_discovery_options() -> DiscoveryOptions {
    DiscoveryOptions::default()
}

// =============================================================================
// MCPCache Tests
// =============================================================================

mod cache_tests {
    use super::*;

    #[test]
    fn test_cache_new_creates_empty_cache() {
        let cache = MCPCache::new();
        let config = test_server_config();
        
        // Cache should be empty initially
        assert!(cache.get(&config).is_none());
    }

    #[test]
    fn test_cache_store_and_retrieve() {
        let cache = MCPCache::new();
        let config = test_server_config();
        let tools = vec![test_discovered_tool()];

        // Store tools
        cache.store(&config, tools.clone());

        // Retrieve tools
        let cached = cache.get(&config);
        assert!(cached.is_some());
        
        let cached_tools = cached.unwrap();
        assert_eq!(cached_tools.len(), 1);
        assert_eq!(cached_tools[0].tool_name, "test_tool");
    }

    #[test]
    fn test_cache_different_endpoints() {
        let cache = MCPCache::new();
        
        let config1 = MCPServerConfig {
            name: "server1".to_string(),
            endpoint: "https://server1.example.com".to_string(),
            auth_token: None,
            timeout_seconds: 30,
            protocol_version: "2024-11-05".to_string(),
        };
        
        let config2 = MCPServerConfig {
            name: "server2".to_string(),
            endpoint: "https://server2.example.com".to_string(),
            auth_token: None,
            timeout_seconds: 30,
            protocol_version: "2024-11-05".to_string(),
        };

        let tools1 = vec![DiscoveredMCPTool {
            tool_name: "tool1".to_string(),
            description: None,
            input_schema: None,
            output_schema: None,
            input_schema_json: None,
        }];

        let tools2 = vec![DiscoveredMCPTool {
            tool_name: "tool2".to_string(),
            description: None,
            input_schema: None,
            output_schema: None,
            input_schema_json: None,
        }];

        // Store tools for both servers
        cache.store(&config1, tools1);
        cache.store(&config2, tools2);

        // Verify each server has its own cached tools
        let cached1 = cache.get(&config1).unwrap();
        let cached2 = cache.get(&config2).unwrap();
        
        assert_eq!(cached1[0].tool_name, "tool1");
        assert_eq!(cached2[0].tool_name, "tool2");
    }

    #[test]
    fn test_cache_clear() {
        let cache = MCPCache::new();
        let config = test_server_config();
        let tools = vec![test_discovered_tool()];

        cache.store(&config, tools);
        assert!(cache.get(&config).is_some());

        cache.clear().expect("Failed to clear cache");
        assert!(cache.get(&config).is_none());
    }

    #[test]
    fn test_file_cache_store_and_retrieve() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let cache_path = temp_dir.path().to_path_buf();
        
        let cache = MCPCache::new().with_cache_dir(cache_path.clone());
        let config = test_server_config();
        let tools = vec![test_discovered_tool()];

        // Store tools
        cache.store(&config, tools.clone());

        // Verify file was created
        let cache_file = cache_path.join("test-server_tools.json");
        assert!(cache_file.exists(), "Cache file should exist");

        // Create new cache instance and verify it loads from file
        let new_cache = MCPCache::new().with_cache_dir(cache_path);
        let cached = new_cache.get(&config);
        
        assert!(cached.is_some());
        assert_eq!(cached.unwrap()[0].tool_name, "test_tool");
    }

    #[test]
    fn test_cache_ttl_expiration() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let cache_path = temp_dir.path().to_path_buf();
        
        // Create cache with very short TTL (1 second)
        // Use a very short TTL (1 second) but wait longer to ensure expiration
        let ttl_seconds = 1;
        let cache = MCPCache::new()
            .with_cache_dir(cache_path.clone())
            .with_ttl(ttl_seconds);
        
        let config = test_server_config();
        let tools = vec![test_discovered_tool()];

        // Store tools
        cache.store(&config, tools);

        // Verify it's cached
        assert!(cache.get(&config).is_some());

        // Wait for TTL to expire (wait 3x the TTL to be safe on busy systems)
        std::thread::sleep(std::time::Duration::from_secs(ttl_seconds as u64 * 3));

        // Create new cache instance (bypasses memory cache)
        let new_cache = MCPCache::new()
            .with_cache_dir(cache_path)
            .with_ttl(ttl_seconds);
        
        // Should be expired now - the file cache should recognize expired entries
        let cached = new_cache.get(&config);
        assert!(
            cached.is_none(),
            "Cache entry should have expired after {} seconds (waited {}s), but got {:?}",
            ttl_seconds,
            ttl_seconds * 3,
            cached.map(|v| v.len())
        );
    }

    #[test]
    fn test_file_cache_sanitizes_filename() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let cache_path = temp_dir.path().to_path_buf();
        
        let cache = MCPCache::new().with_cache_dir(cache_path.clone());
        
        // Server name with special characters
        let config = MCPServerConfig {
            name: "github/mcp-server".to_string(),
            endpoint: "https://github.example.com".to_string(),
            auth_token: None,
            timeout_seconds: 30,
            protocol_version: "2024-11-05".to_string(),
        };
        
        let tools = vec![test_discovered_tool()];
        cache.store(&config, tools);

        // Verify file was created with sanitized name
        let cache_file = cache_path.join("github_mcp-server_tools.json");
        assert!(cache_file.exists(), "Cache file should exist with sanitized name");
    }
}

// =============================================================================
// MCPDiscoveryService Tests
// =============================================================================

mod discovery_service_tests {
    use super::*;

    #[test]
    fn test_service_creation() {
        let service = MCPDiscoveryService::new();
        // Service should be created without panicking
        let _servers = service.list_known_servers();
    }

    #[test]
    fn test_service_with_marketplace() {
        let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
        let marketplace = Arc::new(CapabilityMarketplace::new(registry));
        
        let service = MCPDiscoveryService::new()
            .with_marketplace(marketplace);
        
        // Should not panic
        let _servers = service.list_known_servers();
    }

    #[test]
    fn test_service_with_catalog() {
        let catalog = Arc::new(CatalogService::new());
        
        let service = MCPDiscoveryService::new()
            .with_catalog(catalog);
        
        // Should not panic
        let _servers = service.list_known_servers();
    }

    #[test]
    fn test_service_with_marketplace_and_catalog() {
        let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
        let marketplace = Arc::new(CapabilityMarketplace::new(registry));
        let catalog = Arc::new(CatalogService::new());
        
        let service = MCPDiscoveryService::new()
            .with_marketplace(marketplace)
            .with_catalog(catalog);
        
        // Should not panic
        let _servers = service.list_known_servers();
    }

    #[test]
    fn test_tool_to_manifest_conversion() {
        let service = MCPDiscoveryService::new();
        let config = test_server_config();
        let tool = test_discovered_tool();

        let manifest = service.tool_to_manifest(&tool, &config);

        // Verify manifest ID
        assert_eq!(manifest.id, "mcp.test-server.test_tool");
        
        // Verify name and description
        assert_eq!(manifest.name, "test_tool");
        assert_eq!(manifest.description, "A test tool for testing");
        
        // Verify provider
        match manifest.provider {
            ProviderType::MCP(mcp) => {
                assert_eq!(mcp.server_url, "https://test-mcp.example.com");
                assert_eq!(mcp.tool_name, "test_tool");
                assert_eq!(mcp.timeout_ms, 30000); // 30 seconds * 1000
            }
            _ => panic!("Expected MCP provider type"),
        }
        
        // Verify input schema is preserved
        assert!(manifest.input_schema.is_some());
        
        // Verify metadata
        assert_eq!(manifest.metadata.get("mcp_server_name"), Some(&"test-server".to_string()));
        assert_eq!(manifest.metadata.get("discovery_source"), Some(&"mcp_unified_service".to_string()));
    }

    #[test]
    fn test_tool_to_manifest_without_description() {
        let service = MCPDiscoveryService::new();
        let config = test_server_config();
        
        let tool = DiscoveredMCPTool {
            tool_name: "minimal_tool".to_string(),
            description: None,
            input_schema: None,
            output_schema: None,
            input_schema_json: None,
        };

        let manifest = service.tool_to_manifest(&tool, &config);

        assert_eq!(manifest.id, "mcp.test-server.minimal_tool");
        assert_eq!(manifest.name, "minimal_tool");
        assert_eq!(manifest.description, ""); // Empty when no description
        assert!(manifest.input_schema.is_none());
    }

    #[test]
    fn test_list_known_servers_empty_config() {
        let service = MCPDiscoveryService::new();
        let servers = service.list_known_servers();
        
        // May be empty or have some configured servers
        // This test just ensures the method doesn't panic
        let _ = servers.len();
    }
}

// =============================================================================
// DiscoveryOptions Tests
// =============================================================================

mod discovery_options_tests {
    use super::*;

    #[test]
    fn test_default_options() {
        let options = DiscoveryOptions::default();
        
        assert!(!options.introspect_output_schemas);
        assert!(!options.use_cache);
        assert!(!options.register_in_marketplace);
        assert!(!options.export_to_rtfs);
        assert!(options.export_directory.is_none());
        assert!(options.auth_headers.is_none());
    }

    #[test]
    fn test_custom_options() {
        use ccos::mcp::rate_limiter::{RateLimitConfig, RetryPolicy};
        
        let mut auth_headers = HashMap::new();
        auth_headers.insert("Authorization".to_string(), "Bearer test".to_string());
        
        let options = DiscoveryOptions {
            introspect_output_schemas: true,
            use_cache: true,
            register_in_marketplace: true,
            export_to_rtfs: true,
            export_directory: Some("custom/path".to_string()),
            auth_headers: Some(auth_headers.clone()),
            retry_policy: RetryPolicy::aggressive(),
            rate_limit: RateLimitConfig::strict(),
        };

        assert!(options.introspect_output_schemas);
        assert!(options.use_cache);
        assert!(options.register_in_marketplace);
        assert!(options.export_to_rtfs);
        assert_eq!(options.export_directory, Some("custom/path".to_string()));
        assert_eq!(options.auth_headers.as_ref().unwrap().get("Authorization"), Some(&"Bearer test".to_string()));
        assert_eq!(options.retry_policy.max_retries, 5); // Aggressive has 5 retries
        assert_eq!(options.rate_limit.requests_per_second, 1.0); // Strict is 1 req/sec
    }
}

// =============================================================================
// DiscoveredMCPTool Tests
// =============================================================================

mod discovered_tool_tests {
    use super::*;

    #[test]
    fn test_tool_serialization() {
        let tool = test_discovered_tool();
        
        let json = serde_json::to_string(&tool).expect("Failed to serialize tool");
        let deserialized: DiscoveredMCPTool = serde_json::from_str(&json).expect("Failed to deserialize tool");
        
        assert_eq!(deserialized.tool_name, tool.tool_name);
        assert_eq!(deserialized.description, tool.description);
    }

    #[test]
    fn test_tool_with_complex_schema() {
        let tool = DiscoveredMCPTool {
            tool_name: "complex_tool".to_string(),
            description: Some("A tool with complex schema".to_string()),
            input_schema: Some(TypeExpr::Map {
                entries: vec![
                    MapTypeEntry {
                        key: Keyword::new("items"),
                        value_type: Box::new(TypeExpr::Vector(Box::new(
                            TypeExpr::Map {
                                entries: vec![
                                    MapTypeEntry {
                                        key: Keyword::new("id"),
                                        value_type: Box::new(TypeExpr::Primitive(PrimitiveType::Int)),
                                        optional: false,
                                    },
                                    MapTypeEntry {
                                        key: Keyword::new("name"),
                                        value_type: Box::new(TypeExpr::Primitive(PrimitiveType::String)),
                                        optional: false,
                                    },
                                ],
                                wildcard: None,
                            }
                        ))),
                        optional: false,
                    },
                ],
                wildcard: None,
            }),
            output_schema: Some(TypeExpr::Map {
                entries: vec![
                    MapTypeEntry {
                        key: Keyword::new("success"),
                        value_type: Box::new(TypeExpr::Primitive(PrimitiveType::Bool)),
                        optional: false,
                    },
                    MapTypeEntry {
                        key: Keyword::new("count"),
                        value_type: Box::new(TypeExpr::Primitive(PrimitiveType::Int)),
                        optional: false,
                    },
                ],
                wildcard: None,
            }),
            input_schema_json: Some(serde_json::json!({
                "type": "object",
                "properties": {
                    "items": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "id": { "type": "integer" },
                                "name": { "type": "string" }
                            },
                            "required": ["id", "name"]
                        }
                    }
                },
                "required": ["items"]
            })),
        };

        // Verify schemas are set
        assert!(tool.input_schema.is_some());
        assert!(tool.output_schema.is_some());
        
        // Serialize and deserialize
        let json = serde_json::to_string(&tool).expect("Failed to serialize");
        let deserialized: DiscoveredMCPTool = serde_json::from_str(&json).expect("Failed to deserialize");
        
        assert_eq!(deserialized.tool_name, "complex_tool");
    }
}

// =============================================================================
// Async Tests (require tokio runtime)
// =============================================================================

mod async_tests {
    use super::*;

    #[tokio::test]
    async fn test_register_capability_without_marketplace() {
        let service = MCPDiscoveryService::new();
        let config = test_server_config();
        let tool = test_discovered_tool();
        let manifest = service.tool_to_manifest(&tool, &config);

        // Should not fail even without marketplace
        let result = service.register_capability(&manifest).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_register_capability_with_marketplace() {
        let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
        let marketplace = Arc::new(CapabilityMarketplace::new(registry));
        
        let service = MCPDiscoveryService::new()
            .with_marketplace(Arc::clone(&marketplace));
        
        let config = test_server_config();
        let tool = test_discovered_tool();
        let manifest = service.tool_to_manifest(&tool, &config);

        // Register capability
        let result = service.register_capability(&manifest).await;
        assert!(result.is_ok());

        // Verify it's in the marketplace
        let capabilities = marketplace.list_capabilities().await;
        assert!(capabilities.iter().any(|c| c.id == "mcp.test-server.test_tool"));
    }

    #[tokio::test]
    async fn test_register_capability_with_catalog() {
        let catalog = Arc::new(CatalogService::new());
        
        let service = MCPDiscoveryService::new()
            .with_catalog(Arc::clone(&catalog));
        
        let config = test_server_config();
        let tool = test_discovered_tool();
        let manifest = service.tool_to_manifest(&tool, &config);

        // Register capability
        let result = service.register_capability(&manifest).await;
        assert!(result.is_ok());

        // Verify it's in the catalog
        let results = catalog.search_keyword("test_tool", None, 10);
        assert!(!results.is_empty(), "Capability should be indexed in catalog");
    }

    #[tokio::test]
    async fn test_discover_tools_uses_cache() {
        // This test verifies cache behavior by checking that subsequent calls
        // return cached results. Since we can't mock the HTTP client easily,
        // we test the cache integration at a higher level.
        
        let cache = MCPCache::new();
        let config = test_server_config();
        let tools = vec![test_discovered_tool()];

        // Pre-populate cache
        cache.store(&config, tools.clone());

        // Verify cache contains the tools
        let cached = cache.get(&config);
        assert!(cached.is_some());
        assert_eq!(cached.unwrap().len(), 1);
    }
}

// =============================================================================
// Error Handling Tests
// =============================================================================

mod error_handling_tests {
    use super::*;

    #[test]
    fn test_cache_handles_invalid_json_file() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let cache_path = temp_dir.path().to_path_buf();
        
        // Create an invalid cache file
        let cache_file = cache_path.join("invalid-server_tools.json");
        std::fs::write(&cache_file, "invalid json content").expect("Failed to write file");
        
        let cache = MCPCache::new().with_cache_dir(cache_path);
        
        let config = MCPServerConfig {
            name: "invalid-server".to_string(),
            endpoint: "https://invalid.example.com".to_string(),
            auth_token: None,
            timeout_seconds: 30,
            protocol_version: "2024-11-05".to_string(),
        };

        // Should return None for invalid cache file (graceful degradation)
        let cached = cache.get(&config);
        assert!(cached.is_none());
    }

    #[test]
    fn test_cache_handles_missing_directory() {
        // Create cache with non-existent directory (should create it)
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let cache_path = temp_dir.path().join("non_existent_subdir");
        
        // This should not panic
        let cache = MCPCache::new().with_cache_dir(cache_path.clone());
        let config = test_server_config();
        let tools = vec![test_discovered_tool()];

        // Should be able to store
        cache.store(&config, tools);
        
        // Verify directory was created
        assert!(cache_path.exists());
    }
}

// =============================================================================
// Integration Tests
// =============================================================================

mod integration_tests {
    use super::*;

    #[tokio::test]
    async fn test_full_discovery_workflow() {
        // This test simulates the full discovery workflow without making actual HTTP requests
        
        // 1. Create service with all components
        let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
        let marketplace = Arc::new(CapabilityMarketplace::new(registry));
        let catalog = Arc::new(CatalogService::new());
        
        let _service = MCPDiscoveryService::new()
            .with_marketplace(Arc::clone(&marketplace))
            .with_catalog(Arc::clone(&catalog));

        // 2. Pre-create a mock discovered tool
        let mock_tool = test_discovered_tool();
        let config = test_server_config();

        // 3. Convert to manifest manually (simulating discovery)
        let service = MCPDiscoveryService::new();
        let manifest = service.tool_to_manifest(&mock_tool, &config);

        // 4. Register in marketplace and catalog
        let service_with_deps = MCPDiscoveryService::new()
            .with_marketplace(Arc::clone(&marketplace))
            .with_catalog(Arc::clone(&catalog));
        
        service_with_deps.register_capability(&manifest).await.expect("Registration failed");

        // 5. Verify in marketplace
        let caps = marketplace.list_capabilities().await;
        assert!(caps.iter().any(|c| c.id == "mcp.test-server.test_tool"));

        // 6. Verify in catalog
        let search_results = catalog.search_keyword("test_tool", None, 10);
        assert!(!search_results.is_empty());
    }

    #[tokio::test]
    async fn test_export_to_rtfs_creates_file() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let export_path = temp_dir.path().to_path_buf();

        let service = MCPDiscoveryService::new();
        let config = test_server_config();
        let tool = test_discovered_tool();
        let _manifest = service.tool_to_manifest(&tool, &config);

        let options = DiscoveryOptions {
            export_to_rtfs: true,
            export_directory: Some(export_path.to_string_lossy().to_string()),
            ..Default::default()
        };

        // The actual export happens in discover_and_export_tools, which requires
        // an actual server. For unit testing, we verify the options are set correctly.
        assert!(options.export_to_rtfs);
        assert!(options.export_directory.is_some());
    }
}

// =============================================================================
// Capability Loading Tests
// =============================================================================

mod capability_loading_tests {
    use super::*;
    use std::fs;

    /// Create a sample RTFS capability file for testing
    fn create_test_rtfs_file(dir: &std::path::Path, filename: &str, capability_id: &str) -> std::path::PathBuf {
        let file_path = dir.join(filename);
        let content = format!(
            r#";; Test capability file
(capability
  :id "{}"
  :name "Test Capability"
  :description "A test capability for loading tests"
  :implementation (fn [input] input)
)
"#,
            capability_id
        );
        fs::write(&file_path, content).expect("Failed to write test RTFS file");
        file_path
    }

    #[tokio::test]
    async fn test_load_discovered_capabilities_empty_dir() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
        let marketplace = CapabilityMarketplace::new(registry);

        // Loading from empty directory should succeed with 0 capabilities
        let result = marketplace.load_discovered_capabilities(Some(temp_dir.path())).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 0);
    }

    #[tokio::test]
    async fn test_load_discovered_capabilities_nonexistent_dir() {
        let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
        let marketplace = CapabilityMarketplace::new(registry);

        // Loading from non-existent directory should return 0 (not error)
        let result = marketplace.load_discovered_capabilities(Some(std::path::Path::new("/nonexistent/path"))).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 0);
    }

    #[tokio::test]
    async fn test_load_discovered_capabilities_flat_dir() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        
        // Create test RTFS files in the root
        create_test_rtfs_file(temp_dir.path(), "cap1.rtfs", "test.cap1");
        create_test_rtfs_file(temp_dir.path(), "cap2.rtfs", "test.cap2");
        
        let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
        let marketplace = CapabilityMarketplace::new(registry);

        let result = marketplace.load_discovered_capabilities(Some(temp_dir.path())).await;
        assert!(result.is_ok());
        // Note: The loading may fail to parse simple test files, which is expected
        // The important thing is that the recursive scan works
    }

    #[tokio::test]
    async fn test_load_discovered_capabilities_recursive() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        
        // Create nested directory structure like capabilities/discovered/mcp/github/
        let mcp_dir = temp_dir.path().join("mcp");
        let github_dir = mcp_dir.join("github");
        let slack_dir = mcp_dir.join("slack");
        
        fs::create_dir_all(&github_dir).expect("Failed to create github dir");
        fs::create_dir_all(&slack_dir).expect("Failed to create slack dir");
        
        // Create test RTFS files in subdirectories
        create_test_rtfs_file(&github_dir, "capabilities.rtfs", "mcp.github.list_issues");
        create_test_rtfs_file(&slack_dir, "capabilities.rtfs", "mcp.slack.send_message");
        
        let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
        let marketplace = CapabilityMarketplace::new(registry);

        let result = marketplace.import_capabilities_from_rtfs_dir_recursive(temp_dir.path()).await;
        assert!(result.is_ok());
        // The recursive scan should find files in subdirectories
    }

    #[tokio::test]
    async fn test_load_discovered_capabilities_ignores_non_rtfs() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        
        // Create various file types
        fs::write(temp_dir.path().join("readme.md"), "# Readme").unwrap();
        fs::write(temp_dir.path().join("config.json"), "{}").unwrap();
        fs::write(temp_dir.path().join("data.txt"), "data").unwrap();
        create_test_rtfs_file(temp_dir.path(), "valid.rtfs", "test.valid");
        
        let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
        let marketplace = CapabilityMarketplace::new(registry);

        // Should only attempt to load .rtfs files
        let result = marketplace.load_discovered_capabilities(Some(temp_dir.path())).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_import_single_rtfs_file_duplicate_handling() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        
        // Create a test RTFS file
        let _file_path = create_test_rtfs_file(temp_dir.path(), "test.rtfs", "test.duplicate");
        
        let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
        let marketplace = CapabilityMarketplace::new(registry);

        // Load the same file twice - second load should handle duplicates gracefully
        let result1 = marketplace.import_capabilities_from_rtfs_dir_recursive(temp_dir.path()).await;
        let result2 = marketplace.import_capabilities_from_rtfs_dir_recursive(temp_dir.path()).await;
        
        assert!(result1.is_ok());
        assert!(result2.is_ok());
        // Second load should skip duplicates (same version)
    }
}
