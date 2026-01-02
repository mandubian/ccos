//! DialoguePlanner Demo
//!
//! Interactive demo of the DialoguePlanner that orchestrates conversational
//! planning through CCOS.
//!
//! Run with: cargo run -p ccos --bin dialogue_planner_demo
//!
//! Example goals to try:
//! - "List all files in the current directory"
//! - "Read the contents of README.md"
//! - "Search for TODO comments in the codebase"

use ccos::examples_common::builder::CcosEnvBuilder;
use ccos::planner::dialogue_planner::{
    DialogueConfig, DialoguePlanner, DialogueResult, HumanEntity,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║           DialoguePlanner Interactive Demo                   ║");
    println!("╚══════════════════════════════════════════════════════════════╝\n");

    // Build CCOS environment using the standard builder
    println!("🔧 Initializing CCOS environment...");
    let env = CcosEnvBuilder::new().build().await?;
    let ccos = env.ccos;
    println!("✅ CCOS initialized\n");

    // Show available capabilities
    let marketplace = ccos.get_capability_marketplace();
    let capabilities = marketplace.list_capabilities().await;
    println!("📦 Available capabilities: {}", capabilities.len());

    // Extract and display unique domains
    let mut domains: std::collections::HashSet<String> = std::collections::HashSet::new();
    for cap in &capabilities {
        for domain in &cap.domains {
            domains.insert(domain.clone());
        }
    }
    if !domains.is_empty() {
        println!(
            "🏷️  Domains: {}\n",
            domains.into_iter().collect::<Vec<_>>().join(", ")
        );
    }

    // Create human entity for CLI interaction
    let entity = HumanEntity::new(Some("You".to_string()));

    // Create dialogue planner with default config
    let config = DialogueConfig::default();
    let mut planner = DialoguePlanner::new(Box::new(entity), ccos.clone(), config);

    // Main dialogue loop
    loop {
        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        println!("🎯 Enter your goal (or 'quit' to exit):");
        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

        let mut goal = String::new();
        std::io::stdin().read_line(&mut goal)?;
        let goal = goal.trim();

        if goal.is_empty() {
            continue;
        }

        if goal == "quit" || goal == "exit" || goal == "q" {
            println!("\n👋 Goodbye!\n");
            break;
        }

        // Start conversation
        println!("\n🗣️  Starting dialogue for goal: \"{}\"\n", goal);

        match planner.converse(goal).await {
            Ok(result) => {
                print_dialogue_result(&result);
            }
            Err(e) => {
                println!("❌ Dialogue error: {}\n", e);
            }
        }

        // Reset planner for next goal
        let entity = HumanEntity::new(Some("You".to_string()));
        let config = DialogueConfig::default();
        planner = DialoguePlanner::new(Box::new(entity), ccos.clone(), config);
    }

    Ok(())
}

fn print_dialogue_result(result: &DialogueResult) {
    println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("📋 Dialogue Result");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    match result {
        DialogueResult::PlanGenerated { plan, history } => {
            println!("✅ Status: Plan Generated");
            println!(
                "📝 Plan preview: {}...",
                &plan.rtfs_plan[..plan.rtfs_plan.len().min(100)]
            );
            println!("📄 Intent IDs: {:?}", plan.intent_ids);
            println!("💬 Turns: {}", history.turns.len());
        }
        DialogueResult::Abandoned { reason, history } => {
            println!("⏹️  Status: Abandoned");
            println!("📌 Reason: {}", reason);
            println!("💬 Turns: {}", history.turns.len());
        }
    }

    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");
}
