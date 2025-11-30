//! Test script for capability description-based semantic matching
//!
//! This tests the matching algorithm independently of the full discovery process.

use ccos::capability_marketplace::types::CapabilityManifest;
use ccos::discovery::capability_matcher;
use ccos::discovery::need_extractor::CapabilityNeed;
use rtfs::runtime::values::Value;

fn main() {
    println!("🧪 Testing Description-Based Semantic Matching\n");
    println!("{}", "═".repeat(80));

    // Test case 1: GitHub issues matching
    println!("\n📋 Test Case 1: GitHub Issues Listing");
    println!("{}", "─".repeat(80));
    test_github_issues_matching();

    // Test case 2: Generic step name matching
    println!("\n📋 Test Case 2: Generic Step Name Matching");
    println!("{}", "─".repeat(80));
    test_generic_step_name_matching();

    // Test case 3: Different wording variations
    println!("\n📋 Test Case 3: Wording Variations");
    println!("{}", "─".repeat(80));
    test_wording_variations();

    println!("\n{}", "═".repeat(80));
    println!("✅ Testing complete");
    println!("\n💡 Key Insights:");
    println!("   • Functional descriptions score higher than generic step names");
    println!("   • Wording variations matter but semantic matching handles most cases");
    println!("   • Threshold of 0.5 may need adjustment based on rationale quality");
    println!("   • Consider improving rationale generation to be more descriptive");
}

fn test_github_issues_matching() {
    // Create a need with a functional rationale
    let need = CapabilityNeed::new(
        "github.issues.list".to_string(),
        vec!["repository".to_string(), "state".to_string()],
        vec!["issues_list".to_string()],
        "List all open issues in a GitHub repository".to_string(),
    );

    // Create candidate capabilities
    let candidates = vec![
        create_manifest(
            "mcp.github.list_issues",
            "list_issues",
            "List issues in a GitHub repository. For pagination, use the 'endCursor' from the previous response's 'pageInfo' in the 'after' parameter.",
        ),
        create_manifest(
            "mcp.github.get_issue",
            "get_issue",
            "Get details of a specific issue in a GitHub repository.",
        ),
        create_manifest(
            "mcp.github.list_pull_requests",
            "list_pull_requests",
            "List pull requests in a GitHub repository.",
        ),
        create_manifest(
            "github.api.issues.list",
            "GitHub Issues API",
            "List issues using GitHub REST API.",
        ),
    ];

    test_matching(&need, &candidates);
}

fn test_generic_step_name_matching() {
    // Create a need with generic step name rationale (current problem)
    let need = CapabilityNeed::new(
        "github.issues.list".to_string(),
        vec!["repository".to_string(), "state".to_string()],
        vec!["issues_list".to_string()],
        "Need for step: List GitHub Repository Issues".to_string(),
    );

    // Same candidates
    let candidates = vec![
        create_manifest(
            "mcp.github.list_issues",
            "list_issues",
            "List issues in a GitHub repository. For pagination, use the 'endCursor' from the previous response's 'pageInfo' in the 'after' parameter.",
        ),
        create_manifest(
            "mcp.github.get_issue",
            "get_issue",
            "Get details of a specific issue in a GitHub repository.",
        ),
    ];

    test_matching(&need, &candidates);
}

fn test_wording_variations() {
    // Test different ways of expressing the same need
    let variations = vec![
        (
            "List all open issues in a GitHub repository",
            0.7,
            "High - functional description",
        ),
        (
            "Retrieve GitHub repository issues",
            0.5,
            "Medium - similar keywords",
        ),
        (
            "Get issues from GitHub repo",
            0.5,
            "Medium - different wording",
        ),
        (
            "Fetch open issues for a repository",
            0.4,
            "Lower - less specific",
        ),
        (
            "Need for step: List GitHub Repository Issues",
            0.4,
            "Lower - generic format",
        ),
    ];

    let capability_desc = "List issues in a GitHub repository. For pagination, use the 'endCursor' from the previous response's 'pageInfo' in the 'after' parameter.";

    println!("Testing rationale variations against fixed description:");
    println!("  Capability: {}", capability_desc);
    println!("\n  Rationale Variants:\n");

    let mut results = Vec::new();
    for (rationale, min_score, explanation) in variations {
        let score = capability_matcher::calculate_description_match_score(
            rationale,
            capability_desc,
            "list_issues",
        );

        let status = if score >= min_score { "✓" } else { "✗" };
        results.push((rationale, score, min_score, explanation, status));
    }

    // Sort by score descending
    results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    for (rationale, score, min_score, explanation, status) in &results {
        println!(
            "  {} Score: {:.3} (min: {:.1}) - {}",
            status, score, min_score, explanation
        );
        println!("     \"{}\"", rationale);
        println!();
    }

    println!(
        "  📊 Summary: {} out of {} variations meet minimum thresholds",
        results
            .iter()
            .filter(|(_, score, min, _, _)| *score >= *min)
            .count(),
        results.len()
    );
}

fn test_matching(need: &CapabilityNeed, candidates: &[CapabilityManifest]) {
    println!("Need:");
    println!("  Capability class: {}", need.capability_class);
    println!("  Rationale: {}", need.rationale);
    println!("\nCandidates:\n");

    let mut matches: Vec<(String, f64, f64)> = Vec::new(); // (id, desc_score, name_score)

    for manifest in candidates {
        let desc_score = capability_matcher::calculate_description_match_score(
            &need.rationale,
            &manifest.description,
            &manifest.name,
        );

        let name_score = capability_matcher::calculate_semantic_match_score(
            &need.capability_class,
            &manifest.id,
            &manifest.name,
        );

        matches.push((manifest.id.clone(), desc_score, name_score));

        println!("  • {}", manifest.id);
        println!("    Description: {}", manifest.description);
        println!("    Description match score: {:.3}", desc_score);
        println!("    Name match score: {:.3}", name_score);
        println!("    Best score: {:.3}\n", desc_score.max(name_score));
    }

    // Find best match
    let threshold = 0.5;
    let best_desc = matches
        .iter()
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
    let best_name = matches
        .iter()
        .max_by(|a, b| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal));

    println!("Results:");
    let mut best_overall: Option<(&str, f64, &str)> = None;

    if let Some((id, score, _)) = best_desc {
        if *score >= threshold {
            println!("  ✓ Best description match: {} (score: {:.3})", id, score);
            best_overall = Some((id, *score, "description"));
        } else {
            println!(
                "  ✗ Best description match: {} (score: {:.3}) - below threshold {:.1}",
                id, score, threshold
            );
        }
    }

    if let Some((id, _, score)) = best_name {
        if *score >= threshold {
            println!("  ✓ Best name match: {} (score: {:.3})", id, score);
            if let Some((_, best_score, _)) = best_overall {
                if *score > best_score {
                    best_overall = Some((id, *score, "name"));
                }
            } else {
                best_overall = Some((id, *score, "name"));
            }
        } else {
            println!(
                "  ✗ Best name match: {} (score: {:.3}) - below threshold {:.1}",
                id, score, threshold
            );
        }
    }

    if let Some((id, score, match_type)) = best_overall {
        println!(
            "\n  🎯 RECOMMENDED MATCH: {} ({}, score: {:.3})",
            id, match_type, score
        );
    } else {
        println!("\n  ⚠️  NO MATCHES ABOVE THRESHOLD {:.1}", threshold);
        println!("     Consider:");
        println!("      - Lowering threshold (currently {:.1})", threshold);
        println!("      - Improving rationale quality (more descriptive)");
        println!("      - Adding more candidate capabilities");
    }
}

fn create_manifest(id: &str, name: &str, description: &str) -> CapabilityManifest {
    use ccos::capability_marketplace::types::{LocalCapability, ProviderType};
    use rtfs::runtime::error::RuntimeResult;
    use std::sync::Arc;

    let stub_handler: Arc<dyn Fn(&Value) -> RuntimeResult<Value> + Send + Sync> =
        Arc::new(|_| Ok(Value::String("stub".to_string())));

    CapabilityManifest::new(
        id.to_string(),
        name.to_string(),
        description.to_string(),
        ProviderType::Local(LocalCapability {
            handler: stub_handler,
        }),
        "1.0.0".to_string(),
    )
}
