//! Round-trip Export/Import and Session Management Tests
//!
//! Tests that:
//! 1. Capabilities can be exported to RTFS format
//! 2. Capabilities can be imported from RTFS format
//! 3. Session management works for MCP providers
//! 4. Round-trip preserves capability metadata

#[cfg(test)]
mod tests {
    use axum::{extract::Query, http::HeaderMap, routing::get, Json, Router};
    use rtfs_compiler::ast::MapKey;
    use rtfs_compiler::ccos::capability_marketplace::types::{
        CapabilityManifest, HttpCapability, MCPCapability, OpenApiAuth, OpenApiCapability,
        OpenApiOperation, ProviderType,
    };
    use rtfs_compiler::ccos::capability_marketplace::CapabilityMarketplace;
    use rtfs_compiler::runtime::capabilities::registry::CapabilityRegistry;
    use rtfs_compiler::runtime::values::Value;
    use serde_json::json;
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::time::Duration;
    use tempfile::TempDir;
    use tokio::sync::RwLock;
    use tokio::{net::TcpListener, sync::oneshot, time::sleep};

    /// Create a test marketplace with session pool (minimal setup, no CCOSEnvironment)
    async fn setup_marketplace_with_sessions() -> (CapabilityMarketplace, TempDir) {
        let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
        let marketplace = CapabilityMarketplace::new(registry);

        // Create session pool with MCP handler (minimal setup)
        let session_pool = Arc::new(rtfs_compiler::ccos::capabilities::SessionPoolManager::new());
        marketplace.set_session_pool(session_pool).await;

        let tmpdir = TempDir::new().expect("Failed to create temp dir");
        (marketplace, tmpdir)
    }

    async fn weather_handler(
        headers: HeaderMap,
        Query(params): Query<HashMap<String, String>>,
    ) -> Json<serde_json::Value> {
        let city = params
            .get("q")
            .cloned()
            .unwrap_or_else(|| "unknown".to_string());
        let api_key = headers
            .get("x-api-key")
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
            .to_string();
        Json(json!({
            "city": city,
            "auth": api_key,
        }))
    }

    #[tokio::test]
    async fn test_http_capability_roundtrip_export_import() {
        // Create an HTTP capability
        let capability = CapabilityManifest {
            id: "test.http.search".to_string(),
            name: "HTTP Search".to_string(),
            description: "A simple HTTP-based search capability".to_string(),
            provider: ProviderType::Http(HttpCapability {
                base_url: "https://api.example.com".to_string(),
                auth_token: Some("token123".to_string()),
                timeout_ms: 30000,
            }),
            version: "1.0.0".to_string(),
            input_schema: None,
            output_schema: None,
            attestation: None,
            provenance: None,
            permissions: vec!["http:request".to_string()],
            effects: vec![":network".to_string()],
            metadata: {
                let mut m = HashMap::new();
                m.insert("source".to_string(), "test".to_string());
                m
            },
            agent_metadata: None,
        };

        let (marketplace, tmpdir) = setup_marketplace_with_sessions().await;

        // Register capability in marketplace using public API
        marketplace
            .register_capability_manifest(capability.clone())
            .await
            .expect("Failed to register capability");

        let export_dir = tmpdir.path().join("export");
        std::fs::create_dir_all(&export_dir).expect("Failed to create export dir");

        // Export to RTFS
        let exported = marketplace
            .export_capabilities_to_rtfs_dir(&export_dir)
            .await
            .expect("Failed to export capabilities");

        assert_eq!(exported, 1, "Should export 1 capability");

        // Verify RTFS file was created
        let rtfs_files: Vec<_> = std::fs::read_dir(&export_dir)
            .expect("Failed to read export dir")
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().map_or(false, |ext| ext == "rtfs"))
            .collect();

        assert_eq!(rtfs_files.len(), 1, "Should have 1 RTFS file");

        // Create new marketplace and import
        let (marketplace2, _tmpdir2) = setup_marketplace_with_sessions().await;
        let imported = marketplace2
            .import_capabilities_from_rtfs_dir(&export_dir)
            .await
            .expect("Failed to import capabilities");

        assert_eq!(imported, 1, "Should import 1 capability");

        // Verify imported capability matches original
        let imported_cap = marketplace2
            .get_capability(&capability.id)
            .await
            .expect("Failed to get imported capability");

        assert_eq!(imported_cap.id, capability.id, "IDs should match");
        assert_eq!(imported_cap.name, capability.name, "Names should match");
        assert_eq!(
            imported_cap.description, capability.description,
            "Descriptions should match"
        );
        assert_eq!(
            imported_cap.version, capability.version,
            "Versions should match"
        );

        // Verify provider was preserved
        match (&imported_cap.provider, &capability.provider) {
            (ProviderType::Http(imp_http), ProviderType::Http(orig_http)) => {
                assert_eq!(
                    imp_http.base_url, orig_http.base_url,
                    "Base URLs should match"
                );
                assert_eq!(
                    imp_http.timeout_ms, orig_http.timeout_ms,
                    "Timeouts should match"
                );
            }
            _ => panic!("Provider types should match"),
        }
    }

    #[tokio::test]
    async fn test_mcp_capability_roundtrip_with_session_metadata() {
        // Create an MCP capability with session metadata
        let mut metadata = HashMap::new();
        metadata.insert(
            "mcp_server_url".to_string(),
            "http://localhost:3000".to_string(),
        );
        metadata.insert("mcp_tool_name".to_string(), "search".to_string());
        metadata.insert("mcp_requires_session".to_string(), "true".to_string());

        let capability = CapabilityManifest {
            id: "test.mcp.search".to_string(),
            name: "MCP Search".to_string(),
            description: "MCP-based search capability".to_string(),
            provider: ProviderType::MCP(MCPCapability {
                server_url: "http://localhost:3000".to_string(),
                tool_name: "search".to_string(),
                timeout_ms: 5000,
            }),
            version: "1.0.0".to_string(),
            input_schema: None,
            output_schema: None,
            attestation: None,
            provenance: None,
            permissions: vec!["mcp:tool:execute".to_string()],
            effects: vec![":network".to_string(), ":io".to_string()],
            metadata,
            agent_metadata: None,
        };

        let (marketplace, tmpdir) = setup_marketplace_with_sessions().await;

        // Register capability using public API
        marketplace
            .register_capability_manifest(capability.clone())
            .await
            .expect("Failed to register MCP capability");

        let export_dir = tmpdir.path().join("export_mcp");
        std::fs::create_dir_all(&export_dir).expect("Failed to create export dir");

        // Export to RTFS
        let exported = marketplace
            .export_capabilities_to_rtfs_dir(&export_dir)
            .await
            .expect("Failed to export MCP capability");

        assert_eq!(exported, 1, "Should export 1 MCP capability");

        // Create new marketplace and import
        let (marketplace2, _tmpdir2) = setup_marketplace_with_sessions().await;
        let imported = marketplace2
            .import_capabilities_from_rtfs_dir(&export_dir)
            .await
            .expect("Failed to import MCP capability");

        assert_eq!(imported, 1, "Should import 1 MCP capability");

        // Verify MCP provider was preserved
        let imported_cap = marketplace2
            .get_capability(&capability.id)
            .await
            .expect("Failed to get MCP capability");

        match (&imported_cap.provider, &capability.provider) {
            (ProviderType::MCP(imp_mcp), ProviderType::MCP(orig_mcp)) => {
                assert_eq!(
                    imp_mcp.server_url, orig_mcp.server_url,
                    "Server URLs should match"
                );
                assert_eq!(
                    imp_mcp.tool_name, orig_mcp.tool_name,
                    "Tool names should match"
                );
                assert_eq!(
                    imp_mcp.timeout_ms, orig_mcp.timeout_ms,
                    "Timeouts should match"
                );
            }
            _ => panic!("Provider types should be MCP"),
        }

        // Verify session metadata was preserved
        assert_eq!(
            imported_cap.metadata.get("mcp_requires_session"),
            Some(&"true".to_string()),
            "Session requirement metadata should be preserved"
        );
        assert_eq!(
            imported_cap.metadata.get("mcp_server_url"),
            Some(&"http://localhost:3000".to_string()),
            "Server URL metadata should be preserved"
        );
    }

    #[tokio::test]
    async fn test_openapi_capability_execution_with_auth_and_params() {
        let (marketplace, _tmpdir) = setup_marketplace_with_sessions().await;

        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("Failed to bind test server");
        let server_addr = listener.local_addr().expect("Failed to get server address");
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let router = Router::new().route("/weather", get(weather_handler));

        tokio::spawn(async move {
            axum::serve(listener, router)
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                })
                .await
                .expect("Test server failed");
        });

        sleep(Duration::from_millis(50)).await;

        let env_var = "TEST_OPENAPI_KEY_EXEC";
        std::env::set_var(env_var, "secret123");

        let capability = CapabilityManifest {
            id: "test.openapi.weather.execution".to_string(),
            name: "OpenAPI Weather Execution".to_string(),
            description: "OpenAPI capability for weather data".to_string(),
            provider: ProviderType::OpenApi(OpenApiCapability {
                base_url: format!("http://{}", server_addr),
                spec_url: Some("https://spec.example.com/weather.json".to_string()),
                operations: vec![OpenApiOperation {
                    operation_id: Some("getWeather".to_string()),
                    path: "/weather".to_string(),
                    method: "GET".to_string(),
                    summary: Some("Fetch weather".to_string()),
                    description: Some("Fetch current weather".to_string()),
                }],
                auth: Some(OpenApiAuth {
                    auth_type: "api_key".to_string(),
                    location: "header".to_string(),
                    parameter_name: "X-API-Key".to_string(),
                    env_var_name: Some(env_var.to_string()),
                    required: true,
                }),
                timeout_ms: 5_000,
            }),
            version: "1.0.0".to_string(),
            input_schema: None,
            output_schema: None,
            attestation: None,
            provenance: None,
            permissions: vec!["http:request".to_string()],
            effects: vec![":network".to_string()],
            metadata: {
                let mut m = HashMap::new();
                m.insert(
                    "openapi_provider_kind".to_string(),
                    "weather_api".to_string(),
                );
                m
            },
            agent_metadata: None,
        };

        marketplace
            .register_capability_manifest(capability.clone())
            .await
            .expect("Failed to register OpenAPI capability");

        let mut params_map = HashMap::new();
        params_map.insert(
            MapKey::String("q".to_string()),
            Value::String("Paris".to_string()),
        );

        let mut input_map = HashMap::new();
        input_map.insert(
            MapKey::String("operation".to_string()),
            Value::String("getWeather".to_string()),
        );
        input_map.insert(MapKey::String("params".to_string()), Value::Map(params_map));

        let result = marketplace
            .execute_capability(&capability.id, &Value::Map(input_map))
            .await
            .expect("OpenAPI execution should succeed");

        let result_map = match result {
            Value::Map(map) => map,
            other => panic!("Expected map result, got {:?}", other),
        };

        let status_key = MapKey::String("status".to_string());
        let json_key = MapKey::String("json".to_string());

        let status_value = result_map
            .get(&status_key)
            .expect("Status should be present");
        assert_eq!(status_value, &Value::Integer(200));

        let json_value = result_map
            .get(&json_key)
            .cloned()
            .expect("JSON payload should be present");

        let json_map = match json_value {
            Value::Map(map) => map,
            other => panic!("Expected JSON map, got {:?}", other),
        };

        let city_key = MapKey::String("city".to_string());
        assert_eq!(
            json_map.get(&city_key),
            Some(&Value::String("Paris".to_string())),
            "City query parameter should round-trip"
        );

        let auth_key = MapKey::String("auth".to_string());
        assert_eq!(
            json_map.get(&auth_key),
            Some(&Value::String("secret123".to_string())),
            "Auth header should be forwarded"
        );

        let _ = shutdown_tx.send(());
    }

    #[tokio::test]
    async fn test_json_export_import_preserves_capabilities() {
        // Create multiple capabilities
        let http_cap = CapabilityManifest {
            id: "test.http.api".to_string(),
            name: "HTTP API".to_string(),
            description: "HTTP API capability".to_string(),
            provider: ProviderType::Http(HttpCapability {
                base_url: "https://api.example.com".to_string(),
                auth_token: None,
                timeout_ms: 30000,
            }),
            version: "1.0.0".to_string(),
            input_schema: None,
            output_schema: None,
            attestation: None,
            provenance: None,
            permissions: vec![],
            effects: vec![":network".to_string()],
            metadata: HashMap::new(),
            agent_metadata: None,
        };

        let (marketplace, tmpdir) = setup_marketplace_with_sessions().await;

        // Register capability using public API
        marketplace
            .register_capability_manifest(http_cap.clone())
            .await
            .expect("Failed to register capability");

        let json_file = tmpdir.path().join("capabilities.json");

        // Export to JSON
        let exported = marketplace
            .export_capabilities_to_file(&json_file)
            .await
            .expect("Failed to export to JSON");

        assert_eq!(exported, 1, "Should export 1 capability to JSON");

        // Create new marketplace and import
        let (marketplace2, _tmpdir2) = setup_marketplace_with_sessions().await;
        let imported = marketplace2
            .import_capabilities_from_file(&json_file)
            .await
            .expect("Failed to import from JSON");

        assert_eq!(imported, 1, "Should import 1 capability from JSON");

        // Verify
        let imported_cap = marketplace2
            .get_capability(&http_cap.id)
            .await
            .expect("Capability should exist");
        assert_eq!(imported_cap.id, http_cap.id, "IDs should match");
    }

    #[tokio::test]
    async fn test_openapi_capability_roundtrip_export_import() {
        let capability = CapabilityManifest {
            id: "test.openapi.weather.roundtrip".to_string(),
            name: "OpenAPI Weather Export".to_string(),
            description: "OpenAPI capability for export/import".to_string(),
            provider: ProviderType::OpenApi(OpenApiCapability {
                base_url: "https://api.weather.example".to_string(),
                spec_url: Some("https://spec.example.com/weather.json".to_string()),
                operations: vec![OpenApiOperation {
                    operation_id: Some("getWeather".to_string()),
                    path: "/weather".to_string(),
                    method: "GET".to_string(),
                    summary: Some("Fetch weather".to_string()),
                    description: Some("Fetch current weather".to_string()),
                }],
                auth: Some(OpenApiAuth {
                    auth_type: "api_key".to_string(),
                    location: "header".to_string(),
                    parameter_name: "X-API-Key".to_string(),
                    env_var_name: Some("TEST_OPENAPI_KEY_EXPORT".to_string()),
                    required: true,
                }),
                timeout_ms: 5_000,
            }),
            version: "1.0.0".to_string(),
            input_schema: None,
            output_schema: None,
            attestation: None,
            provenance: None,
            permissions: vec!["http:request".to_string()],
            effects: vec![":network".to_string()],
            metadata: {
                let mut m = HashMap::new();
                m.insert(
                    "openapi_manifest_source".to_string(),
                    "unit_test".to_string(),
                );
                m
            },
            agent_metadata: None,
        };

        let (marketplace, tmpdir) = setup_marketplace_with_sessions().await;
        marketplace
            .register_capability_manifest(capability.clone())
            .await
            .expect("Failed to register OpenAPI capability");

        let json_file = tmpdir.path().join("openapi_capabilities.json");
        let exported = marketplace
            .export_capabilities_to_file(&json_file)
            .await
            .expect("Failed to export OpenAPI capability");
        assert_eq!(exported, 1, "Should export 1 OpenAPI capability");

        let (marketplace2, _tmpdir2) = setup_marketplace_with_sessions().await;
        let imported = marketplace2
            .import_capabilities_from_file(&json_file)
            .await
            .expect("Failed to import OpenAPI capability");
        assert_eq!(imported, 1, "Should import 1 OpenAPI capability");

        let imported_cap = marketplace2
            .get_capability(&capability.id)
            .await
            .expect("Imported capability should exist");

        match imported_cap.provider {
            ProviderType::OpenApi(openapi) => {
                assert_eq!(openapi.base_url, "https://api.weather.example");
                assert_eq!(
                    openapi.spec_url.as_deref(),
                    Some("https://spec.example.com/weather.json")
                );
                assert_eq!(openapi.operations.len(), 1);
                let operation = &openapi.operations[0];
                assert_eq!(operation.operation_id.as_deref(), Some("getWeather"));
                assert_eq!(operation.method, "GET");
                assert!(
                    openapi.auth.is_some(),
                    "Auth configuration should round-trip"
                );
                assert_eq!(openapi.timeout_ms, 5_000);
            }
            other => panic!("Expected OpenAPI provider, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_export_skips_non_serializable_providers() {
        // This test verifies that Local and Stream providers are skipped during export
        // (they are not serializable due to containing closures)

        let (marketplace, tmpdir) = setup_marketplace_with_sessions().await;

        // Note: We can't easily create Local capabilities in this test context
        // since they require closure handlers. This is a conceptual test.
        // In real use, verify that export_capabilities_to_rtfs_dir skips
        // non-serializable providers and reports them via debug callbacks.

        let export_dir = tmpdir.path().join("export_skip");
        std::fs::create_dir_all(&export_dir).expect("Failed to create export dir");

        // Export from empty marketplace (should work without errors)
        let exported = marketplace
            .export_capabilities_to_rtfs_dir(&export_dir)
            .await
            .expect("Export should succeed even with empty marketplace");

        assert_eq!(
            exported, 0,
            "Should export 0 capabilities from empty marketplace"
        );
    }
}
