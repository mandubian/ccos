//! Semantic Judge Demo
//!
//! This example demonstrates the Semantic Plan Judge in action:
//! 1. Shows how the judge blocks plans that don't align with the goal
//! 2. Shows how the judge allows safe, aligned plans
//! 3. Demonstrates the fail-open/fail-closed behavior
//!
//! The Semantic Judge is a "common sense" check that uses an LLM to verify:
//! - Goal Alignment: Does the plan actually achieve the stated goal?
//! - Semantic Safety: Are the tools appropriate for the action?
//! - Hallucination Check: Does the plan invent nonsensical steps?
//!
//! Usage:
//!   # Test with a safe goal (should pass)
//!   cargo run --example semantic_judge_demo -- --goal "list issues in mandubian/ccos"
//!
//!   # Test with a dangerous goal (should be blocked if plan doesn't match)
//!   cargo run --example semantic_judge_demo -- --goal "delete all temporary files" --verbose
//!
//!   # Show the judge's reasoning
//!   cargo run --example semantic_judge_demo -- --goal "list issues" --verbose

use ccos::examples_common::builder::ModularPlannerBuilder;
use ccos::types::{Plan, PlanBody, PlanStatus};
use clap::Parser;
use rtfs::runtime::security::RuntimeContext;
use std::error::Error;

#[derive(Parser, Debug)]
struct Args {
    /// Natural language goal
    #[arg(long, default_value = "list issues in mandubian/ccos")]
    goal: String,

    /// Show detailed output including judge reasoning
    #[arg(long)]
    verbose: bool,

    /// Path to agent config file
    #[arg(long, default_value = "config/agent_config.toml")]
    config: String,

    /// Risk threshold (0.0-1.0). Plans with higher risk are blocked.
    #[arg(long, default_value_t = 0.7)]
    risk_threshold: f64,

    /// If true, allow execution when the LLM is unavailable (fail-open)
    #[arg(long)]
    fail_open: bool,

    /// Disable the semantic judge entirely
    #[arg(long)]
    disable_judge: bool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    // Initialize logging if RUST_LOG is set
    let _ = std::env::var("RUST_LOG").map(|_| {
        // env_logger is optional; we just skip if not available
    });

    let args = Args::parse();

    println!("\n╔══════════════════════════════════════════════════════════════╗");
    println!("║           ⚖️  Semantic Judge Demo                            ║");
    println!("╚══════════════════════════════════════════════════════════════╝\n");

    println!("📋 Goal: \"{}\"\n", args.goal);

    // Build the CCOS environment
    let env = ModularPlannerBuilder::new()
        .with_config(&args.config)
        .with_options(true, false, false, false) // embeddings, no MCP, use cache, LLM mode
        .with_safe_exec(true)
        .with_debug_options(args.verbose, false, false)
        .build()
        .await?;

    let ccos = env.ccos;
    let mut planner = env.planner;

    // Configure the semantic judge policy
    println!("⚙️  Semantic Judge Configuration:");
    println!("   Enabled: {}", !args.disable_judge);
    println!("   Risk Threshold: {:.2}", args.risk_threshold);
    println!("   Fail-Open: {}", args.fail_open);
    println!();

    // Note: The GovernanceKernel is inside CCOS and already configured with defaults.
    // In a real scenario, you'd configure the policy during CCOS initialization.
    // For this demo, we'll just show the flow.

    // Step 1: Generate a plan using the modular planner
    println!("🧩 Step 1: Generating plan from goal...\n");

    let plan_result = match planner.plan(&args.goal).await {
        Ok(result) => {
            println!("✅ Plan generated successfully!");
            println!();
            println!("📜 Generated RTFS Plan:");
            println!("───────────────────────────────────────────────────────────────");
            println!("{}", result.rtfs_plan);
            println!("───────────────────────────────────────────────────────────────");
            Some(result)
        }
        Err(e) => {
            println!("❌ Plan generation failed: {}", e);
            None
        }
    };

    // Step 2: Execute the plan through the governance pipeline
    if let Some(result) = plan_result {
        if result.plan_status == PlanStatus::PendingSynthesis {
            println!("\n⚠️  Plan has pending capabilities - skipping execution");
            return Ok(());
        }

        println!("\n⚖️  Step 2: Submitting plan to Governance Kernel (includes Semantic Judge)...\n");

        let plan = Plan {
            plan_id: format!("judge-demo-{}", uuid::Uuid::new_v4()),
            name: Some("Semantic Judge Demo Plan".to_string()),
            body: PlanBody::Rtfs(result.rtfs_plan.clone()),
            intent_ids: result.intent_ids.clone(),
            status: result.plan_status,
            ..Default::default()
        };

        let context = RuntimeContext::full();

        match ccos.validate_and_execute_plan(plan, &context).await {
            Ok(exec_result) => {
                println!("✅ Plan PASSED governance checks (including Semantic Judge)");
                println!();
                println!("🏁 Execution Result:");
                println!("   Success: {}", exec_result.success);
                if args.verbose {
                    println!("   Value: {:?}", exec_result.value);
                }
            }
            Err(e) => {
                let err_msg = format!("{}", e);
                
                if err_msg.contains("Plan rejected by semantic judge") {
                    println!("🛑 Plan BLOCKED by Semantic Judge!");
                    println!();
                    println!("📝 Judge Reasoning:");
                    println!("───────────────────────────────────────────────────────────────");
                    // Extract the reasoning from the error message
                    if let Some(start) = err_msg.find("Reasoning:") {
                        println!("   {}", &err_msg[start..]);
                    } else {
                        println!("   {}", err_msg);
                    }
                    println!("───────────────────────────────────────────────────────────────");
                    println!();
                    println!("💡 The Semantic Judge detected that the plan may not align with");
                    println!("   the goal, or poses semantic safety risks.");
                } else {
                    println!("❌ Execution failed: {}", e);
                }
            }
        }
    }

    println!("\n✅ Demo complete!");
    Ok(())
}
