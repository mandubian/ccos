//! Basic User Interaction Example
//!
//! Demonstrates the simplest human-in-the-loop pattern with CCOS:
//! A plan that asks the user for their name and greets them.
//!
//! Run:
//!   cargo run --example user_interaction_basic
//!   cargo run --example user_interaction_basic -- --debug
//!   cargo run --example user_interaction_basic -- --verbose  # Show CCOS process steps
//!   cargo run --example user_interaction_basic -- --enable-delegation --verbose  # Full LLM visibility
//!
//! Configuration (same as live_interactive_assistant):
//!   
//!   Env based:
//!     export CCOS_ENABLE_DELEGATION=1
//!     export OPENAI_API_KEY=...
//!     export CCOS_DELEGATING_MODEL=gpt-4o-mini
//!     cargo run --example user_interaction_basic
//!
//!   CLI overrides:
//!     --enable-delegation
//!     --llm-provider openai --llm-model gpt-4o-mini
//!     --llm-provider openrouter --llm-model meta-llama/llama-3-8b-instruct --llm-api-key $OPENROUTER_API_KEY
//!     --llm-provider stub --llm-model deterministic-stub-model (offline)
//!
//!   Config file (with profiles, model_sets, auto-selection):
//!     --config path/to/agent_config.json
//!     --config path/to/agent_config.toml
//!     --model-auto-prompt-budget 0.001
//!     --model-auto-completion-budget 0.003

use clap::Parser;
use crossterm::style::Stylize;
use rtfs_compiler::ccos::CCOS;
use rtfs_compiler::config::types::{AgentConfig, LlmProfile};
use rtfs_compiler::config::validation::validate_config;
use rtfs_compiler::config::{auto_select_model, expand_profiles};
use rtfs_compiler::runtime::security::{RuntimeContext, SecurityLevel};
use serde_json;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::time::sleep;
use toml;

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

    /// Load agent config (JSON or TOML) with optional llm_profiles
    #[arg(long)]
    config: Option<String>,

    /// Auto-pick best model within prompt cost budget (USD per 1K tokens)
    #[arg(long)]
    model_auto_prompt_budget: Option<f64>,

    /// Auto-pick best model within completion cost budget (USD per 1K tokens)
    #[arg(long)]
    model_auto_completion_budget: Option<f64>,

    /// Show detailed process steps
    #[arg(long, default_value_t = false)]
    verbose: bool,
}

/// Display a spinner while waiting for LLM response
async fn show_progress(message: &str, enabled: bool) {
    if !enabled {
        return;
    }
    
    let spinner_chars = vec!["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    print!("\r{} {} ", "🤖".cyan(), message.blue());
    use std::io::{self, Write};
    io::stdout().flush().ok();
    
    for ch in &spinner_chars {
        print!("\r{} {} {} ", "🤖".cyan(), message.blue(), ch.yellow());
        io::stdout().flush().ok();
        sleep(Duration::from_millis(80)).await;
    }
}

/// Process a request with progress indicators
async fn process_with_progress(
    ccos: &Arc<CCOS>,
    request: &str,
    ctx: &RuntimeContext,
    delegation_enabled: bool,
    verbose: bool,
) -> Result<rtfs_compiler::ccos::types::ExecutionResult, Box<dyn std::error::Error>> {
    if delegation_enabled && verbose {
        println!("\n{}", "┌─ CCOS Processing ─────────────────────────────┐".cyan());
        let request_msg = format!("📝 Request: {}", request);
        println!("{} {}", "│".cyan(), request_msg.white());
        println!("{}", "├───────────────────────────────────────────────┤".cyan());
        println!("{} {}", "│".cyan(), "🔍 Analyzing intent...".blue());
        
        let start = Instant::now();
        
        // Show spinner in background (non-blocking simulation)
        let spinner_task = tokio::spawn(async move {
            for _ in 0..3 {
                show_progress("Building intent graph", true).await;
            }
        });
        
        // Small delay to show spinner
        sleep(Duration::from_millis(100)).await;
        
        println!("{} {}", "│".cyan(), "🧠 Delegating to LLM for plan generation...".yellow());
        println!("{} {}", "│".cyan(), "⚙️  Compiling plan to WASM...".dark_grey());
        println!("{}", "├───────────────────────────────────────────────┤".cyan());
        
        let result = ccos.process_request(request, ctx).await;
        
        let elapsed = start.elapsed();
        
        match &result {
            Ok(res) => {
                let msg = format!("✅ Execution complete ({:.2}s)", elapsed.as_secs_f64());
                println!("{} {}", "│".cyan(), msg.green());
                if res.success {
                    let result_msg = format!("📤 Result: {}", res.value);
                    println!("{} {}", "│".cyan(), result_msg.white());
                } else {
                    let partial_msg = format!("⚠️  Partial success: {}", res.value);
                    println!("{} {}", "│".cyan(), partial_msg.yellow());
                }
            }
            Err(e) => {
                let error_msg = format!("❌ Error: {}", e);
                println!("{} {}", "│".cyan(), error_msg.red());
            }
        }
        
        println!("{}", "└───────────────────────────────────────────────┘".cyan());
        
        // Clean up spinner task
        spinner_task.abort();
        
        result.map_err(|e| e.into())
    } else if delegation_enabled {
        // Simple progress without verbose
        print!("🤖 Processing");
        use std::io::{self, Write};
        io::stdout().flush().ok();
        
        let result = ccos.process_request(request, ctx).await;
        
        print!("\r");
        io::stdout().flush().ok();
        
        result.map_err(|e| e.into())
    } else {
        // No delegation, just process
        ccos.process_request(request, ctx).await.map_err(|e| e.into())
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    
    if args.debug {
        std::env::set_var("RTFS_SHOW_PROMPTS", "1");
    }

    // Load config file (if provided) and extract LLM profiles
    let mut loaded_config: Option<AgentConfig> = None;
    if let Some(cfg_path) = &args.config {
        match load_agent_config(cfg_path) {
            Ok(cfg) => {
                if cfg.llm_profiles.is_some() {
                    println!(
                        "[config] loaded {} LLM profiles",
                        cfg.llm_profiles.as_ref().unwrap().profiles.len()
                    );
                }
                loaded_config = Some(cfg);
            }
            Err(e) => {
                eprintln!("[config] failed to load {}: {}", cfg_path, e);
            }
        }
    }

    // Prepare expanded profile catalog (explicit + model_sets) early for potential auto-selection
    let (expanded_profiles, profile_meta, expansion_rationale) = if let Some(cfg) = &loaded_config {
        expand_profiles(cfg)
    } else {
        (Vec::new(), HashMap::new(), String::from(""))
    };
    if !expansion_rationale.is_empty() {
        println!("[config] profiles expanded:\n{}", expansion_rationale);
    }

    if let Some(cfg) = &loaded_config {
        let report = validate_config(cfg);
        if !report.messages.is_empty() {
            println!("[config] validation ({} messages):", report.messages.len());
            for m in &report.messages {
                println!(
                    "  - [{}] {}{}",
                    match m.level {
                        rtfs_compiler::config::validation::ValidationLevel::Info => "INFO",
                        rtfs_compiler::config::validation::ValidationLevel::Warning => "WARN",
                        rtfs_compiler::config::validation::ValidationLevel::Error => "ERROR",
                    },
                    m.message,
                    m.suggestion
                        .as_ref()
                        .map(|s| format!(" (suggestion: {})", s))
                        .unwrap_or_default()
                );
            }
        }
    }

    // If no explicit CLI model/provider, attempt auto-pick by budgets; else fallback to configured default profile
    if args.llm_model.is_none() && args.llm_provider.is_none() {
        let mut applied = false;
        if args.model_auto_prompt_budget.is_some() || args.model_auto_completion_budget.is_some() {
            let (best, rationale) = auto_select_model(
                &expanded_profiles,
                &profile_meta,
                args.model_auto_prompt_budget,
                args.model_auto_completion_budget,
                None,
            );
            if let Some(best) = best {
                println!("[model-auto] rationale:\n{}", rationale);
                apply_profile_env(best);
                std::env::set_var("CCOS_ENABLE_DELEGATION", "1");
                applied = true;
            } else {
                println!("[model-auto] rationale:\n{}", rationale);
                println!("[model-auto] no model satisfied given budgets");
            }
        }
        if !applied {
            // fallback to top-level default or first set default
            if let Some(cfg) = &loaded_config {
                if let Some(llm_cfg) = &cfg.llm_profiles {
                    if let Some(default_name) = &llm_cfg.default {
                        if let Some(p) = expanded_profiles.iter().find(|p| &p.name == default_name) {
                            apply_profile_env(p);
                            std::env::set_var("CCOS_ENABLE_DELEGATION", "1");
                        }
                    } else {
                        // fallback: use first expanded profile if any
                        if let Some(p) = expanded_profiles.first() {
                            apply_profile_env(p);
                            std::env::set_var("CCOS_ENABLE_DELEGATION", "1");
                        }
                    }
                }
            }
        }
    }

    // Apply CLI overrides via env (overrides config)
    if let Some(ref model) = args.llm_model {
        std::env::set_var("CCOS_DELEGATING_MODEL", model);
    }
    if let Some(ref provider) = args.llm_provider {
        std::env::set_var("CCOS_LLM_PROVIDER_HINT", provider);
        // Provide generic provider env for arbiter routing
        std::env::set_var("CCOS_LLM_PROVIDER", provider);
        if provider == "stub" {
            // Ensure a deterministic stub model name if user omitted --llm-model
            if args.llm_model.is_none() && std::env::var("CCOS_DELEGATING_MODEL").is_err() {
                std::env::set_var("CCOS_DELEGATING_MODEL", "deterministic-stub-model");
            }
            // Force RTFS intent format to test local pipeline without network
            std::env::set_var("CCOS_INTENT_FORMAT", "rtfs");
        }
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
    let delegation_enabled = std::env::var("CCOS_ENABLE_DELEGATION").ok().as_deref() == Some("1");
    if delegation_enabled {
        let model = std::env::var("CCOS_DELEGATING_MODEL")
            .unwrap_or_else(|_| "(default)".into());
        let provider = std::env::var("CCOS_LLM_PROVIDER_HINT")
            .unwrap_or_else(|_| "(inferred)".into());
        println!("🤖 Delegation: enabled");
        println!("   Provider: {}", provider);
        println!("   Model: {}\n", model);
    } else {
        println!("⚠️  Delegation: disabled (using stub arbiter)");
        println!("   Note: Stub arbiter generates simple predetermined plans");
        println!("   For dynamic conversational plans with user input, enable delegation:");
        println!("     export CCOS_ENABLE_DELEGATION=1");
        println!("     export OPENAI_API_KEY=your_key");
        println!("   Or use CLI: --enable-delegation --llm-provider openai --llm-api-key $KEY\n");
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
    if !delegation_enabled {
        println!("💡 Tip: This example works best with delegation enabled");
    }
    
    let result1 = process_with_progress(
        &ccos,
        "ask the user for their name and greet them personally",
        &ctx,
        delegation_enabled,
        args.verbose,
    )
    .await;

    match result1 {
        Ok(res) => {
            if !args.verbose {
                println!("\n✅ Example 1 Result:");
                println!("   Success: {}", res.success);
                println!("   Value: {}\n", res.value);
            }
        }
        Err(e) => {
            eprintln!("\n❌ Example 1 Error: {}", e);
            if !delegation_enabled {
                eprintln!("   💡 This error may be due to stub arbiter limitations.");
                eprintln!("   Try enabling delegation for better plan generation.\n");
            } else {
                eprintln!();
            }
        }
    }

    // Example 2: Ask for favorite color
    println!("📝 Example 2: Favorite Color");
    println!("---------------------------");
    
    let result2 = process_with_progress(
        &ccos,
        "ask the user what their favorite color is and tell them it's a great choice",
        &ctx,
        delegation_enabled,
        args.verbose,
    )
    .await;

    match result2 {
        Ok(res) => {
            if !args.verbose {
                println!("\n✅ Example 2 Result:");
                println!("   Success: {}", res.success);
                println!("   Value: {}\n", res.value);
            }
        }
        Err(e) => {
            eprintln!("\n❌ Example 2 Error: {}\n", e);
        }
    }

    // Example 3: Multiple questions
    println!("📝 Example 3: Mini Survey");
    println!("------------------------");
    
    let result3 = process_with_progress(
        &ccos,
        "conduct a mini survey: ask the user for their name, their age, and their hobby, then summarize the answers",
        &ctx,
        delegation_enabled,
        args.verbose,
    )
    .await;

    match result3 {
        Ok(res) => {
            if !args.verbose {
                println!("\n✅ Example 3 Result:");
                println!("   Success: {}", res.success);
                println!("   Value: {}\n", res.value);
            }
        }
        Err(e) => {
            eprintln!("\n❌ Example 3 Error: {}\n", e);
        }
    }

    println!("✨ All examples completed!");
    Ok(())
}

// Load AgentConfig from JSON or TOML depending on extension
fn load_agent_config(path: &str) -> Result<AgentConfig, Box<dyn std::error::Error>> {
    let raw = fs::read_to_string(path)?;
    let ext = Path::new(path)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();
    if ext == "toml" || ext == "tml" {
        Ok(toml::from_str(&raw)?)
    } else {
        Ok(serde_json::from_str(&raw)?)
    }
}

fn apply_profile_env(p: &LlmProfile) {
    std::env::set_var("CCOS_DELEGATING_MODEL", &p.model);
    std::env::set_var("CCOS_LLM_PROVIDER_HINT", &p.provider);
    if let Some(url) = &p.base_url {
        std::env::set_var("CCOS_LLM_BASE_URL", url);
    } else if p.provider == "openrouter" {
        // OpenRouter requires its public REST base; provide sane default
        if std::env::var("CCOS_LLM_BASE_URL").is_err() {
            std::env::set_var("CCOS_LLM_BASE_URL", "https://openrouter.ai/api/v1");
        }
    }
    // Key precedence: inline > referenced env variable > pre-existing provider env
    if let Some(inline) = &p.api_key {
        dispatch_key(&p.provider, inline);
    } else if let Some(env_key) = &p.api_key_env {
        if let Ok(v) = std::env::var(env_key) {
            dispatch_key(&p.provider, &v);
        }
    }
    // Provide arbiter-compatible generic provider/model envs when possible
    match p.provider.as_str() {
        "openai" => {
            std::env::set_var("CCOS_LLM_PROVIDER", "openai");
        }
        "claude" | "anthropic" => {
            std::env::set_var("CCOS_LLM_PROVIDER", "anthropic");
        }
        "stub" => {
            std::env::set_var("CCOS_LLM_PROVIDER", "stub");
        }
        "local" => {
            std::env::set_var("CCOS_LLM_PROVIDER", "local");
        }
        _ => { /* openrouter & others not yet first-class in Arbiter LlmConfig */ }
    }
    std::env::set_var("CCOS_LLM_MODEL", &p.model);
}

fn dispatch_key(provider: &str, key: &str) {
    match provider {
        "openrouter" => std::env::set_var("OPENROUTER_API_KEY", key),
        "claude" | "anthropic" => std::env::set_var("ANTHROPIC_API_KEY", key),
        "gemini" => std::env::set_var("GEMINI_API_KEY", key),
        "stub" => { /* no key needed */ }
        _ => std::env::set_var("OPENAI_API_KEY", key),
    }
}
