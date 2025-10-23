//! Test Real GitHub MCP Introspection
//!
//! This test connects to a real GitHub MCP server and discovers tools.
//! Requires GITHUB_PAT environment variable for authentication.

use rtfs_compiler::ccos::synthesis::capability_synthesizer::CapabilitySynthesizer;
use std::collections::HashMap;
use std::path::PathBuf;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🔌 Testing Real GitHub MCP API Introspection");
    println!("=============================================");
    println!();

    // Get GitHub PAT from environment
    let github_pat = std::env::var("GITHUB_PAT").unwrap_or_else(|_| {
        eprintln!("❌ Error: GITHUB_PAT environment variable not set");
        eprintln!();
        eprintln!("Please set your GitHub Personal Access Token:");
        eprintln!("  export GITHUB_PAT=github_pat_...");
        eprintln!();
        eprintln!("Get a token from: https://github.com/settings/tokens");
        std::process::exit(1);
    });

    println!("✅ GitHub PAT found (length: {})", github_pat.len());
    println!();

    // GitHub Copilot MCP API
    let server_url = std::env::var("GITHUB_MCP_SERVER_URL")
        .unwrap_or_else(|_| "https://api.githubcopilot.com/mcp/".to_string());
    let server_name = "github";
    
    // Setup authentication headers with Bearer token for GitHub Copilot API
    let mut auth_headers = HashMap::new();
    auth_headers.insert("Authorization".to_string(), format!("Bearer {}", github_pat));
    auth_headers.insert("Content-Type".to_string(), "application/json".to_string());
    auth_headers.insert("Accept".to_string(), "application/json".to_string());
    
    println!("🔍 Connecting to GitHub Copilot MCP API");
    println!("   URL: {}", server_url);
    println!("   Authentication: Bearer token from GITHUB_PAT");
    println!();

    // Create synthesizer (non-mock mode for real API)
    let synthesizer = CapabilitySynthesizer::new();

    // Perform MCP introspection with authentication
    println!("📡 Calling tools/list endpoint with authentication...");
    let introspection_result = synthesizer
        .synthesize_from_mcp_introspection_with_auth(&server_url, server_name, Some(auth_headers))
        .await?;

    println!("✅ MCP Introspection Complete!");
    println!("   Discovered {} tools from GitHub MCP", introspection_result.capabilities.len());
    println!("   Overall Quality Score: {:.2}", introspection_result.overall_quality_score);
    println!("   All Safety Passed: {}", introspection_result.all_safety_passed);
    println!();

    // Display discovered GitHub MCP tools
    println!("📋 Discovered GitHub MCP Tools:");
    println!("================================");
    for (i, capability_result) in introspection_result.capabilities.iter().enumerate() {
        println!("{}. {} ({})", 
            i + 1,
            capability_result.capability.name,
            capability_result.capability.id
        );
        println!("   Description: {}", capability_result.capability.description);
        
        // Show schemas
        if let Some(input_schema) = &capability_result.capability.input_schema {
            println!("   ✅ Input Schema: {:?}", input_schema);
        } else {
            println!("   ⚠️  No input schema");
        }
        
        if let Some(output_schema) = &capability_result.capability.output_schema {
            println!("   ✅ Output Schema: {:?}", output_schema);
        } else {
            println!("   ⚠️  No output schema");
        }
        println!();
    }

    // Save capabilities to RTFS files
    println!("💾 Saving GitHub MCP Capabilities to RTFS Files");
    println!("------------------------------------------------");
    
    let output_dir = std::env::current_dir()?
        .parent()
        .map(|p| p.join("capabilities"))
        .unwrap_or_else(|| PathBuf::from("../capabilities"));
    
    let introspector = synthesizer.get_mcp_introspector();

    for capability_result in &introspection_result.capabilities {
        let file_path = introspector.save_capability_to_rtfs(
            &capability_result.capability,
            &capability_result.implementation_code,
            &output_dir,
        )?;

        println!("✅ Saved: {}", file_path.display());
    }
    println!();

    // Show a sample RTFS capability
    if let Some(first_cap) = introspection_result.capabilities.first() {
        println!("📄 Sample Generated RTFS Capability:");
        println!("=====================================");
        println!();
        
        let rtfs_content = introspector.capability_to_rtfs_string(
            &first_cap.capability,
            &first_cap.implementation_code,
        );
        
        println!("{}", rtfs_content);
        println!();
    }

    println!("🎉 GitHub MCP Introspection Complete!");
    println!();
    println!("📁 Capability files saved to: capabilities/<capability-id>/capability.rtfs");
    println!("   Each GitHub MCP tool is now a typed, validated RTFS capability!");
    println!();
    println!("🔑 Key Points:");
    println!("   ✅ Connected to GitHub MCP server");
    println!("   ✅ Authenticated via GITHUB_PAT environment variable");
    println!("   ✅ Discovered all available GitHub tools");
    println!("   ✅ Generated RTFS capabilities with proper schemas");
    println!("   ✅ Ready to use in CCOS plans!");
    println!();
    println!("💡 To use these capabilities:");
    println!("   ((call \"mcp.github.list_issues\") {{");
    println!("     :owner \"mandubian\"");
    println!("     :repo \"ccos\"");
    println!("     :state \"open\"");
    println!("   }})");

    Ok(())
}


