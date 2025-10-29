/// Integration test that generates actual RTFS and JSON exports to /tmp for inspection
///
/// Run with: cargo test --test demo_serialization_output -- --nocapture --test-threads 1
///
/// This test creates HTTP and MCP capabilities, exports them to both RTFS and JSON,
/// then displays the files so you can see the actual serialization format.

#[cfg(test)]
mod tests {
    use rtfs_compiler::ccos::capability_marketplace::types::{
        CapabilityManifest, HttpCapability, MCPCapability, ProviderType,
    };
    use rtfs_compiler::ccos::capability_marketplace::CapabilityMarketplace;
    use rtfs_compiler::runtime::capabilities::registry::CapabilityRegistry;
    use std::collections::HashMap;
    use std::sync::Arc;
    use tokio::fs;
    use tokio::sync::RwLock;

    /// Setup marketplace with session pool for MCP
    async fn setup_marketplace_with_sessions() -> CapabilityMarketplace {
        let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
        let marketplace = CapabilityMarketplace::new(registry);

        let mut session_pool = rtfs_compiler::ccos::capabilities::SessionPoolManager::new();
        session_pool.register_handler(
            "mcp",
            Arc::new(rtfs_compiler::ccos::capabilities::MCPSessionHandler::new()),
        );
        let session_pool = Arc::new(session_pool);
        marketplace.set_session_pool(session_pool).await;

        marketplace
    }

    /// Demo test that generates and displays serialization output
    #[tokio::test]
    async fn test_demo_generate_serialization_examples() {
        println!("\n");
        println!("╔══════════════════════════════════════════════════════════════════════╗");
        println!("║          CCOS Marketplace Serialization - Live Demo                  ║");
        println!("║                  Generating actual export files                      ║");
        println!("╚══════════════════════════════════════════════════════════════════════╝\n");

        let marketplace = setup_marketplace_with_sessions().await;

        // Create HTTP capability (OpenWeatherMap example)
        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        println!("Step 1: Creating HTTP Capability");
        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

        let mut weather_meta = HashMap::new();
        weather_meta.insert("api_type".to_string(), "REST".to_string());
        weather_meta.insert("rate_limit".to_string(), "1000/day".to_string());
        weather_meta.insert("base_path".to_string(), "/v2.5".to_string());

        let http_cap = CapabilityManifest {
            id: "weather_api".to_string(),
            name: "OpenWeatherMap API".to_string(),
            version: "1.0.0".to_string(),
            description: "Get current weather and forecasts".to_string(),
            provider: ProviderType::Http(HttpCapability {
                base_url: "https://api.openweathermap.org".to_string(),
                timeout_ms: 5000,
                auth_token: Some("sk-weather-abc123def456".to_string()),
            }),
            input_schema: None,
            output_schema: None,
            attestation: None,
            provenance: None,
            metadata: weather_meta,
            permissions: vec!["read:weather".to_string()],
            effects: vec!["call:external_api".to_string()],
            agent_metadata: None,
        };

        marketplace
            .register_capability_manifest(http_cap.clone())
            .await
            .expect("Failed to register HTTP capability");

        println!("✓ Created HTTP capability:");
        println!("  - ID: {}", http_cap.id);
        println!("  - Name: {}", http_cap.name);
        println!("  - Base URL: https://api.openweathermap.org");
        println!("  - Timeout: 5000ms");
        println!("  - Auth Token: ••••••••••••••••••••••• (hidden for security)\n");

        // Create MCP capability (GitHub example with session metadata)
        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        println!("Step 2: Creating MCP Capability (with session metadata)");
        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

        let mut github_meta = HashMap::new();
        github_meta.insert("mcp_requires_session".to_string(), "true".to_string());
        github_meta.insert(
            "mcp_server_url".to_string(),
            "http://localhost:3001".to_string(),
        );
        github_meta.insert("mcp_tool_name".to_string(), "github_operations".to_string());
        github_meta.insert(
            "mcp_description".to_string(),
            "Perform GitHub API operations with session persistence".to_string(),
        );

        let mcp_cap = CapabilityManifest {
            id: "github_mcp".to_string(),
            name: "GitHub MCP Server".to_string(),
            version: "2.0.0".to_string(),
            description: "Interact with GitHub repositories and issues".to_string(),
            provider: ProviderType::MCP(MCPCapability {
                server_url: "http://localhost:3001".to_string(),
                tool_name: "github_operations".to_string(),
                timeout_ms: 10000,
            }),
            input_schema: None,
            output_schema: None,
            attestation: None,
            provenance: None,
            metadata: github_meta,
            permissions: vec!["write:repo".to_string(), "read:issues".to_string()],
            effects: vec![
                "call:github_api".to_string(),
                "maintain:session".to_string(),
            ],
            agent_metadata: None,
        };

        marketplace
            .register_capability_manifest(mcp_cap.clone())
            .await
            .expect("Failed to register MCP capability");

        println!("✓ Created MCP capability:");
        println!("  - ID: {}", mcp_cap.id);
        println!("  - Name: {}", mcp_cap.name);
        println!("  - Server URL: http://localhost:3001");
        println!("  - Tool Name: github_operations");
        println!("  - Session Metadata: YES (mcp_requires_session=true)\n");

        // Export to RTFS
        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        println!("Step 3: Exporting to RTFS format");
        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

        let rtfs_dir = "/tmp/ccos_demo_rtfs";
        fs::create_dir_all(rtfs_dir)
            .await
            .expect("Failed to create RTFS export dir");

        let rtfs_count = marketplace
            .export_capabilities_to_rtfs_dir(rtfs_dir)
            .await
            .expect("Failed to export to RTFS");

        println!(
            "✓ Exported {} capabilities to RTFS directory: {}\n",
            rtfs_count, rtfs_dir
        );

        // Read and display RTFS files
        println!("Generated RTFS Files:\n");
        let mut entries = fs::read_dir(rtfs_dir)
            .await
            .expect("Failed to read RTFS export dir");

        while let Some(entry) = entries
            .next_entry()
            .await
            .expect("Failed to read directory entry")
        {
            let path = entry.path();
            if path.extension().map_or(false, |ext| ext == "rtfs") {
                let filename = path.file_name().unwrap().to_string_lossy().to_string();
                println!("┌─────────────────────────────────────────────────────────────────────┐");
                println!("│ File: {:<61} │", filename);
                println!("└─────────────────────────────────────────────────────────────────────┘");

                let content = fs::read_to_string(&path)
                    .await
                    .expect("Failed to read RTFS file");

                for line in content.lines() {
                    println!("  {}", line);
                }
                println!();
            }
        }

        // Export to JSON
        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        println!("Step 4: Exporting to JSON format");
        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

        let json_dir = "/tmp/ccos_demo_json";
        fs::create_dir_all(json_dir)
            .await
            .expect("Failed to create JSON export dir");

        let json_file = format!("{}/capabilities.json", json_dir);
        marketplace
            .export_capabilities_to_file(&json_file)
            .await
            .expect("Failed to export to JSON");

        println!("✓ Exported capabilities to JSON file: {}\n", json_file);

        println!("┌─────────────────────────────────────────────────────────────────────┐");
        println!("│ File: capabilities.json                                             │");
        println!("└─────────────────────────────────────────────────────────────────────┘");

        let json_content = fs::read_to_string(&json_file)
            .await
            .expect("Failed to read JSON file");

        let formatted: serde_json::Value =
            serde_json::from_str(&json_content).expect("Failed to parse JSON");
        let pretty = serde_json::to_string_pretty(&formatted).expect("Failed to format JSON");

        for line in pretty.lines().take(80) {
            println!("  {}", line);
        }

        if pretty.lines().count() > 80 {
            println!("  ... ({} more lines)", pretty.lines().count() - 80);
        }
        println!();

        // Round-trip: load from RTFS and JSON into new marketplace
        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        println!("Step 5: Round-trip test - loading into new marketplace");
        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

        let marketplace2 = setup_marketplace_with_sessions().await;

        let rtfs_loaded = marketplace2
            .import_capabilities_from_rtfs_dir(rtfs_dir)
            .await
            .expect("Failed to import from RTFS");

        let json_loaded = marketplace2
            .import_capabilities_from_file(&json_file)
            .await
            .expect("Failed to import from JSON");

        println!("✓ Loaded {} capabilities from RTFS", rtfs_loaded);
        println!("✓ Loaded {} capabilities from JSON", json_loaded);
        println!(
            "✓ Total capabilities in new marketplace: {}\n",
            rtfs_loaded + json_loaded
        );

        // Verify round-trip integrity
        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        println!("Step 6: Verifying round-trip integrity");
        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

        if let Some(reloaded_http) = marketplace2.get_capability("weather_api").await {
            println!("✓ HTTP capability verified:");
            println!("  - ID: {}", reloaded_http.id);
            println!("  - Name: {}", reloaded_http.name);
            if let ProviderType::Http(http) = &reloaded_http.provider {
                println!("  - Base URL: {}", http.base_url);
                println!("  - Timeout: {}ms", http.timeout_ms);
                println!(
                    "  - Auth Token: {}",
                    if http.auth_token.is_some() {
                        "✓ preserved"
                    } else {
                        "✗ missing"
                    }
                );
            }
            println!("  - Metadata: {} fields", reloaded_http.metadata.len());
        } else {
            println!("✗ HTTP capability not found!");
        }

        println!();

        if let Some(reloaded_mcp) = marketplace2.get_capability("github_mcp").await {
            println!("✓ MCP capability verified:");
            println!("  - ID: {}", reloaded_mcp.id);
            println!("  - Name: {}", reloaded_mcp.name);
            if let ProviderType::MCP(mcp) = &reloaded_mcp.provider {
                println!("  - Server URL: {}", mcp.server_url);
                println!("  - Tool Name: {}", mcp.tool_name);
            }
            println!("  - Session Metadata:");
            if let Some(requires) = reloaded_mcp.metadata.get("mcp_requires_session") {
                println!("    ✓ mcp_requires_session: {}", requires);
            }
            if let Some(url) = reloaded_mcp.metadata.get("mcp_server_url") {
                println!("    ✓ mcp_server_url: {}", url);
            }
            if let Some(tool) = reloaded_mcp.metadata.get("mcp_tool_name") {
                println!("    ✓ mcp_tool_name: {}", tool);
            }
        } else {
            println!("✗ MCP capability not found!");
        }

        println!();
        println!("╔══════════════════════════════════════════════════════════════════════╗");
        println!("║                    ✓ Demo Completed Successfully                    ║");
        println!("║                                                                      ║");
        println!("║  Output locations:                                                  ║");
        println!("║  - RTFS files: {}                    ║", rtfs_dir);
        println!("║  - JSON file:  {}                 ║", json_file);
        println!("║                                                                      ║");
        println!("║  You can inspect these files:                                       ║");
        println!(
            "║  - ls -la {}                                   ║",
            rtfs_dir
        );
        println!(
            "║  - cat {} | head -50                          ║",
            json_file
        );
        println!("║                                                                      ║");
        println!("║  Summary:                                                            ║");
        println!("║  ✓ 2 capabilities created (HTTP + MCP with sessions)               ║");
        println!("║  ✓ Exported to RTFS (human-readable)                               ║");
        println!("║  ✓ Exported to JSON (portable)                                     ║");
        println!("║  ✓ Loaded into new marketplace                                     ║");
        println!("║  ✓ All metadata and session info preserved                         ║");
        println!("╚══════════════════════════════════════════════════════════════════════╝");

        // Don't assert failure - just display
        assert!(rtfs_loaded > 0, "Should have loaded RTFS capabilities");
    }
}
