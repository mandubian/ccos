//! Basic User Interaction Example
//!
//! Demonstrates the simplest human-in-the-loop pattern with CCOS:
//! A plan that asks the user for their name and greets them.
//!
//! Run:
//!   cargo run --example user_interaction_basic
//!   cargo run --example user_interaction_basic -- --debug
//!
//! Delegation / LLM usage (same as live_interactive_assistant):
//!   Env based:
//!     export CCOS_ENABLE_DELEGATION=1
//!     export OPENAI_API_KEY=...
//!     export CCOS_DELEGATING_MODEL=gpt-4o-mini
//!
//!   CLI overrides:
//!     --enable-delegation
//!     --llm-provider openai --llm-model gpt-4o-mini
//!     --llm-provider openrouter --llm-model meta-llama/llama-3-8b-instruct --llm-api-key $OPENROUTER_API_KEY
//!     --llm-provider stub --llm-model deterministic-stub-model (offline)

use clap::Parser;
use rtfs_compiler::ccos::CCOS;
use rtfs_compiler::runtime::security::{RuntimeContext, SecurityLevel};
use std::sync::Arc;

#[derive(Parser, Debug)]
struct Args {
    /// Enable extra internal debug (prints underlying prompts if delegation)
    #[arg(long, default_value_t = false)]
    debug: bool,

    /// Enable delegation explicitly (overrides env detection)
    #[arg(long, default_value_t = false)]
    enable_delegation: bool,

    /// Override LLM provider (openai|openrouter|claude|gemini|stub)
    #[arg(long)]
    llm_provider: Option<String>,

    /// Override LLM model identifier
    #[arg(long)]
    llm_model: Option<String>,

    /// Override API key (if omitted we rely on env var)
    #[arg(long)]
    llm_api_key: Option<String>,

    /// Override base URL (custom/self-hosted proxy)
    #[arg(long)]
    llm_base_url: Option<String>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    
    if args.debug {
        std::env::set_var("RTFS_SHOW_PROMPTS", "1");
    }

    // Apply CLI overrides via env
    if let Some(ref model) = args.llm_model {
        std::env::set_var("CCOS_DELEGATING_MODEL", model);
    }
    if let Some(ref provider) = args.llm_provider {
        std::env::set_var("CCOS_LLM_PROVIDER_HINT", provider);
    }
    if let Some(ref key) = args.llm_api_key {
        let hint = args.llm_provider.as_deref().unwrap_or("openai");
        match hint {
            "openrouter" => std::env::set_var("OPENROUTER_API_KEY", key),
            "claude" => std::env::set_var("ANTHROPIC_API_KEY", key),
            "gemini" => std::env::set_var("GEMINI_API_KEY", key),
            _ => std::env::set_var("OPENAI_API_KEY", key),
        }
    }
    if let Some(ref base) = args.llm_base_url {
        std::env::set_var("CCOS_LLM_BASE_URL", base);
    }
    if args.enable_delegation {
        std::env::set_var("CCOS_ENABLE_DELEGATION", "1");
    }

    println!("🎯 Basic User Interaction Example");
    println!("================================\n");

    // Show delegation status
    if std::env::var("CCOS_ENABLE_DELEGATION").ok().as_deref() == Some("1") {
        let model = std::env::var("CCOS_DELEGATING_MODEL")
            .unwrap_or_else(|_| "(default)".into());
        let provider = std::env::var("CCOS_LLM_PROVIDER_HINT")
            .unwrap_or_else(|_| "(inferred)".into());
        println!("🤖 Delegation: enabled");
        println!("   Provider: {}", provider);
        println!("   Model: {}\n", model);
    } else {
        println!("⚠️  Delegation: disabled (using stub arbiter)");
        println!("   To enable: export CCOS_ENABLE_DELEGATION=1 and set API key\n");
    }

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
