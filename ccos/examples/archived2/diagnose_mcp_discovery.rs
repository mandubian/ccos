//! Diagnostic Tool for MCP Discovery Flow
//!
//! This tool systematically diagnoses each step of the MCP discovery pipeline:
//! 1. Session initialization & tools/list
//! 2. File system operations (directory creation, file writing)
//! 3. RTFS file parsing & loading
//! 4. Capability registration in marketplace
//! 5. Capability invocation
//!
//! Usage:
//!   cargo run --example diagnose_mcp_discovery -- \
//!     --server-url https://glama.ai/mcp/github \
//!     --output-dir capabilities/discovered

use clap::Parser;
use std::error::Error;
use std::path::PathBuf;
use std::collections::HashMap;

#[derive(Parser, Debug)]
struct Args {
    /// MCP server URL to test
    #[arg(long)]
    server_url: String,

    /// Output directory for discovered capabilities
    #[arg(long, default_value = "capabilities/discovered")]
    output_dir: String,

    /// Skip session initialization test
    #[arg(long)]
    skip_session: bool,

    /// Skip file operations test
    #[arg(long)]
    skip_file_ops: bool,

    /// Skip RTFS parsing test
    #[arg(long)]
    skip_parsing: bool,

    /// Skip marketplace registration test
    #[arg(long)]
    skip_marketplace: bool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    println!("🔧 MCP Discovery Pipeline Diagnostics");
    println!("======================================\n");

    let args = Args::parse();
    let mut test_results = Vec::new();

    // TEST 1: Session Initialization
    if !args.skip_session {
        println!("🔍 TEST 1: Session Initialization");
        println!("----------------------------------");
        
        match test_session_initialization(&args.server_url).await {
            Ok(session_info) => {
                println!("✅ PASS: Session initialized successfully");
                println!("   Session ID: {:?}", session_info.session_id);
                println!("   Server: {} v{}", session_info.server_name, session_info.server_version);
                println!("   Tools available: {}", session_info.tool_count);
                test_results.push(("Session Init", true, None));
            }
            Err(e) => {
                println!("❌ FAIL: Session initialization failed");
                println!("   Error: {}", e);
                test_results.push(("Session Init", false, Some(e.to_string())));
            }
        }
        println!();
    }

    // TEST 2: File System Operations
    if !args.skip_file_ops {
        println!("🔍 TEST 2: File System Operations");
        println!("----------------------------------");
        
        match test_file_operations(&args.output_dir).await {
            Ok(test_file) => {
                println!("✅ PASS: File operations working");
                println!("   Created directory: {}", args.output_dir);
                println!("   Wrote test file: {}", test_file.display());
                test_results.push(("File Operations", true, None));
            }
            Err(e) => {
                println!("❌ FAIL: File operations failed");
                println!("   Error: {}", e);
                test_results.push(("File Operations", false, Some(e.to_string())));
            }
        }
        println!();
    }

    // TEST 3: RTFS Parsing
    if !args.skip_parsing {
        println!("🔍 TEST 3: RTFS File Parsing");
        println!("-----------------------------");
        
        match test_rtfs_parsing(&args.output_dir).await {
            Ok(cap_count) => {
                println!("✅ PASS: RTFS parsing working");
                println!("   Parsed {} capabilities", cap_count);
                test_results.push(("RTFS Parsing", true, None));
            }
            Err(e) => {
                println!("❌ FAIL: RTFS parsing failed");
                println!("   Error: {}", e);
                test_results.push(("RTFS Parsing", false, Some(e.to_string())));
            }
        }
        println!();
    }

    // TEST 4: Marketplace Registration
    if !args.skip_marketplace {
        println!("🔍 TEST 4: Marketplace Registration");
        println!("------------------------------------");
        
        match test_marketplace_registration(&args.output_dir).await {
            Ok(loaded_count) => {
                println!("✅ PASS: Marketplace registration working");
                println!("   Loaded {} capabilities", loaded_count);
                test_results.push(("Marketplace", true, None));
            }
            Err(e) => {
                println!("❌ FAIL: Marketplace registration failed");
                println!("   Error: {}", e);
                test_results.push(("Marketplace", false, Some(e.to_string())));
            }
        }
        println!();
    }

    // Print summary
    println!("\n📊 Test Summary");
    println!("===============");
    
    let passed = test_results.iter().filter(|(_, pass, _)| *pass).count();
    let failed = test_results.iter().filter(|(_, pass, _)| !*pass).count();
    
    for (name, passed, error) in &test_results {
        let status = if *passed { "✅ PASS" } else { "❌ FAIL" };
        println!("{}: {}", status, name);
        if let Some(err) = error {
            println!("       └─ {}", err);
        }
    }
    
    println!("\nResults: {} passed, {} failed", passed, failed);
    
    if failed > 0 {
        println!("\n⚠️  Discovery pipeline has reliability issues!");
        std::process::exit(1);
    } else {
        println!("\n🎉 All tests passed! Discovery pipeline is healthy.");
    }

    Ok(())
}

struct SessionInfo {
    session_id: Option<String>,
    server_name: String,
    server_version: String,
    tool_count: usize,
}

async fn test_session_initialization(server_url: &str) -> Result<SessionInfo, Box<dyn Error>> {
    use ccos::synthesis::mcp_session::{MCPSessionManager, MCPServerInfo};
    
    // Get auth from environment
    let mut auth_headers = HashMap::new();
    if let Ok(token) = std::env::var("GITHUB_TOKEN") {
        auth_headers.insert("Authorization".to_string(), format!("Bearer {}", token));
    }

    let session_manager = MCPSessionManager::new(Some(auth_headers));
    let client_info = MCPServerInfo {
        name: "diagnose-tool".to_string(),
        version: "1.0.0".to_string(),
    };

    println!("  Initializing session with {}", server_url);
    let session = session_manager
        .initialize_session(server_url, &client_info)
        .await?;

    println!("  Listing available tools...");
    let tools_resp = session_manager
        .make_request(&session, "tools/list", serde_json::json!({}))
        .await?;

    let tool_count = tools_resp
        .get("result")
        .and_then(|r| r.get("tools"))
        .and_then(|t| t.as_array())
        .map(|a| a.len())
        .unwrap_or(0);

    // Clean up
    let _ = session_manager.terminate_session(&session).await;

    Ok(SessionInfo {
        session_id: session.session_id.clone(),
        server_name: session.server_info.name.clone(),
        server_version: session.server_info.version.clone(),
        tool_count,
    })
}

async fn test_file_operations(output_dir: &str) -> Result<PathBuf, Box<dyn Error>> {
    use std::fs;

    let test_dir = PathBuf::from(output_dir).join("_test");
    
    println!("  Creating directory: {}", test_dir.display());
    fs::create_dir_all(&test_dir)?;

    let test_file = test_dir.join("test_capability.rtfs");
    let test_content = r#"; Test RTFS capability file
(def mcp-capabilities-module
  {
    :module-type "ccos.capabilities.mcp:v1"
    :server-config {
      :name "test-server"
      :endpoint "https://example.com"
      :auth-token nil
      :timeout-seconds 5
      :protocol-version "2024-11-05"
    }
    :generated-at "2025-11-21T00:00:00Z"
    :capabilities [
      {
        :capability {:id "mcp.test.tool" :name "Test Tool"}
        :input-schema nil
        :output-schema nil
      }
    ]
  })
"#;

    println!("  Writing test file: {}", test_file.display());
    fs::write(&test_file, test_content)?;

    println!("  Verifying file exists...");
    if !test_file.exists() {
        return Err("Test file not found after writing!".into());
    }

    println!("  Reading file back...");
    let read_content = fs::read_to_string(&test_file)?;
    
    if read_content != test_content {
        return Err("File content mismatch!".into());
    }

    // Cleanup
    println!("  Cleaning up test files...");
    fs::remove_dir_all(&test_dir)?;

    Ok(test_file)
}

async fn test_rtfs_parsing(output_dir: &str) -> Result<usize, Box<dyn Error>> {
    use ccos::capability_marketplace::mcp_discovery::{MCPDiscoveryProvider, MCPServerConfig};
    use std::fs;

    let test_dir = PathBuf::from(output_dir).join("_parsing_test");
    fs::create_dir_all(&test_dir)?;

    let test_file = test_dir.join("parse_test.rtfs");
    let test_content = r#"; Test RTFS capability file
(def mcp-capabilities-module
  {
    :module-type "ccos.capabilities.mcp:v1"
    :server-config {
      :name "github-server"
      :endpoint "https://glama.ai/mcp/github"
      :auth-token nil
      :timeout-seconds 5
      :protocol-version "2024-11-05"
    }
    :generated-at "2025-11-21T00:00:00Z"
    :capabilities [
      {
        :capability {:id "mcp.github.list_issues" :name "List GitHub Issues" :description "List issues in a GitHub repository"}
        :input-schema nil
        :output-schema nil
      }
    ]
  })
"#;

    fs::write(&test_file, test_content)?;

    println!("  Parsing RTFS file...");
    let provider = MCPDiscoveryProvider::new(MCPServerConfig::default())?;
    let module = provider.load_rtfs_capabilities(test_file.to_str().unwrap())?;

    println!("  Module type: {}", module.module_type);
    println!("  Server: {}", module.server_config.name);
    println!("  Capabilities: {}", module.capabilities.len());

    // Try to convert to manifest
    for (idx, cap_def) in module.capabilities.iter().enumerate() {
        println!("  Converting capability {}...", idx + 1);
        let _manifest = provider.rtfs_to_capability_manifest(cap_def)?;
    }

    // Cleanup
    fs::remove_dir_all(&test_dir)?;

    Ok(module.capabilities.len())
}

async fn test_marketplace_registration(output_dir: &str) -> Result<usize, Box<dyn Error>> {
    use std::sync::Arc;
    use ccos::CCOS;
    use std::fs;

    let test_dir = PathBuf::from(output_dir).join("_marketplace_test");
    fs::create_dir_all(&test_dir)?;

    // Create a test RTFS file
    let test_file = test_dir.join("marketplace_test.rtfs");
    let test_content = r#"; Test RTFS capability file
(def mcp-capabilities-module
  {
    :module-type "ccos.capabilities.mcp:v1"
    :server-config {
      :name "test-marketplace"
      :endpoint "https://example.com"
      :auth-token nil
      :timeout-seconds 5
      :protocol-version "2024-11-05"
    }
    :generated-at "2025-11-21T00:00:00Z"
    :capabilities [
      {
        :capability {
          :id "mcp.test.capability"
          :name "Test Capability"
          :description "A test capability for marketplace"
          :version "1.0.0"
          :provider {
            :type "mcp"
            :server_endpoint "https://example.com"
            :tool_name "test_capability"
            :timeout_seconds 5
            :protocol_version "2024-11-05"
          }
          :permissions ["mcp:tool:execute"]
          :effects [":network"]
          :metadata {
            :mcp_server "test-marketplace"
            :mcp_endpoint "https://example.com"
            :tool_name "test_capability"
            :protocol_version "2024-11-05"
          }
        }
        :input-schema nil
        :output-schema nil
      }
    ]
  })
"#;

    fs::write(&test_file, test_content)?;

    // Initialize CCOS and marketplace
    println!("  Initializing CCOS...");
    let ccos = Arc::new(CCOS::new().await?);
    let marketplace = ccos.get_capability_marketplace();

    // Import capabilities
    println!("  Importing capabilities from {}...", test_dir.display());
    let loaded = marketplace
        .import_capabilities_from_rtfs_dir(&test_dir)
        .await?;

    println!("  Verifying capabilities are registered...");
    let all_caps = marketplace.list_capabilities().await;
    let test_caps: Vec<_> = all_caps
        .iter()
        .filter(|c| c.id.starts_with("mcp.test"))
        .collect();

    if test_caps.is_empty() {
        return Err("No test capabilities found in marketplace!".into());
    }

    println!("  Found {} test capabilities:", test_caps.len());
    for cap in &test_caps {
        println!("    - {} ({})", cap.id, cap.name);
    }

    // Cleanup
    fs::remove_dir_all(&test_dir)?;

    Ok(loaded)
}
