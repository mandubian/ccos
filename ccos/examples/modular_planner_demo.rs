//! Modular Planner Demo
//!
//! This example demonstrates the new modular planning architecture that:
//! 1. Uses pluggable decomposition strategies (pattern-first, then LLM fallback)
//! 2. Properly stores all intents in the IntentGraph as real nodes
//! 3. Uses resolution strategies to map semantic intents to capabilities
//! 4. Generates executable RTFS plans from resolved capabilities
//! 5. EXECUTES the generated plan using the CCOS runtime
//!
//! The key difference from autonomous_agent_demo is that this architecture:
//! - Separates WHAT (decomposition produces semantic intents) from HOW (resolution finds capabilities)
//! - Uses pattern matching first for common goal structures (fast, deterministic)
//! - Falls back to LLM only when patterns don't match
//! - Stores all planning decisions in IntentGraph for audit/reuse
//!
//! Usage:
//!   cargo run --example modular_planner_demo -- --goal "list issues in mandubian/ccos but ask me for the page size"

use ccos::examples_common::builder::ModularPlannerBuilder;
use ccos::planner::modular_planner::resolution::semantic::{CapabilityCatalog, CapabilityInfo};
use ccos::planner::modular_planner::{
    orchestrator::{PlanResult, TraceEvent},
    CatalogResolution, ModularPlanner, PatternDecomposition, ResolvedCapability,
};
use clap::Parser;
use rtfs::runtime::security::RuntimeContext;
use std::error::Error;
use std::sync::Arc;

// ============================================================================
// CLI Arguments
// ============================================================================

#[derive(Parser, Debug)]
struct Args {
    /// Natural language goal
    #[arg(
        long,
        default_value = "list issues in mandubian/ccos but ask me for the page size"
    )]
    goal: String,

    /// Show detailed planning trace
    #[arg(long)]
    verbose: bool,

    /// Show LLM prompts and responses (verbose LLM debugging)
    #[arg(long)]
    verbose_llm: bool,

    /// Discover tools from MCP servers (requires GITHUB_TOKEN)
    #[arg(long)]
    discover_mcp: bool,

    /// Path to agent config file
    #[arg(long, default_value = "config/agent_config.toml")]
    config: String,

    /// Skip execution (just show the plan)
    #[arg(long)]
    no_execute: bool,

    /// Force pure LLM decomposition (skip patterns)
    #[arg(long)]
    pure_llm: bool,

    /// Use embedding-based scoring (default: true, use --no-embeddings to disable)
    #[arg(long, default_value_t = true)]
    use_embeddings: bool,

    /// Disable tool cache (force fresh MCP discovery)
    #[arg(long)]
    no_cache: bool,

    /// Show the full prompt sent to LLM during decomposition
    #[arg(long)]
    show_prompt: bool,

    /// Confirm before each LLM call (shows prompt and waits for Enter)
    #[arg(long)]
    confirm_llm: bool,
}

// ============================================================================
// Main Demo
// ============================================================================

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    let args = Args::parse();

    println!("\n╔══════════════════════════════════════════════════════════════╗");
    println!("║           🧩 Modular Planner Demo                            ║");
    println!("╚══════════════════════════════════════════════════════════════╝\n");

    println!("📋 Goal: \"{}\"\n", args.goal);

    // Use the builder to set up the environment
    let env = ModularPlannerBuilder::new()
        .with_config(&args.config)
        .with_options(
            args.use_embeddings,
            args.discover_mcp,
            args.no_cache,
            args.pure_llm,
        )
        .with_debug_options(args.verbose_llm, args.show_prompt, args.confirm_llm)
        .build()
        .await?;

    let ccos = env.ccos;
    let mut planner = env.planner;
    let intent_graph = env.intent_graph;

    // 6. Plan!
    println!("\n🚀 Planning...\n");

    let plan_result = match planner.plan(&args.goal).await {
        Ok(result) => {
            print_plan_result(&result, args.verbose);

            // Show IntentGraph state
            println!("\n📊 IntentGraph State:");
            let graph = intent_graph.lock().unwrap();
            println!(
                "   Root intent: {}",
                &result.root_intent_id[..40.min(result.root_intent_id.len())]
            );
            println!("   Total intents created: {}", result.intent_ids.len() + 1); // +1 for root

            if let Some(root) = graph.get_intent(&result.root_intent_id) {
                println!("   Root goal: \"{}\"", root.goal);
            }

            Some(result)
        }
        Err(e) => {
            println!("\n❌ Planning failed: {}", e);
            println!("\n💡 Tip: The pattern decomposition only handles specific goal patterns:");
            println!("   - \"X but ask me for Y\"");
            println!("   - \"ask me for X then Y\"");
            println!("   - \"X then Y\"");
            println!("   - \"X and filter/sort by Y\"");
            None
        }
    };

    // 7. Execute!
    if let Some(result) = plan_result {
        if !args.no_execute {
            println!("\n⚡ Executing Plan...");
            println!("───────────────────────────────────────────────────────────────");

            let plan_obj = ccos::types::Plan {
                plan_id: format!("modular-plan-{}", uuid::Uuid::new_v4()),
                name: Some("Modular Plan".to_string()),
                body: ccos::types::PlanBody::Rtfs(result.rtfs_plan.clone()),
                intent_ids: result.intent_ids.clone(),
                ..Default::default()
            };

            let context = RuntimeContext::full();
            match ccos.validate_and_execute_plan(plan_obj, &context).await {
                Ok(exec_result) => {
                    println!("\n🏁 Execution Result:");
                    println!("   Success: {}", exec_result.success);

                    // Format output nicely
                    let output_str = value_to_string(&exec_result.value);
                    println!("   Result: {}", output_str);

                    if !exec_result.success {
                        if let Some(err) = exec_result.metadata.get("error") {
                            println!("   Error: {:?}", err);
                        }
                    }
                }
                Err(e) => {
                    println!("\n❌ Execution Failed: {}", e);
                }
            }
        }
    }

    println!("\n✅ Demo complete!");
    Ok(())
}

/// Convert RTFS value to string for display
fn value_to_string(v: &rtfs::runtime::values::Value) -> String {
    format!("{:?}", v)
}

/// Print the plan result
fn print_plan_result(result: &PlanResult, verbose: bool) {
    println!("═══════════════════════════════════════════════════════════════");
    println!("📋 Plan Result");
    println!("═══════════════════════════════════════════════════════════════\n");

    // Show resolved steps
    println!("📝 Resolved Steps ({}):", result.intent_ids.len());
    for (i, intent_id) in result.intent_ids.iter().enumerate() {
        if let Some(resolution) = result.resolutions.get(intent_id) {
            let (status, cap_id) = match resolution {
                ResolvedCapability::Local { capability_id, .. } => {
                    ("Local", capability_id.as_str())
                }
                ResolvedCapability::Remote { capability_id, .. } => {
                    ("Remote", capability_id.as_str())
                }
                ResolvedCapability::BuiltIn { capability_id, .. } => {
                    ("BuiltIn", capability_id.as_str())
                }
                ResolvedCapability::Synthesized { capability_id, .. } => {
                    ("Synth", capability_id.as_str())
                }
                ResolvedCapability::NeedsReferral { reason, .. } => ("Referral", reason.as_str()),
            };
            println!("   {}. [{}] {}", i + 1, status, cap_id);
        }
    }

    // Show generated RTFS plan
    println!("\n📜 Generated RTFS Plan:");
    println!("───────────────────────────────────────────────────────────────");
    println!("{}", result.rtfs_plan);
    println!("───────────────────────────────────────────────────────────────");

    // Show trace if verbose
    if verbose {
        println!("\n🔍 Planning Trace:");
        for event in &result.trace.events {
            match event {
                TraceEvent::DecompositionStarted { strategy } => {
                    println!("   → Decomposition started with strategy: {}", strategy);
                }
                TraceEvent::DecompositionCompleted {
                    num_intents,
                    confidence,
                } => {
                    println!(
                        "   ✓ Decomposition completed: {} intents, confidence: {:.2}",
                        num_intents, confidence
                    );
                }
                TraceEvent::IntentCreated {
                    intent_id,
                    description,
                } => {
                    println!(
                        "   + Intent created: {} - \"{}\"",
                        &intent_id[..20.min(intent_id.len())],
                        description
                    );
                }
                TraceEvent::EdgeCreated {
                    from,
                    to,
                    edge_type,
                } => {
                    println!(
                        "   ⟶ Edge: {} -> {} ({})",
                        &from[..16.min(from.len())],
                        &to[..16.min(to.len())],
                        edge_type
                    );
                }
                TraceEvent::ResolutionStarted { intent_id } => {
                    println!("   🔍 Resolving: {}", &intent_id[..20.min(intent_id.len())]);
                }
                TraceEvent::ResolutionCompleted {
                    intent_id,
                    capability,
                } => {
                    println!(
                        "   ✓ Resolved: {} → {}",
                        &intent_id[..16.min(intent_id.len())],
                        capability
                    );
                }
                TraceEvent::ResolutionFailed { intent_id, reason } => {
                    println!(
                        "   ✗ Failed: {} - {}",
                        &intent_id[..16.min(intent_id.len())],
                        reason
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_pattern_decomposition() {
        use ccos::intent_graph::{config::IntentGraphConfig, IntentGraph};
        use std::sync::Mutex;

        let intent_graph = Arc::new(Mutex::new(
            IntentGraph::with_config(IntentGraphConfig::with_in_memory_storage()).unwrap(),
        ));

        // Mock catalog for test (since we can't easily spin up CCOS here)
        struct MockCatalog;
        #[async_trait::async_trait(?Send)]
        impl CapabilityCatalog for MockCatalog {
            async fn list_capabilities(&self, _domain: Option<&str>) -> Vec<CapabilityInfo> {
                vec![]
            }
            async fn get_capability(&self, _id: &str) -> Option<CapabilityInfo> {
                None
            }
            async fn search(&self, _query: &str, _limit: usize) -> Vec<CapabilityInfo> {
                vec![]
            }
        }
        let catalog = Arc::new(MockCatalog);

        let mut planner = ModularPlanner::new(
            Box::new(PatternDecomposition::new()),
            Box::new(CatalogResolution::new(catalog)),
            intent_graph,
        );

        let result = planner
            .plan("list issues but ask me for page size")
            .await
            .unwrap();

        assert_eq!(result.intent_ids.len(), 2);
        assert!(result.rtfs_plan.contains("ccos.user.ask"));
    }
}
