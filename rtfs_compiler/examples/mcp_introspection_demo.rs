//! MCP Introspection → CCOS Capability Demo
//!
//! This example:
//! - Introspects an MCP server using JSON-RPC `tools/list`
//! - Registers each discovered tool as a CCOS capability in the CapabilityMarketplace
//! - Executes a selected tool via CCOS marketplace, with optional JSON args
//!
//! Usage examples:
//!   cargo run --example mcp_introspection_demo -- --server-url http://localhost:3000 --list
//!   cargo run --example mcp_introspection_demo -- --server-url http://localhost:3000 --tool my_tool --args '{"query":"hello"}'

use clap::Parser;
use reqwest::Client;
use rtfs_compiler::runtime::capability_marketplace::{CapabilityMarketplace, CapabilityIsolationPolicy};
use rtfs_compiler::runtime::capability_registry::CapabilityRegistry;
use rtfs_compiler::runtime::values::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Parser, Debug)]
#[command(name = "mcp_introspection_demo")] 
#[command(about = "Introspect an MCP server, register tools as CCOS capabilities, and execute one.")]
struct Args {
    /// MCP server URL (JSON-RPC endpoint)
    #[arg(long)]
    server_url: String,

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
}

#[derive(Debug, Clone)]
struct DiscoveredTool { name: String, description: Option<String> }

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    // Build CCOS environment: CapabilityRegistry + CapabilityMarketplace
    let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
    let mut marketplace = CapabilityMarketplace::new(registry);
    let mut policy = CapabilityIsolationPolicy::default();
    policy.allowed_capabilities = vec!["mcp.*".to_string()];
    marketplace.set_isolation_policy(policy);

    // Introspect MCP tools
    println!("🔎 Introspecting MCP server: {}", args.server_url);
    let tools = list_mcp_tools(&args.server_url, args.timeout_ms).await?;
    if tools.is_empty() {
        println!("No tools discovered.");
        return Ok(());
    }

    println!("Discovered {} tool(s):", tools.len());
    for t in &tools { println!("  - {}{}", t.name, t.description.as_ref().map(|d| format!(" — {}", d)).unwrap_or_default()); }

    // Register each discovered tool as a CCOS capability using MCP executor
    for t in &tools {
        let id = format!("mcp.demo.{}", t.name);
        let name = t.name.clone();
        let description = t.description.clone().unwrap_or_else(|| format!("MCP tool '{}'", name));
        marketplace.register_mcp_capability(
            id,
            name.clone(),
            description,
            args.server_url.clone(),
            name,
            args.timeout_ms,
        ).await?;
    }

    if args.list || args.tool.is_none() { return Ok(()); }

    // Execute selected tool
    let tool = args.tool.unwrap();
    let capability_id = format!("mcp.demo.{}", &tool);

    // Build inputs Value from JSON string if provided, otherwise empty map
    let inputs: Value = if let Some(json) = args.args {
        let parsed: serde_json::Value = serde_json::from_str(&json)?;
        rtfs_compiler::runtime::capability_marketplace::CapabilityMarketplace::json_to_rtfs_value(&parsed)?
    } else {
        Value::Map(HashMap::new())
    };

    println!("\n🚀 Executing capability '{}' with inputs: {:?}", capability_id, inputs);
    let result = marketplace.execute_capability(&capability_id, &inputs).await?;

    println!("\n✅ Result: {:?}", result);
    Ok(())
}

async fn list_mcp_tools(server_url: &str, timeout_ms: u64) -> Result<Vec<DiscoveredTool>, Box<dyn std::error::Error>> {
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
    if let Some(error) = json.get("error") { return Err(format!("MCP error: {}", error).into()); }

    let mut tools = Vec::new();
    if let Some(result) = json.get("result") {
        if let Some(arr) = result.get("tools").and_then(|v| v.as_array()) {
            for t in arr {
                let name = t.get("name").and_then(|v| v.as_str()).unwrap_or_default().to_string();
                let description = t.get("description").and_then(|v| v.as_str()).map(|s| s.to_string());
                if !name.is_empty() { tools.push(DiscoveredTool { name, description }); }
            }
        }
    }
    Ok(tools)
}
