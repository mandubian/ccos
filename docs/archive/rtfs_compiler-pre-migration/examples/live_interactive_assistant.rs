//! Live Interactive CCOS Assistant Example
//!
//! Demonstrates a realistic human <-> AI loop where each user input is turned
//! into (1) new intents, (2) plans, (3) executions; and we render incremental
//! diffs of the Intent Graph and Causal Chain after every request.
//!
//! Features:
//!  - Uses full `CCOS::process_request` pipeline (Arbiter → Governance Kernel → Orchestrator)
//!  - Shows newly created intents and status changes of existing ones
//!  - Displays newly appended causal chain actions (PlanStarted, CapabilityCall, etc.)
//!  - Simple REPL: type natural language; special commands begin with ':'
//!
//! Try:
//!   cargo run --example live_interactive_assistant
//!   cargo run --example live_interactive_assistant -- --debug
//!
//! Delegation / LLM usage:
//!   Env based (original):
//!     export CCOS_ENABLE_DELEGATION=1
//!     export OPENAI_API_KEY=...   # or OPENROUTER_API_KEY / ANTHROPIC_API_KEY / GEMINI_API_KEY
//!     export CCOS_DELEGATING_MODEL=gpt-4o-mini
//!
//!   CLI overrides (precedence: CLI > env > default):
//!     --enable-delegation
//!     --llm-provider openrouter --llm-model meta-llama/llama-3-8b-instruct --llm-api-key $OPENROUTER_API_KEY
//!     --llm-provider openai --llm-model gpt-4o-mini
//!     --llm-provider claude --llm-model claude-3-haiku-20240307
//!     --llm-provider gemini --llm-model gemini-1.5-flash --llm-api-key $GEMINI_API_KEY
//!     --llm-provider stub --llm-model deterministic-stub-model (offline deterministic)
//!
//!   Discovery helpers:
//!     --list-llm-providers
//!     --list-llm-models
//!
//!   Custom base URL (proxy/self-hosted):
//!     --llm-base-url https://my-proxy.example.com/v1
//!
//! Sample session:
//!   > plan a 2-day trip to Paris focusing on art museums and budget food
//!   > refine itinerary with an evening Seine river activity
//!   > summarize the plan in bullet points
//!
//! Each step will show how intents accumulate and how the causal chain maintains an auditable log.

use atty::Stream;
use clap::Parser;
use crossterm::cursor::{Hide, Show};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::style::{Attribute, Color, Stylize};
use crossterm::terminal::{
    self, disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use rtfs_compiler::ccos::types::ActionType;
use rtfs_compiler::ccos::CCOS;
use rtfs_compiler::config::profile_selection::ProfileMeta;
use rtfs_compiler::config::types::{AgentConfig, LlmProfile};
use rtfs_compiler::config::validation::validate_config;
use rtfs_compiler::config::{auto_select_model, expand_profiles};
use rtfs_compiler::runtime::security::{RuntimeContext, SecurityLevel};
use serde_json::Value as JsonValue;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::sync::Arc;

#[derive(Parser, Debug)]
struct Args {
    /// Enable extra internal debug (prints underlying prompts if delegation)
    #[arg(long, default_value_t = false)]
    debug: bool,

    /// Start with an initial seed request (optional)
    #[arg(long)]
    seed: Option<String>,

    /// Maximum causal actions to show per step (tail)
    #[arg(long, default_value_t = 24)]
    max_actions: usize,

    /// Show full raw execution value (may be large)
    #[arg(long, default_value_t = false)]
    show_full_value: bool,

    /// Truncate value preview length when not showing full value
    #[arg(long, default_value_t = 160)]
    value_preview: usize,

    /// Show generated plan RTFS body (truncated unless --plan-full)
    #[arg(long, default_value_t = false)]
    show_plan: bool,

    /// Show full plan body (overrides plan-preview-len)
    #[arg(long, default_value_t = false)]
    plan_full: bool,

    /// Truncate plan preview length
    #[arg(long, default_value_t = 280)]
    plan_preview_len: usize,

    /// Show intent diff automatically after each request
    #[arg(long, default_value_t = false)]
    show_intents: bool,

    /// Show causal chain tail automatically after each request
    #[arg(long, default_value_t = false)]
    show_chain: bool,

    /// Enable delegation explicitly (overrides env detection)
    #[arg(long, default_value_t = false)]
    enable_delegation: bool,

    /// Override LLM provider (openai|openrouter|claude|gemini|stub)
    #[arg(long)]
    llm_provider: Option<String>,

    /// Override LLM model identifier (e.g. gpt-4o-mini, meta-llama/llama-3-8b-instruct)
    #[arg(long)]
    llm_model: Option<String>,

    /// Override API key (if omitted we rely on env var)
    #[arg(long)]
    llm_api_key: Option<String>,

    /// Override base URL (custom/self-hosted proxy)
    #[arg(long)]
    llm_base_url: Option<String>,

    /// List supported LLM providers and exit
    #[arg(long, default_value_t = false)]
    list_llm_providers: bool,

    /// List sample LLM models and exit
    #[arg(long, default_value_t = false)]
    list_llm_models: bool,

    /// Load agent config (JSON or TOML) with optional llm_profiles
    #[arg(long)]
    config: Option<String>,

    /// Auto-pick best model within prompt cost budget (USD per 1K tokens)
    #[arg(long)]
    model_auto_prompt_budget: Option<f64>,

    /// Auto-pick best model within completion cost budget (USD per 1K tokens)
    #[arg(long)]
    model_auto_completion_budget: Option<f64>,

    /// One-shot prompt (if supplied, run a single request then exit)
    #[arg(long)]
    prompt: Option<String>,

    /// Rationale output format (text|json) for profiles & selection (default text)
    #[arg(long, default_value_t = String::from("text"))]
    rationale_format: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    if args.debug {
        std::env::set_var("RTFS_SHOW_PROMPTS", "1");
    }

    if args.list_llm_providers {
        println!("Supported providers:\n  openai\n  openrouter\n  claude (anthropic)\n  gemini (google)\n  stub (deterministic testing)\n");
        return Ok(());
    }
    if args.list_llm_models {
        println!("Sample model slugs (not exhaustive):\n  openai: gpt-4o-mini, gpt-4o, o3-mini\n  openrouter: meta-llama/llama-3-8b-instruct, mistralai/mistral-7b-instruct, openai/gpt-4o-mini\n  claude: claude-3-haiku-20240307, claude-3-sonnet-20240229\n  gemini: gemini-1.5-flash, gemini-1.5-pro\n  stub: stub-model, deterministic-stub-model\n");
        return Ok(());
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
    if !expansion_rationale.is_empty() && args.prompt.is_none() {
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

    // If no explicit CLI model/provider, attempt auto-pick by budgets; else fallback to configured default profile; else do nothing
    // Capture selection rationale optionally for one-shot JSON output
    let mut selection_rationale_text: Option<String> = None;
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
                if args.prompt.is_none() {
                    println!("[model-auto] rationale:\n{}", rationale);
                }
                selection_rationale_text = Some(rationale.clone());
                apply_profile_env(best, /*announce=*/ true);
                std::env::set_var("CCOS_ENABLE_DELEGATION", "1");
                applied = true;
            } else {
                if args.prompt.is_none() {
                    println!("[model-auto] rationale:\n{}", rationale);
                    println!("[model-auto] no model satisfied given budgets");
                }
                selection_rationale_text = Some(rationale.clone());
            }
        }
        if !applied {
            // fallback to top-level default or first set default handled during expansion (marked in meta via default flag)
            if let Some(cfg) = &loaded_config {
                if let Some(llm_cfg) = &cfg.llm_profiles {
                    if let Some(default_name) = &llm_cfg.default {
                        if let Some(p) = expanded_profiles.iter().find(|p| &p.name == default_name)
                        {
                            apply_profile_env(p, /*announce=*/ true);
                            std::env::set_var("CCOS_ENABLE_DELEGATION", "1");
                        }
                    } else {
                        // fallback: use first expanded profile if any
                        if let Some(p) = expanded_profiles.first() {
                            apply_profile_env(p, /*announce=*/ true);
                            std::env::set_var("CCOS_ENABLE_DELEGATION", "1");
                        }
                    }
                }
            }
        }
    }

    // Apply CLI overrides via env so existing initialization code picks them up (overrides config)
    if let Some(ref model) = args.llm_model {
        std::env::set_var("CCOS_DELEGATING_MODEL", model);
    }
    if let Some(ref provider) = args.llm_provider {
        // We don't have a direct provider env yet; provider type is inferred from base_url + key.
        // For explicit provider guidance we set helper env used only here.
        std::env::set_var("CCOS_LLM_PROVIDER_HINT", provider);
        // Provide a direct provider env for Arbiter if supported
        match provider.as_str() {
            "openai" => {
                std::env::set_var("CCOS_LLM_PROVIDER", "openai");
            }
            "claude" | "anthropic" => {
                std::env::set_var("CCOS_LLM_PROVIDER", "anthropic");
            }
            "gemini" => {
                std::env::set_var("CCOS_LLM_PROVIDER", "gemini");
            }
            "stub" => {
                std::env::set_var("CCOS_LLM_PROVIDER", "stub");
            }
            _ => { /* openrouter & others may be inferred later */ }
        }
    }
    if let Some(ref key) = args.llm_api_key {
        // Decide which env to set based on provider hint (fallback openai)
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

    // Offline deterministic path: if stub provider selected (explicitly or via hint) ensure sensible defaults
    let provider_is_stub = args
        .llm_provider
        .as_deref()
        .map(|p| p.eq_ignore_ascii_case("stub"))
        .unwrap_or(false)
        || std::env::var("CCOS_LLM_PROVIDER_HINT")
            .map(|v| v == "stub")
            .unwrap_or(false);
    if provider_is_stub {
        // Always prefer RTFS intent format for stub to exercise primary code path while offline
        std::env::set_var("CCOS_INTENT_FORMAT", "rtfs");
        // Enable delegation so intent generation path executes, unless user explicitly disabled (no explicit disable flag yet)
        if std::env::var("CCOS_ENABLE_DELEGATION").ok().as_deref() != Some("1") {
            std::env::set_var("CCOS_ENABLE_DELEGATION", "1");
        }
        // Default deterministic model if user didn't supply one (env or CLI)
        let has_model = args.llm_model.is_some()
            || std::env::var("CCOS_DELEGATING_MODEL").is_ok()
            || std::env::var("CCOS_DELEGATION_MODEL").is_ok();
        if !has_model {
            std::env::set_var("CCOS_DELEGATING_MODEL", "deterministic-stub-model");
        }
    }

    if args.prompt.is_none() {
        let banner = r#"
╔═══════════════════════════════════════════════════════════════════════════════╗
║                                                                               ║
║            🧪  CCOS Live Interactive Assistant                                ║
║                                                                               ║
║            Runtime-First Scripting with Intent-Driven Orchestration          ║
║                                                                               ║
╚═══════════════════════════════════════════════════════════════════════════════╝
"#;
        println!("{}", banner.cyan());
        println!(
            "{}",
            "  💡 Type natural language goals or commands starting with ':'".dark_grey()
        );
        println!("{}", "  📖 Use :help for command reference\n".dark_grey());
    }

    // Build CCOS with debug callback capturing plan lifecycle events
    let (dbg_tx, mut dbg_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let debug_cb = Arc::new(move |line: String| {
        // filter only our plan events or request events to avoid spamming with capability registration duplicates
        if line.contains("plan_")
            || line.contains("request_received")
            || line.contains("plan_execution")
        {
            let _ = dbg_tx.send(line);
        }
    });
    let mut ccos = Arc::new(CCOS::new_with_debug_callback(Some(debug_cb.clone())).await?);

    // Keep expanded profiles & metadata in memory for :models / :model / :model-auto
    // Retain for potential future logic (reserved) but currently unused beyond determining active profile.
    let mut active_profile: Option<String> =
        std::env::var("CCOS_DELEGATING_MODEL").ok().and_then(|m| {
            expanded_profiles
                .iter()
                .find(|p| p.model == m)
                .map(|p| p.name.clone())
        });

    // Post-init summary of LLM config if delegation active
    if std::env::var("CCOS_ENABLE_DELEGATION").ok().as_deref() == Some("1")
        || std::env::var("CCOS_DELEGATION_ENABLED").ok().as_deref() == Some("1")
    {
        let model = std::env::var("CCOS_DELEGATING_MODEL")
            .or_else(|_| std::env::var("CCOS_DELEGATION_MODEL"))
            .unwrap_or_else(|_| "(default)".into());
        let provider_hint =
            std::env::var("CCOS_LLM_PROVIDER_HINT").unwrap_or_else(|_| "(inferred)".into());
        println!(
            "[delegation] provider_hint={} model={} (override precedence: CLI > env > default)",
            provider_hint, model
        );
        if args.debug {
            let base = std::env::var("CCOS_LLM_BASE_URL").unwrap_or_else(|_| "(none)".into());
            let key_src =
                std::env::var("CCOS_LLM_KEY_SOURCE").unwrap_or_else(|_| "(unknown)".into());
            let arb_provider =
                std::env::var("CCOS_LLM_PROVIDER").unwrap_or_else(|_| "(unset)".into());
            println!("[delegation.debug] resolved provider={} (arbiter) hint={} model={} base_url={} key_source={}", arb_provider, provider_hint, model, base, key_src);
        }
        // Helpful guidance for OpenRouter free-tier models that require a privacy/data policy setting.
        if provider_hint == "openrouter"
            && model.contains(":free")
            && std::env::var("CCOS_SUPPRESS_OPENROUTER_HINT").is_err()
        {
            eprintln!("[openrouter] Detected free-tier model '{}'. If you encounter 404: 'No endpoints found matching your data policy (Free model publication)', configure your privacy settings at https://openrouter.ai/settings/privacy (enable free model publication) or choose a non-free model. Set CCOS_SUPPRESS_OPENROUTER_HINT=1 to hide this message.", model);
        }
    }

    // Security context: fairly permissive for demo, restrict to safe capabilities
    let ctx = RuntimeContext {
        security_level: SecurityLevel::Controlled,
        allowed_capabilities: vec![
            "ccos.echo".to_string(),
            "ccos.math.add".to_string(),
            "ccos.network.http-fetch".to_string(), // may be filtered if registry not mocked
            "ccos.user.ask".to_string(),           // interactive user input
        ]
        .into_iter()
        .collect(),
        ..RuntimeContext::pure()
    };

    // Snapshots for diffing
    let mut known_intents: HashMap<String, IntentSnapshot> = HashMap::new();
    let mut last_action_count: usize = 0;

    if let Some(seed) = args.seed.clone() {
        process_and_render(
            &ccos,
            &ctx,
            &seed,
            &mut known_intents,
            &mut last_action_count,
            args.max_actions,
            &mut dbg_rx,
            args.show_full_value,
            args.value_preview,
            args.show_plan,
            args.plan_full,
            args.plan_preview_len,
            args.show_intents,
            args.show_chain,
        )
        .await;
    }

    // One-shot prompt path
    if let Some(prompt) = args.prompt.clone() {
        // Process single request
        let mut last_action_count_local = last_action_count;
        process_and_render(
            &ccos,
            &ctx,
            &prompt,
            &mut known_intents,
            &mut last_action_count_local,
            args.max_actions,
            &mut dbg_rx,
            args.show_full_value,
            args.value_preview,
            args.show_plan,
            args.plan_full,
            args.plan_preview_len,
            args.show_intents,
            args.show_chain,
        )
        .await;

        // Output rationale in requested format (if any)
        match args.rationale_format.as_str() {
            "json" => {
                use serde_json::json;
                let obj = json!({
                    "expansion_rationale": expansion_rationale.lines().collect::<Vec<_>>(),
                    "selection_rationale": selection_rationale_text.as_ref().map(|s| s.lines().collect::<Vec<_>>()),
                    "active_model": std::env::var("CCOS_DELEGATING_MODEL").ok(),
                });
                println!("{}", serde_json::to_string_pretty(&obj)?);
            }
            _ => {
                if !expansion_rationale.is_empty() {
                    println!("[rationale] expansion:\n{}", expansion_rationale);
                }
                if let Some(sel) = selection_rationale_text {
                    println!("[rationale] selection:\n{}", sel);
                }
            }
        }
        return Ok(());
    }

    // REPL loop (interactive mode only)
    let stdin = io::stdin();
    loop {
        print!("> ");
        let _ = io::stdout().flush();
        let mut line = String::new();
        if stdin.read_line(&mut line).is_err() {
            break;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.starts_with(':') {
            match trimmed {
                ":quit" | ":q" | ":exit" => {
                    println!("👋 Bye");
                    break;
                }
                ":help" => print_help(),
                ":intents" => render_full_intents(&ccos),
                ":chain" => {
                    last_action_count = render_recent_actions(
                        &ccos,
                        last_action_count,
                        args.max_actions,
                        args.value_preview,
                        true,
                    );
                }
                ":models" => {
                    if expanded_profiles.is_empty() {
                        println!("(no profiles loaded; supply --config with llm_profiles)");
                    } else {
                        println!("📦 LLM Profiles ({} total):", expanded_profiles.len());
                        for p in &expanded_profiles {
                            let star = if Some(&p.name) == active_profile.as_ref() {
                                "*"
                            } else {
                                " "
                            };
                            if let Some(meta) = profile_meta.get(&p.name) {
                                println!(
                                    " {} {} | provider={} model={}{}{}{}{}",
                                    star,
                                    p.name,
                                    p.provider,
                                    p.model,
                                    p.base_url
                                        .as_ref()
                                        .map(|u| format!(" url={}", u))
                                        .unwrap_or_default(),
                                    meta.prompt_cost
                                        .map(|c| format!(" prompt_cost={:.2}", c))
                                        .unwrap_or_default(),
                                    meta.completion_cost
                                        .map(|c| format!(" completion_cost={:.2}", c))
                                        .unwrap_or_default(),
                                    meta.quality
                                        .as_ref()
                                        .map(|q| format!(" quality={}", q))
                                        .unwrap_or_default()
                                );
                            } else {
                                println!(
                                    " {} {} | provider={} model={}",
                                    star, p.name, p.provider, p.model
                                );
                            }
                        }
                        println!(
                            "Use :model <name> or :model-auto prompt=.. completion=.. [quality=..]"
                        );
                        if !atty::is(Stream::Stdout) {
                            println!(
                                "(interactive picker requires a TTY; use :model <name> to switch manually)"
                            );
                        } else {
                            match interactive_profile_select(
                                &expanded_profiles,
                                &profile_meta,
                                active_profile.as_deref(),
                            ) {
                                Ok(Some(choice)) => {
                                    if let Some(profile) = expanded_profiles.get(choice) {
                                        apply_profile_env(profile, /*announce=*/ false);
                                        std::env::set_var("CCOS_ENABLE_DELEGATION", "1");
                                        match CCOS::new_with_debug_callback(Some(debug_cb.clone())).await {
                                            Ok(new_ccos) => {
                                                ccos = Arc::new(new_ccos);
                                                active_profile = Some(profile.name.clone());
                                                println!(
                                                    "[models] selected '{}' provider={} model={}",
                                                    profile.name, profile.provider, profile.model
                                                );
                                            }
                                            Err(e) => println!(
                                                "[models] failed to rebuild CCOS with new profile: {}",
                                                e
                                            ),
                                        }
                                    }
                                }
                                Ok(None) => {
                                    println!("[models] interactive picker cancelled");
                                }
                                Err(e) => {
                                    println!("[models] interactive picker error: {}", e);
                                }
                            }
                        }
                    }
                }
                cmd if cmd.starts_with(":model ") => {
                    let parts: Vec<&str> = cmd.split_whitespace().collect();
                    if parts.len() == 2 {
                        let target = parts[1];
                        if let Some(profile) = expanded_profiles.iter().find(|p| p.name == target) {
                            apply_profile_env(profile, /*announce=*/ false);
                            std::env::set_var("CCOS_ENABLE_DELEGATION", "1");
                            match CCOS::new_with_debug_callback(Some(debug_cb.clone())).await {
                                Ok(new_ccos) => {
                                    ccos = Arc::new(new_ccos);
                                    active_profile = Some(profile.name.clone());
                                    println!(
                                        "[models] switched to profile '{}' provider={} model={}",
                                        profile.name, profile.provider, profile.model
                                    );
                                }
                                Err(e) => println!(
                                    "[models] failed to rebuild CCOS with new profile: {}",
                                    e
                                ),
                            }
                        } else {
                            println!("[models] profile '{}' not found", target);
                        }
                    } else {
                        println!("Usage: :model <profile-name>");
                    }
                }
                cmd if cmd.starts_with(":model-auto") => {
                    let args_str = cmd[":model-auto".len()..].trim();
                    let mut prompt_budget: Option<f64> = None;
                    let mut completion_budget: Option<f64> = None;
                    let mut min_quality: Option<String> = None;
                    for tok in args_str.split_whitespace() {
                        if let Some(rest) = tok.strip_prefix("prompt=") {
                            prompt_budget = rest.parse().ok();
                        } else if let Some(rest) = tok.strip_prefix("completion=") {
                            completion_budget = rest.parse().ok();
                        } else if let Some(rest) = tok.strip_prefix("quality=") {
                            min_quality = Some(rest.to_string());
                        }
                    }
                    if prompt_budget.is_none()
                        && completion_budget.is_none()
                        && min_quality.is_none()
                    {
                        println!("Usage: :model-auto prompt=<usd_per_1k> completion=<usd_per_1k> [quality=<tier>]");
                    } else {
                        let (best, rationale) = auto_select_model(
                            &expanded_profiles,
                            &profile_meta,
                            prompt_budget,
                            completion_budget,
                            min_quality.as_deref(),
                        );
                        println!("[model-auto] rationale:\n{}", rationale);
                        if let Some(best) = best {
                            apply_profile_env(best, /*announce=*/ false);
                            std::env::set_var("CCOS_ENABLE_DELEGATION", "1");
                            match CCOS::new_with_debug_callback(Some(debug_cb.clone())).await {
                                Ok(new_ccos) => {
                                    active_profile = Some(best.name.clone());
                                    ccos = Arc::new(new_ccos);
                                    println!(
                                        "[model-auto] selected '{}' provider={} model={}",
                                        best.name, best.provider, best.model
                                    );
                                }
                                Err(e) => println!("[model-auto] failed to rebuild CCOS: {}", e),
                            }
                        } else {
                            println!("[model-auto] no model matched the criteria");
                        }
                    }
                }
                other => println!("Unknown command: {}", other),
            }
            continue;
        }
        process_and_render(
            &ccos,
            &ctx,
            trimmed,
            &mut known_intents,
            &mut last_action_count,
            args.max_actions,
            &mut dbg_rx,
            args.show_full_value,
            args.value_preview,
            args.show_plan,
            args.plan_full,
            args.plan_preview_len,
            args.show_intents,
            args.show_chain,
        )
        .await;
    }
    Ok(())
}

fn interactive_profile_select(
    profiles: &[LlmProfile],
    profile_meta: &HashMap<String, ProfileMeta>,
    active_profile: Option<&str>,
) -> std::io::Result<Option<usize>> {
    if profiles.is_empty() {
        return Ok(None);
    }
    if !atty::is(Stream::Stdout) {
        return Ok(None);
    }

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    let enter_result = crossterm::execute!(stdout, EnterAlternateScreen, Hide);
    if let Err(e) = enter_result {
        disable_raw_mode()?;
        return Err(e);
    }

    let result = (|| -> std::io::Result<Option<usize>> {
        let mut selected = active_profile
            .and_then(|name| profiles.iter().position(|p| p.name == name))
            .unwrap_or(0);
        if selected >= profiles.len() {
            selected = 0;
        }
        draw_profile_picker(
            &mut stdout,
            profiles,
            profile_meta,
            selected,
            active_profile,
        )?;
        loop {
            match event::read()? {
                Event::Key(KeyEvent {
                    code,
                    modifiers,
                    kind,
                    ..
                }) => {
                    if kind != KeyEventKind::Press {
                        continue;
                    }
                    if modifiers.contains(KeyModifiers::CONTROL)
                        && matches!(code, KeyCode::Char('c') | KeyCode::Char('d'))
                    {
                        return Ok(None);
                    }
                    match code {
                        KeyCode::Up => {
                            if selected == 0 {
                                selected = profiles.len() - 1;
                            } else {
                                selected -= 1;
                            }
                            draw_profile_picker(
                                &mut stdout,
                                profiles,
                                profile_meta,
                                selected,
                                active_profile,
                            )?;
                        }
                        KeyCode::Down => {
                            selected = (selected + 1) % profiles.len();
                            draw_profile_picker(
                                &mut stdout,
                                profiles,
                                profile_meta,
                                selected,
                                active_profile,
                            )?;
                        }
                        KeyCode::Enter => return Ok(Some(selected)),
                        KeyCode::Esc => return Ok(None),
                        _ => {}
                    }
                }
                Event::Resize(_, _) => {
                    draw_profile_picker(
                        &mut stdout,
                        profiles,
                        profile_meta,
                        selected,
                        active_profile,
                    )?;
                }
                _ => {}
            }
        }
    })();

    let leave_result = crossterm::execute!(stdout, Show, LeaveAlternateScreen);
    let raw_off_result = disable_raw_mode();
    leave_result?;
    raw_off_result?;
    result
}

fn draw_profile_picker(
    stdout: &mut std::io::Stdout,
    profiles: &[LlmProfile],
    profile_meta: &HashMap<String, ProfileMeta>,
    selected: usize,
    active_profile: Option<&str>,
) -> std::io::Result<()> {
    use std::io::Write;

    crossterm::execute!(
        stdout,
        terminal::Clear(terminal::ClearType::All),
        crossterm::cursor::MoveTo(0, 0)
    )?;

    write!(stdout, "{}\r\n", "🧠 Select an LLM profile".bold())?;
    write!(
        stdout,
        "{}\r\n",
        "Use ↑/↓ arrows to navigate, Enter to select, Esc to cancel.".dark_grey()
    )?;
    write!(stdout, "\r\n")?;

    for (idx, profile) in profiles.iter().enumerate() {
        let is_selected = idx == selected;
        let is_active = active_profile == Some(profile.name.as_str());

        // Build compact detail string
        let mut detail_parts: Vec<String> = Vec::new();
        if let Some(meta) = profile_meta.get(&profile.name) {
            if let Some(cost) = meta.prompt_cost {
                if cost > 0.0 {
                    detail_parts.push(format!("p${:.2}", cost));
                }
            }
            if let Some(cost) = meta.completion_cost {
                if cost > 0.0 {
                    detail_parts.push(format!("c${:.2}", cost));
                }
            }
            if let Some(q) = &meta.quality {
                detail_parts.push(format!("q:{}", q));
            }
        }

        let details = if detail_parts.is_empty() {
            String::new()
        } else {
            format!(" [{}]", detail_parts.join(" "))
        };

        let active_mark = if is_active { " ⭐" } else { "" };
        let cursor = if is_selected { "➤" } else { " " };

        // Format: cursor name | provider=X model=Y [details] active_mark
        let line = format!(
            "{} {} | {}={} {}={}{}{}",
            cursor,
            profile.name,
            "provider".dark_grey(),
            profile.provider,
            "model".dark_grey(),
            profile.model,
            details.dark_grey(),
            active_mark
        );

        if is_selected {
            write!(
                stdout,
                "{}\r\n",
                line.with(Color::Cyan).attribute(Attribute::Bold)
            )?;
        } else if is_active {
            write!(stdout, "{}\r\n", line.green())?;
        } else {
            write!(stdout, "{}\r\n", line)?;
        }
    }

    stdout.flush()?;
    Ok(())
}

#[derive(Clone)]
struct IntentSnapshot {
    status: String,
    _goal: String,
    _name: Option<String>,
}

async fn process_and_render(
    ccos: &Arc<CCOS>,
    ctx: &RuntimeContext,
    request: &str,
    known_intents: &mut HashMap<String, IntentSnapshot>,
    last_action_count: &mut usize,
    max_actions: usize,
    dbg_rx: &mut tokio::sync::mpsc::UnboundedReceiver<String>,
    show_full_value: bool,
    value_preview: usize,
    show_plan: bool,
    plan_full: bool,
    plan_preview_len: usize,
    show_intents: bool,
    show_chain: bool,
) {
    // Enhanced request display
    let req_border = "━".repeat(80);
    println!("\n{}", req_border.blue());
    println!(
        "{} {}",
        "➡️  REQUEST:".blue().bold(),
        request.white().bold()
    );
    let req_border2 = "━".repeat(80);
    println!("{}", req_border2.blue());
    let mut timeline = TimelinePrinter::new(ccos, value_preview);
    let mut channel_open = true;
    let request_future = ccos.process_request_with_plan(request, ctx);
    tokio::pin!(request_future);
    let mut outcome: Option<
        Result<
            (
                rtfs_compiler::ccos::types::Plan,
                rtfs_compiler::ccos::types::ExecutionResult,
            ),
            rtfs_compiler::runtime::error::RuntimeError,
        >,
    > = None;

    while outcome.is_none() {
        tokio::select! {
            maybe_line = dbg_rx.recv(), if channel_open => {
                match maybe_line {
                    Some(line) => timeline.handle(&line, ccos),
                    None => channel_open = false,
                }
            }
            res = &mut request_future => {
                outcome = Some(res);
            }
        }
    }

    // Flush any remaining debug events
    while let Ok(line) = dbg_rx.try_recv() {
        timeline.handle(&line, ccos);
    }

    timeline.flush_new_actions(ccos);

    match outcome.expect("request future completed") {
        Ok((plan, res)) => {
            // Display plan with enhanced formatting
            if show_plan {
                match &plan.body {
                    rtfs_compiler::ccos::types::PlanBody::Rtfs(src) => {
                        render_plan_box(&plan.plan_id, src, plan_full, plan_preview_len);
                    }
                    rtfs_compiler::ccos::types::PlanBody::Wasm(bytes) => {
                        render_wasm_plan_box(&plan.plan_id, bytes.len());
                    }
                }
            }

            // Display execution result with enhanced formatting
            render_execution_result(&res, show_full_value, value_preview);
        }
        Err(e) => {
            render_error_box(&e.to_string());
        }
    }

    // Diff intents (optional output)
    diff_and_render_intents(ccos, known_intents, show_intents);
    // Diff causal chain (optional output)
    let new_count = render_recent_actions(
        ccos,
        *last_action_count,
        max_actions,
        value_preview,
        show_chain,
    );
    *last_action_count = new_count;

    // Enhanced closing divider
    let divider = "═".repeat(80);
    println!("\n{}\n", divider.dark_grey());
}

fn diff_and_render_intents(
    ccos: &Arc<CCOS>,
    known: &mut HashMap<String, IntentSnapshot>,
    show_output: bool,
) {
    let ig = ccos.get_intent_graph();
    let g = ig.lock().unwrap();
    let all = g.storage.get_all_intents_sync();

    // First pass: collect discoveries and changes without mutating known to avoid borrow conflicts
    struct StatusChange {
        id: String,
        old: String,
        new: String,
    }
    let mut new_intents: Vec<rtfs_compiler::ccos::types::StorableIntent> = Vec::new();
    let mut status_changes: Vec<StatusChange> = Vec::new();

    for intent in &all {
        let status_str = format!("{:?}", intent.status);
        if let Some(prev) = known.get(&intent.intent_id) {
            if prev.status != status_str {
                status_changes.push(StatusChange {
                    id: intent.intent_id.clone(),
                    old: prev.status.clone(),
                    new: status_str.clone(),
                });
            }
        } else {
            new_intents.push(intent.clone());
        }
    }

    // Apply mutations after analysis
    for intent in &new_intents {
        known.insert(
            intent.intent_id.clone(),
            IntentSnapshot {
                status: format!("{:?}", intent.status),
                _goal: intent.goal.clone(),
                _name: intent.name.clone(),
            },
        );
    }
    for change in &status_changes {
        if let Some(intent) = all.iter().find(|i| i.intent_id == change.id) {
            known.insert(
                intent.intent_id.clone(),
                IntentSnapshot {
                    status: change.new.clone(),
                    _goal: intent.goal.clone(),
                    _name: intent.name.clone(),
                },
            );
        }
    }

    if show_output {
        if !new_intents.is_empty() {
            println!("\n{}", "─".repeat(80).dark_cyan());
            println!("{}", "🆕 New Intents".bold().cyan());
            println!("{}", "─".repeat(80).dark_cyan());
        }
        for i in &new_intents {
            let status_str = format!("{:?}", i.status);
            let status_colored = match status_str.as_str() {
                "Pending" => status_str.yellow(),
                "Active" => status_str.green(),
                "Completed" => status_str.green().bold(),
                "Failed" => status_str.red(),
                _ => status_str.white(),
            };
            println!(
                "  {} {} {} {}",
                "•".cyan(),
                i.intent_id.as_str().dark_yellow(),
                format!("[{}]", status_colored),
                truncate(&i.goal, 60).white()
            );
        }
        if !status_changes.is_empty() {
            println!("\n{}", "─".repeat(80).dark_cyan());
            println!("{}", "♻️  Intent Status Changes".bold().magenta());
            println!("{}", "─".repeat(80).dark_cyan());
        }
        for ch in status_changes {
            let old_colored = match ch.old.as_str() {
                "Pending" => ch.old.yellow(),
                "Active" => ch.old.green(),
                "Completed" => ch.old.green().bold(),
                "Failed" => ch.old.red(),
                _ => ch.old.white(),
            };
            let new_colored = match ch.new.as_str() {
                "Pending" => ch.new.yellow(),
                "Active" => ch.new.green(),
                "Completed" => ch.new.green().bold(),
                "Failed" => ch.new.red(),
                _ => ch.new.white(),
            };
            println!(
                "  {} {} {} {} {}",
                "•".magenta(),
                ch.id.dark_yellow(),
                old_colored,
                "→".white().bold(),
                new_colored
            );
        }
    }
}

fn render_full_intents(ccos: &Arc<CCOS>) {
    if let Ok(g) = ccos.get_intent_graph().lock() {
        let all = g.storage.get_all_intents_sync();
        let top = format!("╔{}╗", "═".repeat(78));
        let mid = format!("╠{}╣", "═".repeat(78));
        let bot = format!("╚{}╝", "═".repeat(78));
        println!("\n{}", top.cyan());
        let padding = " ".repeat(78 - 15 - all.len().to_string().len() - 3);
        let header = format!(
            "║ {} {} {}║",
            "📚 All Intents".bold().white(),
            format!("({})", all.len()).dark_grey(),
            padding
        );
        println!("{}", header);
        println!("{}", mid.cyan());

        for i in all {
            let status_str = format!("{:?}", i.status);
            let status_colored = match status_str.as_str() {
                "Pending" => status_str.yellow(),
                "Active" => status_str.green(),
                "Completed" => status_str.green().bold(),
                "Failed" => status_str.red(),
                _ => status_str.white(),
            };
            let goal_preview = truncate(&i.goal, 50);
            println!(
                "{} {} {} {}",
                "║".cyan(),
                i.intent_id.dark_yellow(),
                format!("[{}]", status_colored),
                goal_preview.white()
            );
        }
        println!("{}", bot.cyan());
    }
}

fn render_recent_actions(
    ccos: &Arc<CCOS>,
    from_index: usize,
    max_actions: usize,
    value_preview: usize,
    show_output: bool,
) -> usize {
    if let Ok(chain) = ccos.get_causal_chain().lock() {
        let actions = chain.get_all_actions();
        if show_output && actions.len() > from_index {
            println!("\n{}", "─".repeat(80).dark_cyan());
            println!(
                "{} {} {}",
                "🪵 Causal Chain".bold().cyan(),
                format!("(+{} new actions)", actions.len() - from_index).dark_grey(),
                ""
            );
            println!("{}", "─".repeat(80).dark_cyan());
        }
        let start = actions.len().saturating_sub(max_actions);
        if show_output {
            for a in &actions[start..] {
                let action_type_str = format!("{:?}", a.action_type);
                let action_type_colored = match action_type_str.as_str() {
                    "PlanStarted" => action_type_str.magenta(),
                    "PlanCompleted" => action_type_str.green(),
                    "CapabilityCall" => action_type_str.cyan(),
                    "CapabilityResult" => action_type_str.blue(),
                    "IntentCreated" => action_type_str.yellow(),
                    _ => action_type_str.white(),
                };

                let value_suffix = if let Some(res) = &a.result {
                    let full = format!("{}", res.value);
                    let truncated = truncate(&full, value_preview);
                    let success_icon = if res.success {
                        "✓".green()
                    } else {
                        "✗".red()
                    };
                    format!(" {} {}", success_icon, truncated.white())
                } else {
                    String::new()
                };

                let fn_display = a.function_name.as_deref().unwrap_or("-");
                let fn_colored = if fn_display != "-" {
                    fn_display.yellow()
                } else {
                    fn_display.dark_grey()
                };

                println!(
                    "  {} {} {} fn={} intent={} plan={}{}",
                    "•".dark_cyan(),
                    a.action_id.as_str().dark_yellow(),
                    action_type_colored,
                    fn_colored,
                    a.intent_id.as_str().dark_grey(),
                    a.plan_id.as_str().dark_grey(),
                    value_suffix
                );
            }
        }
        actions.len()
    } else {
        from_index
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max])
    }
}

fn print_help() {
    let box_top = "╔═══════════════════════════════════════════════════════════════════════════╗";
    let box_bot = "╚═══════════════════════════════════════════════════════════════════════════╝";
    let box_mid = "╠═══════════════════════════════════════════════════════════════════════════╣";

    println!("{}", box_top.cyan());
    println!(
        "║ {}                                                             ║",
        "📖 CCOS Interactive Assistant Help".bold().white()
    );
    println!("{}", box_mid.cyan());
    println!(
        "║ {}                                                          ║",
        "Commands:".bold().yellow()
    );
    println!(
        "║   {} {}                                              ║",
        ":help".green(),
        "show this help"
    );
    println!(
        "║   {} {}                                       ║",
        ":intents".green(),
        "list all intents"
    );
    println!(
        "║   {} {}                           ║",
        ":chain".green(),
        "show recent causal actions tail"
    );
    println!(
        "║   {} {} ║",
        ":models".green(),
        "list & pick LLM profiles (↑/↓, Enter)"
    );
    println!(
        "║   {} {}                        ║",
        ":model <name>".green(),
        "switch active LLM profile"
    );
    println!(
        "║   {} {}  ║",
        ":model-auto k=v".green(),
        "auto-select model"
    );
    println!(
        "║   {} {}                                           ║",
        ":quit | :q".green(),
        "exit"
    );
    println!("║                                                                           ║");
    println!(
        "║ {}                                                             ║",
        "Flags:".bold().yellow()
    );
    println!(
        "║   {} {}                 ║",
        "--show-plan".blue(),
        "show plan body each request"
    );
    println!(
        "║   {} {}                 ║",
        "--plan-full".blue(),
        "show complete plan body"
    );
    println!(
        "║   {} {}               ║",
        "--show-intents".blue(),
        "auto-show intent diffs"
    );
    println!(
        "║   {} {}            ║",
        "--show-chain".blue(),
        "auto-show causal chain tail"
    );
    println!(
        "║   {} {}      ║",
        "--config <path>".blue(),
        "load agent config (JSON/TOML)"
    );
    println!("{}", box_bot.cyan());
}

/// Render a beautifully formatted plan code box
fn render_plan_box(plan_id: &str, src: &str, full: bool, preview_len: usize) {
    let title = format!(" 📋 Plan: {} ", plan_id);
    let title_len = title.chars().count(); // count actual chars for proper width calculation
    let width = 80;

    println!();
    let top = format!("╔{}╗", "═".repeat(width - 2));
    let mid = format!("╠{}╣", "═".repeat(width - 2));
    let bot = format!("╚{}╝", "═".repeat(width - 2));
    println!("{}", top.cyan());
    let padding = " ".repeat(width.saturating_sub(title_len + 2));
    let header = format!("║{}{}║", title.bold().white(), padding);
    println!("{}", header);
    println!("{}", mid.cyan());

    let display_src = if full {
        src
    } else {
        &src[..src.len().min(preview_len)]
    };
    let lines: Vec<&str> = display_src.lines().collect();
    let max_lines = if full {
        lines.len()
    } else {
        lines.len().min(20)
    };

    for (idx, line) in lines.iter().take(max_lines).enumerate() {
        let line_num = format!("{:3} ", idx + 1);
        let formatted_line = highlight_rtfs_line(line);
        let display_line = if formatted_line.len() > width - 8 {
            format!("{}...", &formatted_line[..width - 11])
        } else {
            formatted_line.clone()
        };
        println!("{}{}{}", "║".cyan(), line_num.dark_grey(), display_line);
    }

    if !full && src.len() > preview_len {
        let more_msg = format!(
            "  {} more characters... (use --plan-full)",
            src.len() - preview_len
        );
        println!("{}{}", "║".cyan(), more_msg.dark_yellow());
    }

    println!("{}", bot.cyan());
}

/// Render a WASM plan indicator
fn render_wasm_plan_box(plan_id: &str, byte_count: usize) {
    let title = format!(" 📦 WASM Plan: {} ", plan_id);
    let title_len = title.chars().count();
    let width = 80;

    println!();
    let top = format!("╔{}╗", "═".repeat(width - 2));
    let mid = format!("╠{}╣", "═".repeat(width - 2));
    let bot = format!("╚{}╝", "═".repeat(width - 2));
    println!("{}", top.cyan());
    let padding = " ".repeat(width.saturating_sub(title_len + 2));
    let header = format!("║{}{}║", title.bold().white(), padding);
    println!("{}", header);
    println!("{}", mid.cyan());
    let content = format!("  Binary module: {} bytes", byte_count);
    println!("{} {}", "║".cyan(), content.white());
    println!("{}", bot.cyan());
}

/// Basic syntax highlighting for RTFS code
fn highlight_rtfs_line(line: &str) -> String {
    let trimmed = line.trim_start();
    let indent_len = line.len() - trimmed.len();
    let indent = " ".repeat(indent_len);

    // Keywords
    if trimmed.starts_with("let ") || trimmed.starts_with("const ") {
        return format!("{}{}", indent, trimmed.magenta().to_string());
    }
    if trimmed.starts_with("fn ") || trimmed.starts_with("return ") {
        return format!("{}{}", indent, trimmed.yellow().to_string());
    }
    if trimmed.starts_with("if ") || trimmed.starts_with("else") || trimmed.starts_with("match ") {
        return format!("{}{}", indent, trimmed.yellow().to_string());
    }
    // Comments
    if trimmed.starts_with("//") || trimmed.starts_with("#") {
        return format!("{}{}", indent, trimmed.dark_grey().to_string());
    }
    // Capability calls (heuristic: contains "::" or "call")
    if trimmed.contains("::") || trimmed.contains("call(") {
        return format!("{}{}", indent, trimmed.green().to_string());
    }
    // Strings (simple detection)
    if trimmed.contains('"') {
        return format!("{}{}", indent, trimmed.blue().to_string());
    }

    format!("{}{}", indent, trimmed.white().to_string())
}

/// Render execution result in a beautiful box
fn render_execution_result(
    res: &rtfs_compiler::ccos::types::ExecutionResult,
    show_full: bool,
    preview_len: usize,
) {
    let width = 80;
    let status_icon = if res.success { "✅" } else { "❌" };
    let status_text = if res.success { "SUCCESS" } else { "FAILURE" };
    let status_color = if res.success {
        Color::Green
    } else {
        Color::Red
    };
    let title = format!(" {} Execution Result: {} ", status_icon, status_text);
    let title_len = title.chars().count();

    println!();
    let top = format!("╔{}╗", "═".repeat(width - 2));
    let mid = format!("╠{}╣", "═".repeat(width - 2));
    let bot = format!("╚{}╝", "═".repeat(width - 2));
    println!("{}", top.with(status_color));
    let padding = " ".repeat(width.saturating_sub(title_len + 2));
    let header = format!("║{}{}║", title.bold().with(status_color), padding);
    println!("{}", header);
    println!("{}", mid.with(status_color));

    let value_str = format!("{}", res.value);

    // Try to parse as JSON for pretty printing
    if let Ok(json_val) = serde_json::from_str::<JsonValue>(&value_str) {
        if let Ok(pretty) = serde_json::to_string_pretty(&json_val) {
            let lines: Vec<&str> = pretty.lines().collect();
            let max_lines = if show_full {
                lines.len()
            } else {
                lines.len().min(15)
            };

            for line in lines.iter().take(max_lines) {
                let display_line = if line.len() > width - 4 {
                    format!("{}...", &line[..width - 7])
                } else {
                    line.to_string()
                };
                println!("{} {}", "║".with(status_color), display_line.white());
            }

            if !show_full && lines.len() > 15 {
                let more = format!(
                    "  ... {} more lines (use --show-full-value)",
                    lines.len() - 15
                );
                println!("{} {}", "║".with(status_color), more.dark_yellow());
            }
        } else {
            render_plain_value(&value_str, show_full, preview_len, width, status_color);
        }
    } else {
        render_plain_value(&value_str, show_full, preview_len, width, status_color);
    }

    println!("{}", bot.with(status_color));
}

fn render_plain_value(
    value_str: &str,
    show_full: bool,
    preview_len: usize,
    width: usize,
    border_color: Color,
) {
    let display_val = if show_full {
        value_str
    } else {
        &value_str[..value_str.len().min(preview_len)]
    };

    for line in display_val.lines().take(10) {
        let display_line = if line.len() > width - 4 {
            format!("{}...", &line[..width - 7])
        } else {
            line.to_string()
        };
        println!("{} {}", "║".with(border_color), display_line.white());
    }

    if !show_full && value_str.len() > preview_len {
        let more = format!(
            "  ... {} more chars (use --show-full-value)",
            value_str.len() - preview_len
        );
        println!("{} {}", "║".with(border_color), more.dark_yellow());
    }
}

/// Render an error box
fn render_error_box(error: &str) {
    let width = 80;
    let title = " ⚠️  Execution Error ";

    println!();
    println!(
        "{}",
        "╔".red().to_string() + &"═".repeat(width - 2).red().to_string() + &"╗".red().to_string()
    );
    println!(
        "{}{}{}",
        "║".red(),
        title.bold().red(),
        " ".repeat(width - title.len() - 2) + "║".red().to_string().as_str()
    );
    println!(
        "{}",
        "╠".red().to_string() + &"═".repeat(width - 2).red().to_string() + &"╣".red().to_string()
    );

    for line in error.lines().take(15) {
        let display_line = if line.len() > width - 4 {
            format!("{}...", &line[..width - 7])
        } else {
            line.to_string()
        };
        println!("{} {}", "║".red(), display_line.white());
    }

    println!(
        "{}",
        "╚".red().to_string() + &"═".repeat(width - 2).red().to_string() + &"╝".red().to_string()
    );
}

struct TimelinePrinter {
    base_ts: Option<u64>,
    header_printed: bool,
    seen_action_ids: HashSet<String>,
    value_preview: usize,
}

impl TimelinePrinter {
    fn new(ccos: &Arc<CCOS>, value_preview: usize) -> Self {
        let mut seen_action_ids = HashSet::new();
        if let Ok(chain) = ccos.get_causal_chain().lock() {
            for action in chain.get_all_actions() {
                seen_action_ids.insert(action.action_id.clone());
            }
        }
        Self {
            base_ts: None,
            header_printed: false,
            seen_action_ids,
            value_preview,
        }
    }

    fn handle(&mut self, raw: &str, ccos: &Arc<CCOS>) {
        if let Some((ts, message)) = parse_timeline_event(raw) {
            self.print_line(ts, message);
        } else if !raw.is_empty() {
            self.print_line(None, raw.to_string());
        }
        self.flush_new_actions(ccos);
    }

    fn flush_new_actions(&mut self, ccos: &Arc<CCOS>) {
        if let Ok(chain) = ccos.get_causal_chain().try_lock() {
            for action in chain.get_all_actions() {
                if self.seen_action_ids.contains(&action.action_id) {
                    continue;
                }
                if let Some((ts, message)) = self.describe_action(action) {
                    self.print_line(ts, message);
                }
                self.seen_action_ids.insert(action.action_id.clone());
            }
        }
    }

    fn describe_action(
        &self,
        action: &rtfs_compiler::ccos::types::Action,
    ) -> Option<(Option<u64>, String)> {
        match action.action_type {
            ActionType::CapabilityCall => {
                let fn_name = action.function_name.as_deref().unwrap_or("-");
                let args_preview = action.arguments.as_ref().map(|args| {
                    let joined = args
                        .iter()
                        .map(|a| format!("{}", a))
                        .collect::<Vec<_>>()
                        .join(", ");
                    truncate(&joined, self.value_preview)
                });
                let message = match args_preview {
                    Some(ref payload) if !payload.is_empty() => {
                        format!(
                            "{} {} args={}",
                            "🎯 Capability call".cyan().bold(),
                            fn_name.yellow(),
                            payload.as_str().white()
                        )
                    }
                    _ => format!(
                        "{} {}",
                        "🎯 Capability call".cyan().bold(),
                        fn_name.yellow()
                    ),
                };
                Some((Some(action.timestamp / 1000), message))
            }
            ActionType::CapabilityResult => {
                if let Some(result) = &action.result {
                    let value_str = truncate(&format!("{}", result.value), self.value_preview);
                    let status_icon = if result.success { "✓" } else { "✗" };
                    let status_color = if result.success {
                        Color::Green
                    } else {
                        Color::Red
                    };
                    Some((
                        Some(action.timestamp / 1000),
                        format!(
                            "{} {} value={}",
                            format!("📦 Result {}", status_icon)
                                .with(status_color)
                                .bold(),
                            if result.success {
                                "success".green()
                            } else {
                                "failure".red()
                            },
                            value_str.white()
                        ),
                    ))
                } else {
                    Some((
                        Some(action.timestamp / 1000),
                        format!("{}", "📦 Capability result".blue()),
                    ))
                }
            }
            _ => None,
        }
    }

    fn print_line(&mut self, ts: Option<u64>, message: String) {
        if !self.header_printed {
            let header =
                "═══════════════════════════════════════════════════════════════════════════════";
            println!("\n{}", header.cyan());
            println!("{}", " 🧭  Execution Timeline".bold().white());
            println!("{}", header.cyan());
            self.header_printed = true;
        }
        if ts.is_some() && self.base_ts.is_none() {
            self.base_ts = ts;
        }
        let prefix = match (self.base_ts, ts) {
            (Some(base), Some(actual)) => {
                let delta = actual.saturating_sub(base);
                format!("[+{:3}s]", delta).dark_cyan().to_string()
            }
            (_, Some(actual)) => format!("[{}s]", actual).dark_cyan().to_string(),
            _ => "[  ~  ]".dark_grey().to_string(),
        };
        println!("  {} {}", prefix, message);
    }
}

fn parse_timeline_event(raw: &str) -> Option<(Option<u64>, String)> {
    let value: JsonValue = serde_json::from_str(raw).ok()?;
    let event = value.get("event")?.as_str()?;
    let ts = value.get("ts").and_then(|v| v.as_u64());
    let message = match event {
        "request_received" => {
            let text = value.get("text").and_then(|v| v.as_str()).unwrap_or("");
            let truncated = if text.len() > 60 {
                format!("{}...", &text[..60])
            } else {
                text.to_string()
            };
            format!(
                "{} {}",
                "🟢 Request received:".green().bold(),
                truncated.white()
            )
        }
        "plan_generated" => {
            let plan_id = value.get("plan_id").and_then(|v| v.as_str()).unwrap_or("?");
            if let Some(intent_id) = value.get("intent_id").and_then(|v| v.as_str()) {
                format!(
                    "{} plan={} intent={}",
                    "🧠 Plan generated".magenta().bold(),
                    plan_id.yellow(),
                    intent_id.dark_yellow()
                )
            } else {
                format!(
                    "{} plan={}",
                    "🧠 Plan generated".magenta().bold(),
                    plan_id.yellow()
                )
            }
        }
        "plan_validation_start" => {
            let plan_id = value.get("plan_id").and_then(|v| v.as_str()).unwrap_or("?");
            format!(
                "{} plan={}",
                "🛡️  Validation started".blue().bold(),
                plan_id.yellow()
            )
        }
        "plan_execution_start" => {
            let plan_id = value.get("plan_id").and_then(|v| v.as_str()).unwrap_or("?");
            format!(
                "{} plan={}",
                "⚙️  Execution started".cyan().bold(),
                plan_id.yellow()
            )
        }
        "plan_execution_completed" => {
            let plan_id = value.get("plan_id").and_then(|v| v.as_str()).unwrap_or("?");
            let success = value
                .get("success")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if success {
                format!(
                    "{} plan={}",
                    "✅ Execution completed".green().bold(),
                    plan_id.yellow()
                )
            } else {
                format!(
                    "{} plan={}",
                    "❌ Execution failed".red().bold(),
                    plan_id.yellow()
                )
            }
        }
        other => format!("{} {}", "ℹ️".blue(), other.white()),
    };
    Some((ts, message))
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

fn apply_profile_env(p: &LlmProfile, announce: bool) {
    std::env::set_var("CCOS_DELEGATING_MODEL", &p.model);
    std::env::set_var("CCOS_LLM_PROVIDER_HINT", &p.provider);
    if let Some(url) = &p.base_url {
        std::env::set_var("CCOS_LLM_BASE_URL", url);
    } else if p.provider == "openrouter" {
        // OpenRouter requires its public REST base; many configs omit it expecting inference.
        // Provide a sane default only if caller hasn't set one already.
        if std::env::var("CCOS_LLM_BASE_URL").is_err() {
            std::env::set_var("CCOS_LLM_BASE_URL", "https://openrouter.ai/api/v1");
        }
    }
    // Key precedence: inline > referenced env variable > pre-existing provider env.
    // This matches user expectation: an inline key in the active profile should win for that session.
    let mut key_source = String::from("none");
    if let Some(inline) = &p.api_key {
        dispatch_key(&p.provider, inline);
        key_source = "inline".into();
    } else if let Some(env_key) = &p.api_key_env {
        if let Ok(v) = std::env::var(env_key) {
            dispatch_key(&p.provider, &v);
            key_source = format!("env:{}", env_key);
        }
    } else {
        // Fallback: rely on already-set provider specific env (if any); we don't know the source.
        key_source = "provider-env-preexisting".into();
    }
    std::env::set_var("CCOS_LLM_KEY_SOURCE", &key_source);
    // Provide arbiter-compatible generic provider/model envs when possible (subset of providers supported internally)
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
    if announce {
        println!(
            "[config] default profile '{}' provider={} model={}",
            p.name, p.provider, p.model
        );
    }
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

// (helpers moved to config::profile_selection for reuse & testing)
