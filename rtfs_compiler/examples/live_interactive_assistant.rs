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

use clap::Parser;
use rtfs_compiler::ccos::types::ActionType;
use rtfs_compiler::ccos::CCOS;
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

    if args.prompt.is_none() {
        println!("🧪 Live Interactive CCOS Assistant\n================================\nType natural language goals. Commands: :help, :intents, :chain, :quit\n");
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
    println!("\n➡️  Request: {}", request);
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
            let value_str = format!("{}", res.value);
            if show_full_value {
                println!("   Result: success={} value=\n----- VALUE BEGIN -----\n{}\n------ VALUE END ------", res.success, value_str);
            } else {
                let preview = truncate(&value_str, value_preview);
                println!(
                    "   Result: success={} value={}{}",
                    res.success,
                    preview,
                    if value_str.len() > value_preview {
                        " (… use --show-full-value for complete output)"
                    } else {
                        ""
                    }
                );
            }

            if show_plan {
                match &plan.body {
                    rtfs_compiler::ccos::types::PlanBody::Rtfs(src) => {
                        if plan_full {
                            println!("   Plan[{}] body=\n----- PLAN BEGIN -----\n{}\n------ PLAN END -------", plan.plan_id, src);
                        } else {
                            let trunc = truncate(src, plan_preview_len);
                            println!(
                                "   Plan[{}] body-preview={}{}",
                                plan.plan_id,
                                trunc,
                                if src.len() > plan_preview_len {
                                    " (… use --plan-full)"
                                } else {
                                    ""
                                }
                            );
                        }
                    }
                    rtfs_compiler::ccos::types::PlanBody::Wasm(bytes) => {
                        println!("   Plan[{}] <WASM {} bytes>", plan.plan_id, bytes.len());
                    }
                }
            }
        }
        Err(e) => {
            println!("   Error: {}", e);
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
    println!("---\n");
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
            println!("🆕 New intents:");
        }
        for i in &new_intents {
            println!(
                "  • {} [{}] goal=\"{}\"",
                i.intent_id,
                format!("{:?}", i.status),
                truncate(&i.goal, 80)
            );
        }
        if !status_changes.is_empty() {
            println!("♻️  Status changes:");
        }
        for ch in status_changes {
            println!("  • {} {} → {}", ch.id, ch.old, ch.new);
        }
    }
}

fn render_full_intents(ccos: &Arc<CCOS>) {
    if let Ok(g) = ccos.get_intent_graph().lock() {
        let all = g.storage.get_all_intents_sync();
        println!("📚 All Intents ({}):", all.len());
        for i in all {
            println!(
                "  - {} {:?} goal=\"{}\"",
                i.intent_id,
                i.status,
                truncate(&i.goal, 100)
            );
        }
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
            println!("🪵 Causal Chain (+{} new)", actions.len() - from_index);
        }
        let start = actions.len().saturating_sub(max_actions);
        if show_output {
            for a in &actions[start..] {
                let value_suffix = if let Some(res) = &a.result {
                    let full = format!("{}", res.value);
                    let truncated = truncate(&full, value_preview);
                    let note = if full.len() > value_preview {
                        " (truncated)"
                    } else {
                        ""
                    };
                    format!(" => success={} value={}{}", res.success, truncated, note)
                } else {
                    String::new()
                };
                println!(
                    "  - [{}] {} {:?} fn={} intent={} plan={} parent={}{}",
                    a.timestamp,
                    a.action_id,
                    a.action_type,
                    a.function_name.as_deref().unwrap_or("-"),
                    a.intent_id,
                    a.plan_id,
                    a.parent_action_id.as_deref().unwrap_or("-"),
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
    println!("Commands:\n  :help              show this help\n  :intents           list all intents\n  :chain             show recent causal actions tail\n  :models            list loaded LLM profiles (explicit + model_sets)\n  :model <name>      switch active LLM profile\n  :model-auto k=v..  auto-select model (prompt=, completion=, quality=)\n  :quit | :q         exit\nFlags:\n  --show-plan                 show truncated plan body each request\n  --plan-full                 show full plan body\n  --plan-preview-len N        length for truncated plan body (default 280)\n  --config <path>             load agent config (JSON/TOML) with llm_profiles catalog\n  --model-auto-prompt-budget  auto-select prompt cost budget (startup)\n  --model-auto-completion-budget auto-select completion cost budget (startup)\nNote: natural language lines are processed to generate intents, plans & executions.");
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
                        format!("🎯 Capability call {} args={}", fn_name, payload)
                    }
                    _ => format!("🎯 Capability call {}", fn_name),
                };
                Some((Some(action.timestamp / 1000), message))
            }
            ActionType::CapabilityResult => {
                if let Some(result) = &action.result {
                    let value_str = truncate(&format!("{}", result.value), self.value_preview);
                    Some((
                        Some(action.timestamp / 1000),
                        format!(
                            "📦 Capability result success={} value={}",
                            result.success, value_str
                        ),
                    ))
                } else {
                    Some((
                        Some(action.timestamp / 1000),
                        "📦 Capability result".to_string(),
                    ))
                }
            }
            _ => None,
        }
    }

    fn print_line(&mut self, ts: Option<u64>, message: String) {
        if !self.header_printed {
            println!("🧭 Execution timeline:");
            self.header_printed = true;
        }
        if ts.is_some() && self.base_ts.is_none() {
            self.base_ts = ts;
        }
        let prefix = match (self.base_ts, ts) {
            (Some(base), Some(actual)) => {
                let delta = actual.saturating_sub(base);
                format!("[+{}s]", delta)
            }
            (_, Some(actual)) => format!("[{}]", actual),
            _ => "[~]".to_string(),
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
            format!("🟢 Request received: {}", text)
        }
        "plan_generated" => {
            let plan_id = value.get("plan_id").and_then(|v| v.as_str()).unwrap_or("?");
            if let Some(intent_id) = value.get("intent_id").and_then(|v| v.as_str()) {
                format!("🧠 Plan generated (plan={} intent={})", plan_id, intent_id)
            } else {
                format!("🧠 Plan generated (plan={})", plan_id)
            }
        }
        "plan_validation_start" => {
            let plan_id = value.get("plan_id").and_then(|v| v.as_str()).unwrap_or("?");
            format!("🛡️ Validation started (plan={})", plan_id)
        }
        "plan_execution_start" => {
            let plan_id = value.get("plan_id").and_then(|v| v.as_str()).unwrap_or("?");
            format!("⚙️ Execution started (plan={})", plan_id)
        }
        "plan_execution_completed" => {
            let plan_id = value.get("plan_id").and_then(|v| v.as_str()).unwrap_or("?");
            let success = value
                .get("success")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if success {
                format!("✅ Execution completed successfully (plan={})", plan_id)
            } else {
                format!("❌ Execution completed with errors (plan={})", plan_id)
            }
        }
        other => format!("ℹ️ {}", other),
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
