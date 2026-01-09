use ccos::capability_marketplace::mcp_discovery::MCPServerConfig;
use ccos::mcp::core::MCPDiscoveryService;
use ccos::mcp::types::DiscoveryOptions;
use rtfs::runtime::error::RuntimeResult;

#[tokio::main]
async fn main() -> RuntimeResult<()> {
    println!("🚀 Starting MCP Stdio Transport Verification...");

    // Configure a stdio-based server (Puppeteer)
    let config = MCPServerConfig {
        name: "puppeteer-test".to_string(),
        endpoint: "npx -y @modelcontextprotocol/server-puppeteer".to_string(),
        auth_token: None,
        timeout_seconds: 60,
        protocol_version: "2024-11-05".to_string(),
    };

    // Initialize discovery service
    let discovery_service = MCPDiscoveryService::new();

    let workspace_root = ccos::utils::fs::get_workspace_root();
    let pending_dir = workspace_root.join("capabilities/servers/pending/puppeteer-reg-v2");

    let options = DiscoveryOptions {
        introspect_output_schemas: true,
        use_cache: false,
        export_to_rtfs: true,
        export_directory: Some(pending_dir.to_string_lossy().to_string()),
        non_interactive: true,
        ..Default::default()
    };

    println!(
        "🔍 Introspecting Puppeteer via stdio: '{}'...",
        config.endpoint
    );
    let manifests = discovery_service
        .discover_and_export_tools(&config, &options)
        .await?;

    println!("✅ Successfully discovered {} tools:", manifests.len());
    println!("📂 RTFS files exported to: {}", pending_dir.display());

    for manifest in manifests {
        println!("  - {}: {}", manifest.id, manifest.description);
    }

    Ok(())
}
