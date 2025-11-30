//! Alternative test: Direct MCP server introspection (bypass registry)
//!
//! This demonstrates description-based matching when you know the MCP server URL.
//! Useful when the registry doesn't have the server or you want to test with a specific server.

use ccos::discovery::capability_matcher;
use ccos::discovery::embedding_service::EmbeddingService;
use ccos::discovery::CapabilityNeed;
use ccos::synthesis::mcp_introspector::MCPIntrospector;
use std::collections::HashMap;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🔍 Direct MCP Server Introspection Test\n");
    println!("{}", "═".repeat(80));

    // Get GitHub MCP server URL from environment
    let github_mcp_url = std::env::var("GITHUB_MCP_URL")
        .unwrap_or_else(|_| "https://api.githubcopilot.com/mcp/".to_string());

    // Get GitHub token from environment (required for authentication)
    let github_token = std::env::var("GITHUB_TOKEN")
        .or_else(|_| std::env::var("MCP_AUTH_TOKEN"))
        .map_err(|_| {
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "GITHUB_TOKEN or MCP_AUTH_TOKEN environment variable required",
            )) as Box<dyn std::error::Error>
        })?;

    println!("📋 Test: Direct GitHub MCP Server Introspection");
    println!("{}", "─".repeat(80));
    println!("\nServer URL: {}", github_mcp_url);
    println!("Authentication: ✓ Token provided (not printed)");
    println!("(Set GITHUB_MCP_URL and GITHUB_TOKEN env vars to configure)\n");

    // Build authentication headers
    let mut auth_headers = HashMap::new();
    auth_headers.insert(
        "Authorization".to_string(),
        format!("Bearer {}", github_token),
    );

    // Create a need with functional description
    let need = CapabilityNeed::new(
        "github.issues.list".to_string(),
        vec!["repository".to_string(), "state".to_string()],
        vec!["issues_list".to_string()],
        "List all open issues in a GitHub repository".to_string(),
    );

    println!("CapabilityNeed:");
    println!("  Class: {}", need.capability_class);
    println!("  Rationale: {}", need.rationale);

    println!("\n🔍 Introspecting MCP server...");
    println!("  → Using session manager with authentication\n");

    // Introspect the server directly with authentication and session management
    let introspector = MCPIntrospector::new();
    match introspector
        .introspect_mcp_server_with_auth(&github_mcp_url, "github", Some(auth_headers))
        .await
    {
        Ok(introspection) => {
            println!("✓ Introspection successful!");
            println!("  Server: {}", introspection.server_name);
            println!("  Tools found: {}\n", introspection.tools.len());

            // Create capabilities from tools
            match introspector.create_capabilities_from_mcp(&introspection) {
                Ok(capabilities) => {
                    println!("📦 Created {} capability manifest(s)\n", capabilities.len());

                    // Find best match using description-based matching
                    let mut matches_with_scores: Vec<(
                        ccos::capability_marketplace::types::CapabilityManifest,
                        f64,
                        f64,
                        String,
                    )> = Vec::new();
                    let threshold = 0.5;

                    println!("🔍 Matching capabilities against need:");
                    println!("   Need rationale: \"{}\"\n", need.rationale);

                    // Try to use embedding service if available (from environment)
                    let mut embedding_service = EmbeddingService::from_env();
                    if embedding_service.is_some() {
                        println!("  → Using embedding-based matching (more accurate)\n");
                    } else {
                        println!(
                            "  → Using keyword-based matching (embedding service not configured)\n"
                        );
                        println!("  💡 To use embedding: set LOCAL_EMBEDDING_URL or OPENROUTER_API_KEY\n");
                    }

                    // Calculate scores for all capabilities
                    for manifest in &capabilities {
                        let desc_score = if let Some(ref mut emb_svc) = embedding_service {
                            // Use embedding-based matching (more accurate)
                            capability_matcher::calculate_description_match_score_with_embedding_async(
                                &need.rationale,
                                &manifest.description,
                                &manifest.name,
                                Some(emb_svc),
                            ).await
                        } else {
                            // Fallback to keyword-based matching
                            capability_matcher::calculate_description_match_score(
                                &need.rationale,
                                &manifest.description,
                                &manifest.name,
                            )
                        };

                        let name_score = capability_matcher::calculate_semantic_match_score(
                            &need.capability_class,
                            &manifest.id,
                            &manifest.name,
                        );

                        let best_score = desc_score.max(name_score);
                        let match_type = if desc_score > name_score {
                            "description".to_string()
                        } else {
                            "name".to_string()
                        };

                        matches_with_scores.push((
                            manifest.clone(),
                            desc_score,
                            name_score,
                            match_type,
                        ));
                    }

                    // Sort by best score (description-first, then name)
                    matches_with_scores.sort_by(|a, b| {
                        let a_best = a.1.max(a.2);
                        let b_best = b.1.max(b.2);
                        b_best
                            .partial_cmp(&a_best)
                            .unwrap_or(std::cmp::Ordering::Equal)
                    });

                    // Display all matches sorted by relevance
                    println!("📊 Matching Results (sorted by relevance):\n");
                    for (idx, (manifest, desc_score, name_score, match_type)) in
                        matches_with_scores.iter().enumerate()
                    {
                        let best_score = desc_score.max(*name_score);
                        let rank_marker = if idx == 0 && best_score >= threshold {
                            "🥇"
                        } else if idx == 1 && best_score >= threshold {
                            "🥈"
                        } else if idx == 2 && best_score >= threshold {
                            "🥉"
                        } else {
                            "  "
                        };

                        println!(
                            "{} [{}/{}] {}",
                            rank_marker,
                            idx + 1,
                            matches_with_scores.len(),
                            manifest.id
                        );
                        println!("     Description: {}", manifest.description);
                        println!(
                            "     • Description match: {:.3} (matches rationale)",
                            desc_score
                        );
                        println!(
                            "     • Name match: {:.3} (matches capability class)",
                            name_score
                        );
                        println!("     • Best score: {:.3} ({})", best_score, match_type);
                        if best_score >= threshold {
                            println!("     ✓ Above threshold ({:.1})", threshold);
                        } else {
                            println!("     ✗ Below threshold ({:.1})", threshold);
                        }
                        println!();
                    }

                    // Find the best match above threshold
                    let best_match =
                        matches_with_scores
                            .iter()
                            .find(|(_, desc_score, name_score, _)| {
                                desc_score.max(*name_score) >= threshold
                            });

                    if let Some((manifest, desc_score, name_score, match_type)) = best_match {
                        let best_score = desc_score.max(*name_score);
                        println!("{}", "═".repeat(80));
                        println!("✅ CHOSEN CAPABILITY (description-based matching):");
                        println!("{}", "═".repeat(80));
                        println!("   🎯 Capability ID: {}", manifest.id);
                        println!("   📝 Description: {}", manifest.description);
                        println!("   🔢 Match Scores:");
                        println!(
                            "      • Description match: {:.3} (primary - matches rationale)",
                            desc_score
                        );
                        println!(
                            "      • Name match: {:.3} (secondary - matches class)",
                            name_score
                        );
                        println!(
                            "      • Best score: {:.3} ({}-based)",
                            best_score, match_type
                        );
                        println!();
                        println!("   💡 Why this was chosen:");
                        if *match_type == "description" {
                            println!(
                                "      ✓ Description-based matching prioritized (score: {:.3})",
                                desc_score
                            );
                            println!(
                                "      ✓ Rationale '{}' semantically matches",
                                need.rationale
                            );
                            println!("      ✓ Capability description: '{}'", manifest.description);
                        } else {
                            println!(
                                "      ✓ Name-based matching used (description score: {:.3})",
                                desc_score
                            );
                            println!(
                                "      ✓ Capability class '{}' matches ID '{}'",
                                need.capability_class, manifest.id
                            );
                        }
                        println!(
                            "      ✓ Score {:.3} exceeds threshold {:.1}",
                            best_score, threshold
                        );
                    } else {
                        println!("{}", "═".repeat(80));
                        println!("❌ NO MATCH FOUND (all below threshold)");
                        println!("{}", "═".repeat(80));
                        println!("   Threshold: {:.1}", threshold);
                        println!("   Top candidate:");
                        if let Some((manifest, desc_score, name_score, _)) =
                            matches_with_scores.first()
                        {
                            let best_score = desc_score.max(*name_score);
                            println!("      • {}", manifest.id);
                            println!("      • Description: {}", manifest.description);
                            println!(
                                "      • Best score: {:.3} (need {:.1} to match)",
                                best_score, threshold
                            );
                            println!();
                            println!("   💡 Suggestions:");
                            println!(
                                "      • Lower threshold to {:.1} to include this capability",
                                best_score + 0.1
                            );
                            println!("      • Improve rationale to be more specific");
                            println!("      • Check if capability description is accurate");
                        }
                    }
                }
                Err(e) => {
                    println!("❌ Failed to create capabilities: {}", e);
                }
            }
        }
        Err(e) => {
            println!("❌ Introspection failed: {}", e);
            println!("\n💡 Possible reasons:");
            println!("   • Server URL is incorrect");
            println!("   • Server requires authentication");
            println!("   • Network connectivity issue");
            println!("   • Server is not accessible");
        }
    }

    Ok(())
}
