//! Real-life example: Using service description to discover GitHub capabilities via MCP
//!
//! This demonstrates how description-based semantic matching finds MCP capabilities
//! when given functional descriptions rather than exact capability names.

use ccos::capability_marketplace::types::CapabilityManifest;
use ccos::discovery::{CapabilityNeed, DiscoveryEngine};
use ccos::CCOS;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🔍 Real-Life Example: GitHub Issue Discovery via MCP\n");
    println!("{}", "═".repeat(80));

    // Setup: Create CCOS instance (provides marketplace and intent graph)
    let ccos = Arc::new(CCOS::new().await?);

    // Get marketplace and intent graph from CCOS
    let marketplace = ccos.get_capability_marketplace();
    let intent_graph = ccos.get_intent_graph();

    // Create discovery engine (without arbiter for pure discovery test)
    let discovery_engine =
        DiscoveryEngine::new(Arc::clone(&marketplace), Arc::clone(&intent_graph));

    // Test Case 1: Functional description -> MCP discovery
    println!("\n📋 Test Case 1: Functional Description to MCP Discovery");
    println!("{}", "─".repeat(80));
    test_functional_description_discovery(&discovery_engine).await?;

    // Test Case 2: Different wording variations
    println!("\n📋 Test Case 2: Wording Variations");
    println!("{}", "─".repeat(80));
    test_wording_variations(&discovery_engine).await?;

    println!("\n{}", "═".repeat(80));
    println!("✅ Discovery testing complete");
    println!("\n💡 Key Insight:");
    println!("   Functional descriptions like 'List issues in a GitHub repository'");
    println!("   are matched against MCP capability descriptions via semantic search,");
    println!("   allowing discovery even when exact capability names are unknown.");

    Ok(())
}

async fn test_functional_description_discovery(
    discovery_engine: &DiscoveryEngine,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("\nGoal: Find a capability to list GitHub repository issues");
    println!("Using functional description (what we need):");
    println!("  'List all open issues in a GitHub repository'\n");

    // Create a need with a functional rationale (what we want the capability to do)
    let need = CapabilityNeed::new(
        "github.issues.list".to_string(), // LLM-generated capability class
        vec!["repository".to_string(), "state".to_string()],
        vec!["issues_list".to_string()],
        "List all open issues in a GitHub repository".to_string(), // Functional rationale
    );

    println!("CapabilityNeed:");
    println!("  Class: {}", need.capability_class);
    println!("  Rationale: {}", need.rationale);
    println!("  Inputs: {:?}", need.required_inputs);
    println!("  Outputs: {:?}", need.expected_outputs);

    println!("\n🔍 Searching MCP registry...");
    println!(
        "  Search query (from capability class): '{}'",
        need.capability_class
    );
    println!("  (This will introspect MCP servers and match by description)\n");

    // Add verbose logging to see what's happening
    println!("📋 Debug Information:");
    println!("  Capability class: {}", need.capability_class);
    println!("  Rationale: {}", need.rationale);
    println!("  Expected to find: GitHub server with 'list_issues' tool");
    println!();

    // Search MCP registry (this will use description-based matching)
    match discovery_engine.search_mcp_registry(&need).await {
        Ok(Some(manifest)) => {
            println!("✅ FOUND via MCP introspection:");
            println!("   ID: {}", manifest.id);
            println!("   Name: {}", manifest.name);
            println!("   Description: {}", manifest.description);
            println!("\n   📊 Match Details:");
            println!("      • LLM generated: {}", need.capability_class);
            println!("      • Found: {}", manifest.id);
            println!("      • Rationale matched description semantically");

            if let Some(ref provenance) = manifest.provenance {
                println!("\n   🔗 Provenance:");
                println!("      • Source: {}", provenance.source);
                if let Some(ref version) = provenance.version {
                    println!("      • Version: {}", version);
                }
            }

            async fn persist_manifest(
                discovery_engine: &DiscoveryEngine,
                manifest: &CapabilityManifest,
            ) {
                match discovery_engine.save_mcp_capability(manifest).await {
                    Ok(_) => {
                        println!("   💾 Persisted capability manifest to disk");
                    }
                    Err(e) => {
                        println!("   ⚠️  Failed to persist capability manifest: {}", e);
                    }
                }
            }

            if let Some(ref metadata) = manifest.metadata.get("mcp_server_url") {
                println!("\n   🌐 MCP Server: {}", metadata);
            }
            persist_manifest(discovery_engine, &manifest).await;
        }
        Ok(None) => {
            println!("❌ Not found in MCP registry");
            println!();
            println!("🔍 Diagnostic Information:");
            println!("   • Search query: '{}'", need.capability_class);
            println!("   • Rationale used: '{}'", need.rationale);
            println!();
            println!("   Possible reasons:");
            println!(
                "   1. No MCP servers found matching '{}'",
                need.capability_class
            );
            println!("   2. GitHub MCP server not configured in registry");
            println!("   3. MCP server introspection failed");
            println!("   4. Description match score below threshold (0.5)");
            println!("   5. MCP registry client returned no servers");
            println!();
            println!("   💡 To debug further:");
            println!("      • Check if MCP registry is accessible");
            println!("      • Verify GitHub MCP server is registered");
            println!("      • Try lowering the matching threshold");
            println!("      • Check network connectivity to MCP servers");
        }
        Err(e) => {
            println!("❌ Error during MCP search: {}", e);
        }
    }

    Ok(())
}

async fn test_wording_variations(
    discovery_engine: &DiscoveryEngine,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("\nTesting different ways to express the same need:\n");

    let variations = vec![
        (
            "List all open issues in a GitHub repository",
            true,
            "High - very descriptive",
        ),
        (
            "List issues in a GitHub repository",
            true,
            "High - clear and direct",
        ),
        (
            "Retrieve GitHub repository issues",
            true,
            "Medium - similar keywords",
        ),
        (
            "Get issues from GitHub repo",
            true,
            "Medium - casual wording",
        ),
        (
            "Need to see all issues in my GitHub repo",
            false,
            "Lower - vague wording",
        ),
    ];

    for (rationale, expect_match, explanation) in variations {
        println!("  Testing: '{}'", rationale);
        println!(
            "    Expected: {} ({})",
            if expect_match { "Match" } else { "Maybe" },
            explanation
        );

        let need = CapabilityNeed::new(
            "github.issues.list".to_string(),
            vec!["repository".to_string(), "state".to_string()],
            vec!["issues_list".to_string()],
            rationale.to_string(),
        );

        match discovery_engine.search_mcp_registry(&need).await {
            Ok(Some(manifest)) => {
                println!("    ✅ Matched: {}", manifest.id);
                println!("       Description: {}", manifest.description);
            }
            Ok(None) => {
                println!("    ⚠️  No match found");
                if expect_match {
                    println!("       (Unexpected - may need threshold adjustment)");
                }
            }
            Err(e) => {
                println!("    ❌ Error: {}", e);
            }
        }
        println!();
    }

    Ok(())
}
