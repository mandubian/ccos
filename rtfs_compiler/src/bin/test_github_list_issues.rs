//! Standalone test for GitHub list_issues MCP capability
//!
//! This tests the mcp.github.list_issues capability in detail to debug issues

use rtfs_compiler::ccos::environment::CCOSBuilder;
use rtfs_compiler::runtime::execution_outcome::ExecutionOutcome;
use std::fs;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🧪 Testing GitHub list_issues MCP Capability");
    println!("=============================================");
    println!();

    // Setup environment with network access
    let env = CCOSBuilder::new()
        .http_mocking(false)
        .http_allow_hosts(vec![
            "api.githubcopilot.com".to_string(),
            "localhost".to_string(),
        ])
        .verbose(true)
        .build()
        .expect("Failed to build environment");

    println!("✅ CCOS environment ready");
    println!();

    // Check environment variables
    println!("🔧 Environment Configuration:");
    println!("------------------------------");
    
    if let Ok(url) = std::env::var("MCP_SERVER_URL") {
        println!("✅ MCP_SERVER_URL: {}", url);
    } else {
        println!("ℹ️  MCP_SERVER_URL: not set (will use default)");
    }
    
    if let Ok(token) = std::env::var("GITHUB_PAT") {
        println!("✅ GITHUB_PAT: set (length: {})", token.len());
        std::env::set_var("GITHUB_PAT", token);
    } else {
        println!("⚠️  GITHUB_PAT: not set");
    }
    println!();

    // Load the capability
    println!("📦 Loading Capability");
    println!("---------------------");
    
    let capability_path = "../capabilities/mcp/github/list_issues.rtfs";
    println!("Path: {}", capability_path);
    
    let capability_code = match fs::read_to_string(capability_path) {
        Ok(code) => {
            println!("✅ Capability file loaded ({} bytes)", code.len());
            code
        }
        Err(e) => {
            eprintln!("❌ Failed to load capability: {}", e);
            return Err(e.into());
        }
    };
    
    // Show first few lines of implementation
    println!();
    println!("📄 Capability Implementation (excerpt):");
    for line in capability_code.lines().take(25) {
        if line.contains("default_url") || line.contains("mcp_url") || line.contains("env_url") {
            println!("  > {}", line);
        }
    }
    println!();

    // Register the capability
    println!("⚙️  Registering Capability");
    println!("-------------------------");
    
    match env.execute_code(&capability_code) {
        Ok(ExecutionOutcome::Complete(result)) => {
            println!("✅ Capability registered: {:?}", result);
        }
        Ok(ExecutionOutcome::RequiresHost(_)) => {
            println!("⚠️  Host call required during registration");
        }
        Err(e) => {
            eprintln!("❌ Failed to register: {:?}", e);
            return Err(format!("Registration failed: {:?}", e).into());
        }
    }
    println!();

    // Test 1: Call with minimal parameters
    println!("🚀 Test 1: List Issues (Minimal Parameters)");
    println!("--------------------------------------------");
    
    let call_code = r#"
        (call "mcp.github.list_issues" {
            :owner "mandubian"
            :repo "ccos"
        })
    "#;

    println!("📝 RTFS Code:");
    println!("{}", call_code);
    println!();
    println!("⏳ Executing...");
    println!();

    match env.execute_code(call_code) {
        Ok(ExecutionOutcome::Complete(result)) => {
            println!("✅ Execution completed");
            println!();
            println!("📊 Full Result:");
            println!("{:#?}", result);
            println!();
            
            // Analyze the result
            println!("🔍 Result Analysis:");
            let result_str = format!("{:?}", result);
            
            if result_str.contains("error") {
                println!("   ⚠️  Contains 'error' in response");
            }
            if result_str.contains("No response") {
                println!("   ⚠️  'No response from MCP server' - server not accessible");
            }
            if result_str.contains("issues") || result_str.contains("title") {
                println!("   ✅ Looks like actual GitHub issues data!");
            }
            if result_str == "Map({})" || result_str.contains("Nil") {
                println!("   ⚠️  Empty or nil response");
            }
            
            println!();
            println!("Result size: {} chars", result_str.len());
        }
        Ok(ExecutionOutcome::RequiresHost(hc)) => {
            println!("⚠️  Host call required: {:?}", hc);
        }
        Err(e) => {
            println!("❌ Execution error:");
            println!("{:#?}", e);
        }
    }
    println!();

    // Test 2: Call with all parameters
    println!("🚀 Test 2: List Issues (With State Filter)");
    println!("-------------------------------------------");
    
    let call_code2 = r#"
        (call "mcp.github.list_issues" {
            :owner "mandubian"
            :repo "ccos"
            :state "open"
        })
    "#;

    println!("📝 RTFS Code:");
    println!("{}", call_code2);
    println!();
    println!("⏳ Executing...");
    println!();

    match env.execute_code(call_code2) {
        Ok(ExecutionOutcome::Complete(result)) => {
            println!("✅ Execution completed");
            println!();
            
            let result_str = format!("{:?}", result);
            if result_str.len() > 1000 {
                println!("📊 Result (first 1000 chars):");
                println!("{}", &result_str[..1000]);
                println!("... (truncated, total {} chars)", result_str.len());
            } else {
                println!("📊 Full Result:");
                println!("{}", result_str);
            }
        }
        Ok(ExecutionOutcome::RequiresHost(_)) => {
            println!("⚠️  Host call required");
        }
        Err(e) => {
            println!("❌ Execution error: {:?}", e);
        }
    }
    println!();

    // Summary
    println!("📋 Summary");
    println!("==========");
    println!();
    println!("To test with a real MCP server:");
    println!("1. Start a local MCP server:");
    println!("   npx @modelcontextprotocol/server-github");
    println!();
    println!("2. Set environment variables:");
    println!("   export MCP_SERVER_URL=http://localhost:3000/github-mcp");
    println!("   export GITHUB_PAT=your_github_token");
    println!();
    println!("3. Re-run this test:");
    println!("   cargo run --bin test_github_list_issues");

    Ok(())
}

