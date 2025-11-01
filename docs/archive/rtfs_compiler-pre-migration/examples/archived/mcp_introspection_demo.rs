//! MCP Introspection → CCOS Capability Demo
//!
//! This example:
//! - Introspects an MCP server using JSON-RPC `tools/list`
//! - Converts discovered tools to RTFS capability format for persistence
//! - Saves/loads RTFS capability definitions to/from files
//! - Registers each discovered tool as a CCOS capability in the CapabilityMarketplace
//! - Executes a selected tool via CCOS marketplace, with optional JSON args
//!
//! Usage examples:
//!   cargo run --example mcp_introspection_demo -- --server-url http://localhost:3000 --list
//!   cargo run --example mcp_introspection_demo -- --server-url http://localhost:3000 --tool my_tool --args '{"query":"hello"}'
//!   cargo run --example mcp_introspection_demo -- --server-url http://localhost:3000 --save-rtfs capabilities.json
//!   cargo run --example mcp_introspection_demo -- --load-rtfs capabilities.json --list
//!   cargo run --example mcp_introspection_demo -- --server-url http://localhost:3000 --show-rtfs

use clap::Parser;
use reqwest::Client;
use rtfs_compiler::ccos::capability_marketplace::mcp_discovery::{
    MCPDiscoveryBuilder, MCPDiscoveryProvider,
};
use rtfs_compiler::ccos::capability_marketplace::{
    CapabilityIsolationPolicy, CapabilityMarketplace,
};
use rtfs_compiler::runtime::capabilities::registry::CapabilityRegistry;
use rtfs_compiler::runtime::values::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Parser, Debug)]
#[command(name = "mcp_introspection_demo")]
#[command(
    about = "Introspect an MCP server, register tools as CCOS capabilities, and execute one."
)]
struct Args {
    /// MCP server URL (JSON-RPC endpoint) - not required when loading from RTFS
    #[arg(long)]
    server_url: Option<String>,

    /// Tool name to execute (if omitted, just list tools)
    #[arg(long)]
    tool: Option<String>,

    /// JSON arguments to pass to the tool (string, will be parsed)
    #[arg(long)]
    args: Option<String>,

    /// Timeout in milliseconds for MCP calls
    #[arg(long, default_value_t = 5000)]
    timeout_ms: u64,

    /// If set, only lists discovered tools and exits
    #[arg(long, default_value_t = false)]
    list: bool,

    /// Save discovered capabilities to RTFS format file
    #[arg(long)]
    save_rtfs: Option<String>,

    /// Load capabilities from RTFS format file instead of introspecting
    #[arg(long)]
    load_rtfs: Option<String>,

    /// Show RTFS format for discovered capabilities
    #[arg(long, default_value_t = false)]
    show_rtfs: bool,
}

#[derive(Debug, Clone)]
struct DiscoveredTool {
    name: String,
    description: Option<String>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    // Handle loading from RTFS file
    let (tools, rtfs_module) = if let Some(rtfs_file) = &args.load_rtfs {
        println!("📁 Loading capabilities from RTFS file: {}", rtfs_file);
        // Create a temporary provider for loading
        let temp_provider = MCPDiscoveryBuilder::new()
            .name("temp_loader".to_string())
            .endpoint("http://localhost:3000".to_string())
            .build()?;
        let module = temp_provider.load_rtfs_capabilities(rtfs_file)?;
        println!(
            "Loaded {} capability definitions from RTFS module",
            module.capabilities.len()
        );

        // For RTFS loading, we'll handle registration differently
        // Return empty tools list and the module
        (Vec::new(), Some(module))
    } else {
        // Introspect MCP tools
        let server_url = args
            .server_url
            .as_ref()
            .ok_or("Server URL is required when not loading from RTFS")?;
        println!("🔎 Introspecting MCP server: {}", server_url);
        let discovered_tools = list_mcp_tools(server_url, args.timeout_ms).await?;
        if discovered_tools.is_empty() {
            println!("No tools discovered.");
            return Ok(());
        }
        (discovered_tools, None)
    };

    if let Some(ref module) = rtfs_module {
        println!("Available {} tool(s):", module.capabilities.len());
        for rtfs_cap in &module.capabilities {
            println!("  - {} — {}", "name", "description"); // Placeholder for now
        }
    } else {
        println!("Available {} tool(s):", tools.len());
        for t in &tools {
            println!(
                "  - {}{}",
                t.name,
                t.description
                    .as_ref()
                    .map(|d| format!(" — {}", d))
                    .unwrap_or_default()
            );
        }
    }

    // Handle RTFS format display
    if args.show_rtfs {
        println!("\n📋 Converting to RTFS format...");
        let server_url = args
            .server_url
            .clone()
            .unwrap_or_else(|| "http://localhost:3000".to_string());
        let discovery_provider = MCPDiscoveryBuilder::new()
            .name("demo_server".to_string())
            .endpoint(server_url)
            .timeout_seconds((args.timeout_ms / 1000) as u64)
            .build()?;

        // Convert tools to MCPTool format for conversion
        let mcp_tools: Vec<_> = tools
            .iter()
            .map(
                |t| rtfs_compiler::ccos::capability_marketplace::mcp_discovery::MCPTool {
                    name: t.name.clone(),
                    description: t.description.clone(),
                    input_schema: None,
                    output_schema: None,
                    metadata: None,
                    annotations: None,
                },
            )
            .collect();

        let rtfs_capabilities = discovery_provider.convert_tools_to_rtfs_format(&mcp_tools)?;

        println!("RTFS Capability Definitions:");
        for (i, rtfs_cap) in rtfs_capabilities.iter().enumerate() {
            println!("\n--- Capability {} ---", i + 1);
            println!("Capability: {:?}", rtfs_cap.capability);
            if let Some(input_schema) = &rtfs_cap.input_schema {
                println!("Input Schema: {:?}", input_schema);
            }
            if let Some(output_schema) = &rtfs_cap.output_schema {
                println!("Output Schema: {:?}", output_schema);
            }
        }
    }

    // Handle saving to RTFS file
    if let Some(save_path) = &args.save_rtfs {
        println!("\n💾 Saving capabilities to RTFS file: {}", save_path);
        let server_url = args
            .server_url
            .clone()
            .unwrap_or_else(|| "http://localhost:3000".to_string());
        let discovery_provider = MCPDiscoveryBuilder::new()
            .name("demo_server".to_string())
            .endpoint(server_url)
            .timeout_seconds((args.timeout_ms / 1000) as u64)
            .build()?;

        // Convert tools to MCPTool format for conversion
        let mcp_tools: Vec<_> = tools
            .iter()
            .map(
                |t| rtfs_compiler::ccos::capability_marketplace::mcp_discovery::MCPTool {
                    name: t.name.clone(),
                    description: t.description.clone(),
                    input_schema: None,
                    output_schema: None,
                    metadata: None,
                    annotations: None,
                },
            )
            .collect();

        let rtfs_capabilities = discovery_provider.convert_tools_to_rtfs_format(&mcp_tools)?;
        discovery_provider.save_rtfs_capabilities(&rtfs_capabilities, save_path)?;
        println!(
            "✅ Successfully saved {} capabilities to {}",
            rtfs_capabilities.len(),
            save_path
        );
    }

    // Build CCOS environment: CapabilityRegistry + CapabilityMarketplace
    let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
    let mut marketplace = CapabilityMarketplace::new(registry);
    let mut policy = CapabilityIsolationPolicy::default();
    policy.allowed_capabilities = vec!["mcp.*".to_string()];
    marketplace.set_isolation_policy(policy);

    // Register capabilities - handle RTFS modules differently from fresh introspection
    if let Some(ref module) = rtfs_module {
        // Register RTFS capabilities using their original IDs
        let discovery_provider = MCPDiscoveryProvider::from_rtfs_module(module)?;
        for rtfs_cap in &module.capabilities {
            let manifest = discovery_provider.rtfs_to_capability_manifest(rtfs_cap)?;
            println!("Registering RTFS capability: {}", manifest.id);

            // Use the original server endpoint from the RTFS module
            let server_url = module.server_config.endpoint.clone();
            marketplace
                .register_mcp_capability(
                    manifest.id,
                    manifest.name.clone(),
                    manifest.description,
                    server_url,
                    manifest.name, // tool name
                    (module.server_config.timeout_seconds * 1000) as u64,
                )
                .await?;
        }
    } else {
        // Register freshly introspected tools
        for t in &tools {
            let id = format!("mcp.demo.{}", t.name);
            let name = t.name.clone();
            let description = t
                .description
                .clone()
                .unwrap_or_else(|| format!("MCP tool '{}'", name));
            let server_url = args
                .server_url
                .clone()
                .unwrap_or_else(|| "http://localhost:3000".to_string());
            marketplace
                .register_mcp_capability(
                    id,
                    name.clone(),
                    description,
                    server_url,
                    name,
                    args.timeout_ms,
                )
                .await?;
        }
    }

    // If only listing or no tool specified, and not saving/showing RTFS, exit early
    if args.list || (args.tool.is_none() && args.save_rtfs.is_none() && !args.show_rtfs) {
        return Ok(());
    }

    // If we have a tool to execute, proceed with execution
    if let Some(tool) = args.tool {
        // Use the tool name as-is if loading from RTFS (it already has the full ID)
        // Otherwise, add the mcp.demo prefix for freshly introspected tools
        let capability_id = if rtfs_module.is_some() {
            tool.clone()
        } else {
            format!("mcp.demo.{}", &tool)
        };

        // Build inputs Value from JSON string if provided, otherwise empty map
        let inputs: Value = if let Some(json) = args.args {
            let parsed: serde_json::Value = serde_json::from_str(&json)?;
            rtfs_compiler::ccos::capability_marketplace::CapabilityMarketplace::json_to_rtfs_value(
                &parsed,
            )?
        } else {
            Value::Map(HashMap::new())
        };

        println!(
            "\n🚀 Executing capability '{}' with inputs: {:?}",
            capability_id, inputs
        );
        let result = marketplace
            .execute_capability(&capability_id, &inputs)
            .await?;

        println!("\n✅ Result: {}", result);
    }
    Ok(())
}

async fn list_mcp_tools(
    server_url: &str,
    timeout_ms: u64,
) -> Result<Vec<DiscoveredTool>, Box<dyn std::error::Error>> {
    let client = Client::new();
    let req = serde_json::json!({
        "jsonrpc": "2.0",
        "id": "tools_discovery",
        "method": "tools/list",
        "params": {}
    });
    let resp = client
        .post(server_url)
        .json(&req)
        .timeout(std::time::Duration::from_millis(timeout_ms))
        .send()
        .await?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("MCP tools/list HTTP {}: {}", status, body).into());
    }
    let json: serde_json::Value = resp.json().await?;
    if let Some(error) = json.get("error") {
        return Err(format!("MCP error: {}", error).into());
    }

    let mut tools = Vec::new();
    if let Some(result) = json.get("result") {
        if let Some(arr) = result.get("tools").and_then(|v| v.as_array()) {
            for t in arr {
                let name = t
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string();
                let description = t
                    .get("description")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                if !name.is_empty() {
                    tools.push(DiscoveredTool { name, description });
                }
            }
        }
    }
    Ok(tools)
}
