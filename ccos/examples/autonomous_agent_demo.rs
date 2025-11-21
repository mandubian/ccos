//! Autonomous Agent Demo
//!
//! This example demonstrates a self-evolving autonomous agent loop using RTFS and CCOS capabilities.
//! The agent:
//! 1. Builds a menu of capabilities relevant to a goal.
//! 2. Synthesizes a plan (steps) to achieve the goal.
//! 3. Validates the plan.
//! 4. Executes the plan steps dynamically.
//! 5. Asks the user for the next goal.
//!
//! Usage:
//!   cargo run --example autonomous_agent_demo -- --config config/agent_config.toml
//!   cargo run --example autonomous_agent_demo (uses mock capabilities)

use std::sync::Arc;
use std::path::PathBuf;
use ccos::environment::{CCOSConfig, CCOSEnvironment};
use ccos::catalog::CatalogService;
use ccos::planner::capabilities_v2::register_planner_capabilities_v2;
use ccos::capabilities::defaults::register_default_capabilities;
use ccos::capability_marketplace::CapabilityMarketplace;
use rtfs::runtime::values::Value;
use rtfs::runtime::error::RuntimeResult;
use clap::Parser;

#[derive(Parser, Debug)]
#[clap(name = "autonomous_agent_demo")]
#[clap(about = "Autonomous agent with capability discovery")]
struct Args {
    /// Path to agent config TOML file
    #[clap(long)]
    config: Option<PathBuf>,
    
    /// Use mock capabilities instead of real MCP discovery
    #[clap(long)]
    mock: bool,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    
    println!("🤖 Autonomous Agent Demo");
    println!("========================");
    
    // 1. Setup Environment
    let config = if let Some(config_path) = &args.config {
        println!("Loading config from: {}", config_path.display());
        // For now, use default and suggest user to update CCOSConfig with from_file
        // TODO: Implement CCOSConfig::from_file or load TOML manually
        println!("NOTE: Config loading not yet implemented, using defaults");
        CCOSConfig::default()
    } else {
        println!("Using default config (no MCP servers)");
        CCOSConfig::default()
    };
    
    let env = CCOSEnvironment::new(config)?;
    
    // 2. Setup Catalog
    let catalog = Arc::new(CatalogService::new());
    
    // 3. Register Capabilities
    let marketplace = env.marketplace().clone();
    
    // Create a temporary runtime for async registration
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        // Register defaults (includes ccos.user.ask, ccos.io.log, etc.)
        register_default_capabilities(&marketplace).await?;
        
        // Register planner capabilities (v2)
        register_planner_capabilities_v2(marketplace.clone(), catalog.clone()).await?;
        
        // Register capabilities based on mode
        if args.mock || args.config.is_none() {
            println!("Registering mock capabilities...");
            register_mock_capabilities(&marketplace).await?;
        } else {
            println!("Discovering real MCP capabilities...");
            discover_and_register_mcp_capabilities(&marketplace, &catalog).await?;
        }
        
        Ok::<(), Box<dyn std::error::Error>>(())
    })?;
    
    // 4. Execute RTFS Script
    let script_path = "ccos/examples/autonomous_agent.rtfs";
    println!("Executing script: {}", script_path);
    
    // We use execute_file from CCOSEnvironment
    match env.execute_file(script_path) {
        Ok(outcome) => {
            println!("Script execution finished.");
            println!("Outcome: {:?}", outcome);
        }
        Err(e) => {
            eprintln!("Script execution failed: {}", e);
        }
    }
    
    Ok(())
}

async fn discover_and_register_mcp_capabilities(
    marketplace: &Arc<CapabilityMarketplace>,
    catalog: &Arc<CatalogService>,
) -> Result<(), Box<dyn std::error::Error>> {
    use ccos::catalog::CatalogSource;
    
    println!("Starting MCP capability discovery...");
    
    // The marketplace already discovered capabilities during initialization
    let all_caps = marketplace.list_capabilities().await;
    let mcp_caps: Vec<_> = all_caps.iter()
        .filter(|manifest| {
            manifest.id.starts_with("github-mcp_") || 
            manifest.id.starts_with("mcp_")
        })
        .collect();
    
    println!("Discovered {} MCP capabilities", mcp_caps.len());
    
    // Register them in the catalog for semantic search
    for manifest in &mcp_caps {
        catalog.register_capability(manifest, CatalogSource::Discovered);
        println!("  ✓ {}", manifest.id);
    }
    
    Ok(())
}

async fn register_mock_capabilities(marketplace: &Arc<CapabilityMarketplace>) -> RuntimeResult<()> {
    // discovery.search
    marketplace.register_local_capability(
        "discovery.search".to_string(),
        "Discovery Search".to_string(),
        "Searches for information".to_string(),
        Arc::new(|_input| {
             println!("[discovery.search] Searching...");
             // Mock result
             Ok(Value::String("Found 3 results: file1.rs, file2.rs, README.md".to_string()))
        })
    ).await?;
    
    // analysis.analyze_imports
    marketplace.register_local_capability(
        "analysis.analyze_imports".to_string(),
        "Analyze Imports".to_string(),
        "Analyzes imports in the workspace".to_string(),
        Arc::new(|_input| {
             println!("[analysis.analyze_imports] Analyzing...");
             // Mock result
             Ok(Value::String("Imports analyzed: 150 imports found.".to_string()))
        })
    ).await?;

    // github.list_issues
    marketplace.register_local_capability(
        "github.list_issues".to_string(),
        "List GitHub Issues".to_string(),
        "Lists issues from a GitHub repository".to_string(),
        Arc::new(|_input| {
             println!("[github.list_issues] Listing issues...");
             // Mock result
             Ok(Value::String("Found 5 open issues: #1 Bug fix, #2 Feature request...".to_string()))
        })
    ).await?;
    
    Ok(())
}
