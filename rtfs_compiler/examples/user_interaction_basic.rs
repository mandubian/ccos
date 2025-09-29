//! Basic User Interaction Example
//!
//! Demonstrates the simplest human-in-the-loop pattern with CCOS:
//! A plan that asks the user for their name and greets them.
//!
//! Run:
//!   cargo run --example user_interaction_basic

use rtfs_compiler::ccos::CCOS;
use rtfs_compiler::runtime::security::{RuntimeContext, SecurityLevel};
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🎯 Basic User Interaction Example");
    println!("================================\n");

    // Initialize CCOS
    let ccos = Arc::new(CCOS::new().await?);

    // Security context allowing user interaction
    let ctx = RuntimeContext {
        security_level: SecurityLevel::Controlled,
        allowed_capabilities: vec!["ccos.echo".to_string(), "ccos.user.ask".to_string()]
            .into_iter()
            .collect(),
        ..RuntimeContext::pure()
    };

    // Example 1: Simple greeting with user's name
    println!("📝 Example 1: Simple Greeting");
    println!("----------------------------");
    let result1 = ccos
        .process_request("ask the user for their name and greet them personally", &ctx)
        .await;

    match result1 {
        Ok(res) => {
            println!("\n✅ Example 1 Result:");
            println!("   Success: {}", res.success);
            println!("   Value: {}\n", res.value);
        }
        Err(e) => {
            eprintln!("\n❌ Example 1 Error: {}\n", e);
        }
    }

    // Example 2: Ask for favorite color
    println!("📝 Example 2: Favorite Color");
    println!("---------------------------");
    let result2 = ccos
        .process_request(
            "ask the user what their favorite color is and tell them it's a great choice",
            &ctx,
        )
        .await;

    match result2 {
        Ok(res) => {
            println!("\n✅ Example 2 Result:");
            println!("   Success: {}", res.success);
            println!("   Value: {}\n", res.value);
        }
        Err(e) => {
            eprintln!("\n❌ Example 2 Error: {}\n", e);
        }
    }

    // Example 3: Multiple questions
    println!("📝 Example 3: Mini Survey");
    println!("------------------------");
    let result3 = ccos
        .process_request(
            "conduct a mini survey: ask the user for their name, their age, and their hobby, then summarize the answers",
            &ctx,
        )
        .await;

    match result3 {
        Ok(res) => {
            println!("\n✅ Example 3 Result:");
            println!("   Success: {}", res.success);
            println!("   Value: {}\n", res.value);
        }
        Err(e) => {
            eprintln!("\n❌ Example 3 Error: {}\n", e);
        }
    }

    println!("✨ All examples completed!");
    Ok(())
}
