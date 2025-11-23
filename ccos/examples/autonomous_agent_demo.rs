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
use ccos::catalog::CatalogService;
use ccos::planner::capabilities_v2::register_planner_capabilities_v2;
use ccos::capabilities::defaults::register_default_capabilities;
use ccos::capability_marketplace::CapabilityMarketplace;
use ccos::capability_marketplace::mcp_discovery::{MCPDiscoveryProvider, MCPServerConfig};
use rtfs::runtime::values::Value;
use rtfs::runtime::error::RuntimeResult;
use clap::Parser;
use ccos::CCOS;
use rtfs::runtime::security::RuntimeContext;

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

    /// Enable delegation (required for planner.synthesize)
    #[clap(long)]
    delegation: bool,
}

// Helper to recursively find .rtfs files
fn find_rtfs_files(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                files.extend(find_rtfs_files(&path));
            } else if path.extension().map_or(false, |ext| ext == "rtfs") {
                files.push(path);
            }
        }
    }
    files
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    
    println!("🤖 Autonomous Agent Demo");
    println!("========================");

    // Enable delegation if requested
    if args.delegation {
        std::env::set_var("CCOS_ENABLE_DELEGATION", "1");
        // Ensure we have a model set if not present
        if std::env::var("CCOS_DELEGATING_MODEL").is_err() {
             std::env::set_var("CCOS_DELEGATING_MODEL", "gpt-4o");
        }
    }
    
    // 1. Setup CCOS
    // We use the high-level CCOS struct which provides access to Arbiter, Catalog, etc.
    println!("Initializing CCOS...");

    // Load config if provided
    let agent_config = if let Some(config_path) = &args.config {
        println!("Loading config from {:?}", config_path);
        let content = std::fs::read_to_string(config_path)?;
        toml::from_str::<rtfs::config::types::AgentConfig>(&content)?
    } else {
        rtfs::config::types::AgentConfig::default()
    };

    // If delegation is enabled in config or args, ensure model env var is set
    // This prevents panic in CCOS::new when delegation is enabled but no model is specified
    let delegation_enabled = args.delegation || agent_config.delegation.enabled.unwrap_or(false);
    
    if delegation_enabled {
         // Ensure we have a model set if not present
         if std::env::var("CCOS_DELEGATING_MODEL").is_err() {
             let default_model = "google/gemini-2.5-flash-lite";
             std::env::set_var("CCOS_DELEGATING_MODEL", default_model);
             println!("Delegation enabled but CCOS_DELEGATING_MODEL not set. Defaulting to '{}'", default_model);
         }
         // Also ensure enable flag is set for consistency, though config overrides it inside CCOS
         std::env::set_var("CCOS_ENABLE_DELEGATION", "1");
    }

    let ccos = Arc::new(CCOS::new_with_agent_config_and_configs_and_debug_callback(
        ccos::intent_graph::config::IntentGraphConfig::default(),
        None,
        Some(agent_config),
        None
    ).await?);
    
    let marketplace = ccos.get_capability_marketplace();
    let catalog = ccos.get_catalog();
    
    // 2. Register Capabilities
    println!("Registering capabilities...");
    
    // Register defaults (includes ccos.user.ask, ccos.io.log, etc.)
    register_default_capabilities(&marketplace).await?;
    
    // Register tool/log alias manually for backward compatibility/LLM preference
    marketplace.register_local_capability(
        "tool/log".to_string(),
        "Log".to_string(),
        "Log message".to_string(),
        Arc::new(|input| {
            // input is usually a Value::Vector for tool calls from RTFS like (tool/log "a" "b")
            // but if called via `execute_capability` with single arg it might be that arg.
            let args = match input {
                Value::Vector(v) => v.clone(),
                Value::List(l) => l.clone(),
                v => vec![v.clone()],
            };
            let message = args
                .iter()
                .map(|v| format!("{:?}", v))
                .collect::<Vec<_>>()
                .join(" ");
            println!("[CCOS-LOG] {}", message);
            Ok(rtfs::runtime::values::Value::Nil)
        }),
    ).await?;
    
    // Register planner capabilities (v2)
    // This requires the CCOS instance for DelegatingArbiter access
    register_planner_capabilities_v2(marketplace.clone(), catalog.clone(), ccos.clone()).await?;
    
    // Register capabilities based on mode
    if args.mock || args.config.is_none() {
        println!("Registering mock capabilities...");
        register_mock_capabilities(&marketplace).await?;
    } else {
        println!("Discovering real MCP capabilities...");
        // Note: CCOS already bootstraps marketplace, so we might check if we need to do extra discovery
        // But for this demo, we run the explicit discovery function if needed, 
        // or assume CCOS bootstrap handled it if config was loaded.
        // Since we didn't pass config to CCOS::new (it uses default), we might need to discover manually 
        // if we want specific ones not in default config.
        // But let's stick to the demo logic:
        discover_and_register_mcp_capabilities(&marketplace, &catalog).await?;
    }

    // 3. Execute RTFS Script
    // First create the root intent referenced by the plan
    {
        let intent_graph = ccos.get_intent_graph();
        let mut ig = intent_graph.lock().unwrap();
        let root_intent = ccos::types::StorableIntent::new("Run autonomous agent loop".to_string());
        // Override ID to match what we use in the plan
        let mut root_intent = root_intent;
        root_intent.intent_id = "root-intent".to_string();
        ig.store_intent(root_intent)?;
    }

    let script_path = "ccos/examples/autonomous_agent.rtfs";
    println!("Reading script: {}", script_path);
    let script_content = std::fs::read_to_string(script_path)?;
    
    println!("Executing agent loop via CCOS Orchestrator...");
    
    // Wrap the script in a Plan
    let plan = ccos::types::Plan {
        plan_id: "autonomous-agent-loop".to_string(),
        name: Some("Autonomous Agent Loop".to_string()),
        body: ccos::types::PlanBody::Rtfs(script_content),
        intent_ids: vec!["root-intent".to_string()],
        ..Default::default()
    };
    
    let context = RuntimeContext::full();
    
    // Execute via CCOS
    let result = ccos.validate_and_execute_plan(plan, &context).await?;
    
    println!("Agent loop finished.");
    println!("Success: {}", result.success);
    if !result.success {
        if let Some(err) = result.metadata.get("error") {
            println!("Error: {:?}", err);
        }
    }
    
    Ok(())
}

async fn discover_and_register_mcp_capabilities(
    marketplace: &Arc<CapabilityMarketplace>,
    catalog: &Arc<CatalogService>,
) -> Result<(), Box<dyn std::error::Error>> {
    use ccos::catalog::CatalogSource;
    
    println!("Starting MCP capability discovery from disk...");
    
    let mcp_provider = MCPDiscoveryProvider::new(
        MCPServerConfig {
            name: "local_mcp_cache".to_string(),
            endpoint: "http://dummy".to_string(),
            ..Default::default()
        }
    )?;
    
    let mcp_dir = std::path::Path::new("capabilities/discovered/mcp");
    let mut all_caps = Vec::new();
    
    if mcp_dir.exists() {
        println!("Scanning MCP dir: {:?}", mcp_dir);
        let files = find_rtfs_files(mcp_dir);
        println!("Found {} RTFS files", files.len());
        for file in files {
            println!("Loading file: {:?}", file);
            match mcp_provider.load_rtfs_capabilities(file.to_str().unwrap()) {
                Ok(module) => {
                    println!("Loaded module with {} capabilities", module.capabilities.len());
                    for cap_def in module.capabilities {
                        match mcp_provider.rtfs_to_capability_manifest(&cap_def) {
                            Ok(manifest) => {
                                println!("Parsed manifest: {}", manifest.id);
                                all_caps.push(manifest);
                            }
                            Err(e) => println!("Failed to convert manifest: {}", e),
                        }
                    }
                }
                Err(e) => println!("Failed to load RTFS: {}", e),
            }
        }
    } else {
        println!("MCP directory not found: {:?}", mcp_dir);
    }

    let mcp_caps: Vec<_> = all_caps.iter()
        .filter(|manifest| {
            manifest.id.starts_with("github-mcp") || 
            manifest.id.starts_with("mcp.")
        })
        .cloned()
        .collect();
    
    println!("Discovered {} MCP capabilities", mcp_caps.len());
    
    for manifest in mcp_caps {
        // Register in marketplace for execution
        marketplace.register_capability_manifest(manifest.clone()).await?;
        
        // Register in catalog for search
        catalog.register_capability(&manifest, CatalogSource::Discovered);
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
