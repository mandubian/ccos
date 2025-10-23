//! Test MCP Server Introspection
//!
//! This test demonstrates MCP tool introspection and RTFS capability generation.
//! MCP tools are discovered from an MCP server and converted to RTFS-first capabilities
//! with proper schemas, following the same pattern as OpenAPI introspection.

use rtfs_compiler::ccos::synthesis::capability_synthesizer::CapabilitySynthesizer;
use std::path::PathBuf;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🔌 Testing MCP Server Introspection");
    println!("====================================");
    println!();

    // Create a mock synthesizer for testing
    let synthesizer = CapabilitySynthesizer::mock();

    // GitHub MCP server details (mock for now)
    let server_url = "http://localhost:3000/github-mcp";
    let server_name = "github";

    println!("🔍 Introspecting MCP Server");
    println!("   URL: {}", server_url);
    println!("   Name: {}", server_name);
    println!();

    // Perform MCP introspection
    let introspection_result = synthesizer
        .synthesize_from_mcp_introspection(server_url, server_name)
        .await?;

    println!("✅ MCP Introspection Complete!");
    println!("   Discovered {} tools as capabilities", introspection_result.capabilities.len());
    println!("   Overall Quality Score: {:.2}", introspection_result.overall_quality_score);
    println!("   All Safety Passed: {}", introspection_result.all_safety_passed);
    println!();

    // Display discovered MCP capabilities
    println!("📋 Discovered MCP Tools:");
    println!("------------------------");
    for (i, capability_result) in introspection_result.capabilities.iter().enumerate() {
        println!("{}. {} ({})", 
            i + 1,
            capability_result.capability.name,
            capability_result.capability.id
        );
        println!("   Description: {}", capability_result.capability.description);
        println!("   MCP Tool: {}", 
            capability_result.capability.metadata.get("mcp_tool_name").unwrap_or(&"unknown".to_string())
        );
        
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
    println!("💾 Saving MCP Capabilities to RTFS Files");
    println!("-----------------------------------------");
    
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

    // Display RTFS content for each capability
    println!("📄 Generated RTFS Files:");
    println!("========================");
    println!();

    for (i, capability_result) in introspection_result.capabilities.iter().enumerate() {
        println!("📝 MCP Tool {}: {}", i + 1, capability_result.capability.id);
        println!("{}", "=".repeat(80));
        
        let rtfs_content = introspector.capability_to_rtfs_string(
            &capability_result.capability,
            &capability_result.implementation_code,
        );
        
        println!("{}", rtfs_content);
        println!("{}", "=".repeat(80));
        println!();
    }

    println!("🎉 MCP Introspection Complete!");
    println!();
    println!("📁 Capability files saved to: capabilities/<capability-id>/capability.rtfs");
    println!("   Each MCP tool is now a typed, validated RTFS capability!");
    println!();
    println!("🔑 Key Benefits:");
    println!("   ✅ Proper input/output schemas (not :any)");
    println!("   ✅ Runtime validation of all inputs");
    println!("   ✅ Simple RTFS wrapper around MCP JSON-RPC");
    println!("   ✅ Trivially callable from CCOS plans");
    println!("   ✅ Same pattern as OpenAPI capabilities");

    Ok(())
}

