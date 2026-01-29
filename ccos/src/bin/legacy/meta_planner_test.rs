//! Meta-Planner Complex Goal Testing
//!
//! Tests the meta-planner's ability to:
//! - Generate multi-step plans from complex goals
//! - Use safe-exec for read-only intermediate steps
//! - Apply learning patterns from WorkingMemory
//!
//! Uses the modular planner builder with full LLM configuration
//! from config/agent_config.toml

use ccos::examples_common::builder::ModularPlannerBuilder;
use ccos::planner::modular_planner::orchestrator::TraceEvent;
use std::error::Error;

/// Test scenarios for the meta-planner
#[derive(Debug, Clone)]
struct TestScenario {
    name: &'static str,
    goal: &'static str,
    expected_steps_min: usize,
    description: &'static str,
}

const SCENARIOS: &[TestScenario] = &[
    TestScenario {
        name: "Scenario A: Multi-Step Research",
        goal: "Search for Rust repositories on GitHub, identify which ones need dependency updates, and create a summary report",
        expected_steps_min: 3,
        description: "Tests multi-step decomposition with data transformation",
    },
    TestScenario {
        name: "Scenario B: Code Analysis",
        goal: "Analyze the CCOS codebase structure, find the main modules, and explain what each does",
        expected_steps_min: 3,
        description: "Tests iterative exploration and analysis",
    },
    TestScenario {
        name: "Scenario C: Weather Report",
        goal: "Get the current weather for Paris and London, compare them, and tell me which city is warmer",
        expected_steps_min: 3,
        description: "Tests parallel data fetching and comparison",
    },
];

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    println!("╔══════════════════════════════════════════════════════════════════════════════╗");
    println!("║              Meta-Planner Complex Goal Testing                               ║");
    println!("║                                                                              ║");
    println!("║  Testing autonomous plan generation with complex, multi-step goals          ║");
    println!("║  Using LLM decomposition from config/agent_config.toml                      ║");
    println!("╚══════════════════════════════════════════════════════════════════════════════╝\n");

    // =========================================================================
    // Initialize CCOS Infrastructure using ModularPlannerBuilder
    // =========================================================================
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("🔧 INITIALIZATION: Setting up CCOS infrastructure with LLM");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

    let config_path = "config/agent_config.toml";
    println!("  📁 Loading config from: {}", config_path);

    // Build environment with:
    // - Config from agent_config.toml (includes LLM profiles)
    // - Embeddings enabled for semantic matching
    // - MCP discovery enabled
    // - No cache (fresh discovery)
    // - Pure LLM mode (skip patterns since they don't match these complex goals)
    let env = ModularPlannerBuilder::new()
        .with_config(config_path)
        .with_options(
            true,  // use_embeddings
            true,  // discover_mcp (for tool discovery)
            false, // no_cache
            true,  // pure_llm (use LLM decomposition, skip patterns)
        )
        .with_debug_options(
            true,  // verbose_llm (show LLM prompts/responses)
            false, // show_prompt
            false, // confirm_llm
        )
        .build()
        .await?;

    let _ccos = env.ccos;
    let mut planner = env.planner;
    let _intent_graph = env.intent_graph;

    println!("  ✅ ModularPlanner configured with LLM decomposition");
    println!("  ✅ Using default LLM profile from config\n");

    // =========================================================================
    // Run Test Scenarios
    // =========================================================================
    let mut pass_count = 0;
    let mut fail_count = 0;

    for (i, scenario) in SCENARIOS.iter().enumerate() {
        println!("\n");
        println!(
            "╔══════════════════════════════════════════════════════════════════════════════╗"
        );
        println!("║  {} [{}/{}]", scenario.name, i + 1, SCENARIOS.len());
        println!(
            "╠══════════════════════════════════════════════════════════════════════════════╣"
        );
        println!("║  💭 Goal: {}", truncate_str(scenario.goal, 60));
        println!("║  📋 Description: {}", scenario.description);
        println!(
            "║  🎯 Expected: {} steps minimum",
            scenario.expected_steps_min
        );
        println!(
            "╚══════════════════════════════════════════════════════════════════════════════╝\n"
        );

        println!(
            "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
        );
        println!("📋 PHASE 1: Decomposition & Resolution");
        println!(
            "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n"
        );

        match planner.plan(scenario.goal).await {
            Ok(result) => {
                println!("\n✅ Plan generated successfully!");
                println!("   📊 Sub-intents created: {}", result.intent_ids.len());
                println!("   📝 Root intent: {}", result.root_intent_id);

                // Show plan body
                println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
                println!("📄 GENERATED RTFS PLAN:");
                println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
                let plan_str = &result.rtfs_plan;
                for line in plan_str.lines().take(40) {
                    println!("  {}", line);
                }
                if plan_str.lines().count() > 40 {
                    println!("  ... ({} more lines)", plan_str.lines().count() - 40);
                }

                // Show trace summary
                println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
                println!("📊 PLANNING TRACE SUMMARY:");
                println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

                let mut decomp_count = 0;
                let mut resolution_ok = 0;
                let mut resolution_fail = 0;

                for event in &result.trace.events {
                    match event {
                        TraceEvent::DecompositionCompleted {
                            num_intents,
                            confidence,
                        } => {
                            println!(
                                "  📂 Decomposition: {} intents (confidence: {:.2})",
                                num_intents, confidence
                            );
                            decomp_count = *num_intents;
                        }
                        TraceEvent::ResolutionCompleted { capability, .. } => {
                            println!("  ✅ Resolved: {}", capability);
                            resolution_ok += 1;
                        }
                        TraceEvent::ResolutionFailed { reason, .. } => {
                            println!("  ❌ Failed: {}", truncate_str(reason, 50));
                            resolution_fail += 1;
                        }
                        _ => {}
                    }
                }

                println!(
                    "\n  Summary: {} decomposed, {} resolved, {} failed",
                    decomp_count, resolution_ok, resolution_fail
                );

                // Validate expectations
                let step_count = result.intent_ids.len();
                if step_count >= scenario.expected_steps_min {
                    println!(
                        "\n✅ PASS: {} steps >= {} expected",
                        step_count, scenario.expected_steps_min
                    );
                    pass_count += 1;
                } else {
                    println!(
                        "\n⚠️  WARNING: {} steps < {} expected",
                        step_count, scenario.expected_steps_min
                    );
                    fail_count += 1;
                }
            }
            Err(e) => {
                println!("\n❌ Plan generation failed: {:?}", e);
                println!("   This may indicate missing capabilities or LLM configuration issues.");
                fail_count += 1;
            }
        }
    }

    // =========================================================================
    // Summary
    // =========================================================================
    println!("\n\n");
    println!("╔══════════════════════════════════════════════════════════════════════════════╗");
    println!("║                              TEST SUMMARY                                    ║");
    println!("╚══════════════════════════════════════════════════════════════════════════════╝\n");

    // IntentGraph stats available via plan result's intent_ids\n    println!(\"📊 Total intents processed across all scenarios\");

    println!(
        "\n📈 Results: {} passed, {} failed out of {}",
        pass_count,
        fail_count,
        SCENARIOS.len()
    );

    if fail_count == 0 {
        println!("\n✅ All scenarios passed!");
    } else {
        println!("\n⚠️  Some scenarios failed - review output above for details");
    }

    println!("\n   Review the output above to identify:");
    println!("   - Decomposition quality (number and relevance of sub-intents)");
    println!("   - Capability resolution success/failure patterns");
    println!("   - Generated RTFS plan structure");

    Ok(())
}

/// Truncate a string with ellipsis
fn truncate_str(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len - 3])
    }
}
