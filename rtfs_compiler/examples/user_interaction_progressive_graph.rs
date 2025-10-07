//! Progressive Intent Graph Interactive Demo
//!
//! Phase 1 implementation (no automatic semantic classifier yet):
//! - User supplies an initial goal.
//! - Each iteration: user can refine / branch / pivot by free-form text.
//! - System processes request via CCOS; newly created intents are detected by diffing
//!   the set of known intents before/after `process_request`.
//! - A lightweight ASCII graph is rendered showing discovered intents and naive
//!   relationships (currently: all subsequent intents attach to the first root until
//!   classifier phases are implemented).
//!
//! Future phases will:
//! - Infer relationship_kind automatically (refinement_of, alternative_to, etc.).
//! - Create semantic edges (EdgeType extension or metadata mapping).
//! - Support decomposition (multiple child intents per user enumeration input).
//! - Support snapshot export & replay.
//!
//! Run (delegation recommended for richer plan generation):
//!   cargo run --example user_interaction_progressive_graph -- --enable-delegation
//!   cargo run --example user_interaction_progressive_graph -- --enable-delegation --verbose
//!
//! NOTE: This example intentionally limits scope to progressive detection so it can ship early.

// Note on RTFS host delegation:
// This example demonstrates the host-delegation pattern used in RTFS. Any effectful
// operation (user prompting, external data fetch) is performed via the `(call ...)`
// primitive. For example, `(call ccos.user.ask "What are your dates?")` delegates a
// question to the host so that runtime, security, and replayability are enforced.

use clap::Parser;
use crossterm::style::Stylize;
use rtfs_compiler::ccos::CCOS;
use rtfs_compiler::ccos::arbiter::ArbiterEngine;
use rtfs_compiler::config::profile_selection::ProfileMeta;
use rtfs_compiler::ast::CapabilityDefinition as CapabilityDef;
use rtfs_compiler::config::types::{AgentConfig, LlmProfile};
use rtfs_compiler::config::validation::validate_config;
use rtfs_compiler::config::{auto_select_model, expand_profiles};
use rtfs_compiler::ccos::types::{ExecutionResult, StorableIntent};
use rtfs_compiler::runtime::values::Value;
use rtfs_compiler::runtime::security::{RuntimeContext, SecurityLevel};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::sync::Arc;
use tokio::time::{sleep, Duration};


/// Represents one turn of the conversation for later analysis.
#[derive(Clone, Debug)]
struct InteractionTurn {
    user_input: String,
    // We store the full Intent object for detailed analysis
    created_intent: Option<StorableIntent>,
}



#[derive(Parser, Debug)]
struct Args {
    /// Enable delegation (LLM plan generation)
    #[arg(long, default_value_t = false)]
    enable_delegation: bool,

    /// Verbose CCOS progression output
    #[arg(long, default_value_t = false)]
    verbose: bool,

    /// Show raw prompt debug (sets RTFS_SHOW_PROMPTS)
    #[arg(long, default_value_t = false)]
    debug_prompts: bool,

    /// Enable interactive prompting for :ccos.user.ask (instead of echo simulation)
    #[arg(long, default_value_t = false)]
    interactive_ask: bool,

    /// Load agent config (JSON or TOML) with optional llm_profiles
    #[arg(long)]
    config: Option<String>,

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

    /// Emit preference schema + metrics after each turn (schema extraction slice)
    #[arg(long, default_value_t = false)]
    emit_pref_schema: bool,

    /// Auto-pick best model within prompt cost budget (USD per 1K tokens)
    #[arg(long)]
    model_auto_prompt_budget: Option<f64>,

    /// Auto-pick best model within completion cost budget (USD per 1K tokens)
    #[arg(long)]
    model_auto_completion_budget: Option<f64>,

    /// After interaction, attempt LLM-driven capability synthesis & auto-register
    #[arg(long, default_value_t = false)]
    synthesize_capability: bool,

    /// Persist synthesized capability spec to disk (implies --synthesize-capability)
    #[arg(long, default_value_t = false)]
    persist_synthesized: bool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    if args.debug_prompts {
        std::env::set_var("RTFS_SHOW_PROMPTS", "1");
    }
    if args.interactive_ask {
        std::env::set_var("CCOS_INTERACTIVE_ASK", "1");
    }

    // Load config file (if provided) and extract LLM profiles
    let mut loaded_config: Option<AgentConfig> = None;
    if let Some(cfg_path) = &args.config {
        match load_agent_config(cfg_path) {
            Ok(cfg) => {
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
                println!("  {}: {}", "INFO", m.message);
            }
        }
        // Note: we previously propagated these delegation debug flags into
        // environment variables to enable arbiter prints. The example now
        // passes the loaded `AgentConfig` directly into `CCOS::new_with_agent_config*`,
        // so this env propagation is redundant. Keep the checks commented for
        // backward compatibility if external code relies on the env flags.
        // if cfg.delegation.print_extracted_intent.unwrap_or(false) {
        //     std::env::set_var("CCOS_PRINT_EXTRACTED_INTENT", "1");
        // }
        // if cfg.delegation.print_extracted_plan.unwrap_or(false) {
        //     std::env::set_var("CCOS_PRINT_EXTRACTED_PLAN", "1");
        // }
    }

    // If no explicit CLI model/provider, attempt auto-pick by budgets; else fall back to configured default profile; else do nothing
    if args.llm_model.is_none() && args.llm_provider.is_none() {
        if args.model_auto_prompt_budget.is_some() || args.model_auto_completion_budget.is_some() {
            let (best, rationale) = auto_select_model(
                &expanded_profiles,
                &profile_meta,
                args.model_auto_prompt_budget,
                args.model_auto_completion_budget,
                None,
            );
            if let Some(best) = best {
                println!("[config] auto-selected profile '{}' for budget constraints", best.name);
                apply_profile_env(best, true);
                std::env::set_var("CCOS_ENABLE_DELEGATION", "1");
            } else {
                println!("[config] no profile met budget constraints: {}", rationale);
            }
        } else {
            // If a config file was loaded, apply its default profile (if present) so examples behave like the
            // live interactive assistant. This ensures `agent_config.toml` defaults are respected when no
            // CLI overrides or auto-selection are used.
            if let Some(cfg) = &loaded_config {
                if let Some(llm_cfg) = &cfg.llm_profiles {
                    if let Some(default_name) = &llm_cfg.default {
                        if let Some(p) = expanded_profiles.iter().find(|p| &p.name == default_name) {
                            apply_profile_env(p, true);
                            std::env::set_var("CCOS_ENABLE_DELEGATION", "1");
                        } else if !expanded_profiles.is_empty() {
                            // No explicit default name: as a last resort apply the first expanded profile so the
                            // example has a reasonable default behavior (matches the interactive assistant UX).
                            apply_profile_env(&expanded_profiles[0], true);
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
            "openai" => { std::env::set_var("CCOS_LLM_PROVIDER", "openai"); },
            "claude" | "anthropic" => { std::env::set_var("CCOS_LLM_PROVIDER", "anthropic"); },
            "gemini" => { std::env::set_var("GEMINI_API_KEY", "gemini"); },
            "stub" => { std::env::set_var("CCOS_LLM_PROVIDER", "stub"); },
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

    // Example-level: activate provider retry strategy to ask the LLM to correct malformed outputs.
    // These can be overridden by the user's environment or AgentConfig.
    if args.enable_delegation {
        // 2 attempts by default with feedback-enabled for corrective re-prompts
        std::env::set_var("CCOS_LLM_RETRY_MAX_RETRIES", "2");
        std::env::set_var("CCOS_LLM_RETRY_SEND_FEEDBACK", "1");
        // Keep simplify on final attempt enabled to increase chance of valid output
        std::env::set_var("CCOS_LLM_RETRY_SIMPLIFY_FINAL", "1");
    }

    // Offline deterministic path: if stub provider selected (explicitly or via hint) ensure sensible defaults
    let provider_is_stub = args
        .llm_provider
        .as_deref()
        .map(|p| p.eq_ignore_ascii_case("stub"))
        .unwrap_or(false)
        || std::env::var("CCOS_LLM_PROVIDER_HINT").map(|v| v == "stub").unwrap_or(false);
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

    // Fallback: if still no delegation flag or env set yet (non-stub scenarios), consider enabling for richer interaction
    if std::env::var("CCOS_ENABLE_DELEGATION").ok().as_deref() != Some("1") && !provider_is_stub {
        // Leave disabled for now to respect explicit user choice; could auto-enable based on heuristic later.
    }

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
        if args.debug_prompts {
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

    println!("{}", "Progressive Intent Graph Session".bold());
    println!("{}\n", "================================".bold());
    println!("Type a goal to begin. Empty line to quit.");

    // Security context: allow basic user input & echo capability for demo.
    let ctx = RuntimeContext {
        security_level: SecurityLevel::Controlled,
        allowed_capabilities: vec![
            "ccos.echo".to_string(),
            "ccos.user.ask".to_string(),
        ]
        .into_iter()
        .collect(),
        ..RuntimeContext::pure()
    };

    // Initialize CCOS, passing the loaded AgentConfig (if any) so runtime honors
    // agent-level delegation flags (cleaner than relying on env propagation).
    let ccos = Arc::new(
        CCOS::new_with_agent_config_and_configs_and_debug_callback(
            rtfs_compiler::ccos::intent_graph::config::IntentGraphConfig::default(),
            None,
            loaded_config.take(),
            None,
        )
        .await?
    );

    // Track known intents (intent_id -> goal snippet)
    let mut known_intents: HashMap<String, String> = HashMap::new();
    let mut root_intent: Option<String> = None;

    // Track responses from interactive plans for use in subsequent turns
    let mut collected_responses: HashMap<String, String> = HashMap::new();
    // Track asked question prompts to detect stagnation / loops
    let mut asked_questions: HashSet<String> = HashSet::new();
    let mut stagnant_turns = 0usize;
    const STAGNATION_LIMIT: usize = 2;
    
    // Track context from previous plan executions for passing to subsequent plans
    let mut accumulated_context: HashMap<String, String> = HashMap::new();
    
    // --- Phase 2: Simulation of Interaction ---
    println!("\n{}", "--- Running Simulated Interaction ---".yellow().bold());

    // Start with a single seed input and allow the LLM-driven plan to ask follow-up
    // questions via `ccos.user.ask`. The runtime's `ccos.user.ask` currently echoes
    // the prompt string as the simulated user response, so we capture the execution
    // result `res.value` and use its string form as the next user input. This lets
    // the LLM drive the multi-turn flow rather than hardcoding follow-ups here.
    let mut conversation_history: Vec<InteractionTurn> = Vec::new();
    let mut current_request = "I need to plan a trip to Paris.".to_string();

    // Bound the simulated interaction to avoid runaway loops
    let max_turns = 8usize;
    for turn in 0..max_turns {
        println!(
            "\n{}",
            format!("--- Turn {}/{} ---", turn + 1, max_turns)
                .yellow()
                .bold()
        );
        println!("{}: {}", "User Input".bold(), current_request.trim());

        let before_intents = snapshot_intent_ids(&ccos);

        // Process the request and handle potential errors. process_request expects a reference
        // to RuntimeContext. Use process_request_with_plan when we need the synthesized plan id.
        let res = match ccos.process_request(&current_request, &ctx).await {
            Ok(r) => r,
            Err(e) => {
                println!("{}", format!("[error] CCOS failed to process request: {}", e).red());
                break;
            }
        };

        // --- Intent & Graph Update ---
        let after_intents = snapshot_intent_ids(&ccos);
        let new_intent_ids: Vec<_> = after_intents.difference(&before_intents).cloned().collect();

        let mut created_intent_this_turn: Option<StorableIntent> = None;
        if let Some(new_id) = new_intent_ids.get(0) {
            if let Some(goal) = fetch_intent_goal(&ccos, new_id) {
                println!("[intent] New intent created: {} ({})", short(new_id), goal);
                known_intents.insert(new_id.clone(), goal.clone());
                if root_intent.is_none() {
                    root_intent = Some(new_id.clone());
                }

                // Store the full intent for later analysis (IntentGraph API exposes `get_intent`)
                if let Ok(mut ig) = ccos.get_intent_graph().lock() {
                    if let Some(intent_obj) = ig.get_intent(new_id) {
                        created_intent_this_turn = Some(intent_obj.clone());
                    }
                }
            }
        }
        
        conversation_history.push(InteractionTurn {
            user_input: current_request.clone(),
            created_intent: created_intent_this_turn,
        });


        // --- Plan Execution & Interaction Logic ---
        let mut next_request = None;
        let mut plan_exhausted = false;

        match &res.value {
            Value::String(s) => {
                // This branch handles simple :ccos.echo or direct string returns
                if is_user_response(s) {
                    println!("{}: {}", "System Response".bold(), s.clone().cyan());
                    next_request = Some(s.clone());
                } else {
                    println!("{}: {}", "Execution Result".bold(), s.clone().dim());
                    // Fallback: use the execution value as the next user input so the
                    // LLM-driven flow can continue when the capability echoes or
                    // returns a meaningful string that isn't classified as an explicit
                    // user response by heuristics.
                    next_request = Some(s.clone());
                }
            }
            Value::Map(map) => {
                // This branch handles structured data, typical of final plan steps
                println!("{}:\n{}", "Execution Result (Map)".bold(), res.value.to_string().dim());

                // Detect explicit refinement_exhausted signal per strategy prompt
                // Detect explicit refinement_exhausted signal per strategy prompt
                if is_refinement_exhausted(&res.value) {
                    println!("{}", "[flow] Refinement exhausted signal detected. Ending interaction.".green());
                    plan_exhausted = true;
                }

                // Also treat certain final statuses (no further questions expected) as terminal.
                if let Some(status_str) = get_map_string_value(map, "status") {
                    match status_str.as_str() {
                        "itinerary_ready" | "ready_for_planning" | "completed" => {
                            println!("{}", format!("[flow] Plan returned terminal status '{}' - ending interaction.", status_str).green());
                            plan_exhausted = true;
                        }
                        _ => {}
                    }
                }

                // Also check for response data within the map (string responses stored under string keys)
                if let Some(response_str) = get_map_string_value(map, "response") {
                    if is_user_response(response_str) {
                        println!("{}: {}", "System Response".bold(), response_str.clone().cyan());
                        next_request = Some(response_str.to_string());
                    }
                }
                // If the plan provided an explicit next-agent directive, synthesize a concise
                // natural-language prompt from the structured map so the next turn is
                // meaningful (avoid passing raw serialized maps as user input).
                if next_request.is_none() && !plan_exhausted {
                    if let Some(next_agent) = get_map_string_value(map, "next/agent")
                        .or_else(|| get_map_string_value(map, "next_agent"))
                    {
                        // Build a short context summary from the map
                        let mut parts: Vec<String> = Vec::new();
                        for (k, v) in map.iter() {
                            let key_str = k.to_string().trim_start_matches(':').to_string();
                            let val_str = match v {
                                Value::String(s) => s.clone(),
                                other => other.to_string(),
                            };
                            parts.push(format!("{}={}", key_str, val_str));
                        }
                        let summary = parts.join(", ");
                        let prompt = format!(
                            "Agent {}: continue with planning using the following context: {}",
                            next_agent, summary
                        );
                        next_request = Some(prompt);
                    } else {
                        // Fallback: use the serialized map as the next request so the interaction
                        // can continue when no explicit directive was present.
                        next_request = Some(res.value.to_string());
                    }
                }
            }
            _ => {
                println!("{}: {}", "Execution Result".bold(), res.value.to_string().dim());
            }
        }

        // If the plan signaled completion, break the loop
        if plan_exhausted {
            break;
        }

        // --- Multi-turn Response Handling ---
        // Extract context from the current execution to be used in the next turn
        let new_context = extract_context_from_result(&res);
        if !new_context.is_empty() {
            accumulated_context.extend(new_context);
        }

        // If the plan paused for user input, a PlanPaused action will be emitted into the
        // causal chain. We need to find the question and generate a response. We don't have
        // a direct `plan_id` field on ExecutionResult; use metadata or skip this step when
        // plan id is not present in metadata.
        if let Some(Value::String(plan_id_val)) = res.metadata.get("plan_id") {
            let plan_id = plan_id_val.as_str();
            let responses = extract_pending_questions_and_generate_responses(
                &ccos,
                plan_id,
                &collected_responses,
            );

            if !responses.is_empty() {
                println!("[flow] Plan paused. Generated {} responses.", responses.len());
                collected_responses.extend(responses);

                // If we generated responses, we need to re-run the plan with the new context.
                // The `find_latest_plan_checkpoint` will give us the point to resume from.
                if let Some(checkpoint_id) = find_latest_plan_checkpoint(&ccos, plan_id) {
                    next_request = Some(format!(
                        "(plan.resume {} :checkpoint {})",
                        plan_id, checkpoint_id
                    ));
                }
            }
        }

        // --- Loop continuation or termination ---
        if let Some(req) = next_request {
            // Detect if the system is asking the same question repeatedly
            if asked_questions.contains(&req) {
                stagnant_turns += 1;
                println!(
                    "[flow] System repeated a question. Stagnation count: {}",
                    stagnant_turns
                );
                if stagnant_turns >= STAGNATION_LIMIT {
                    println!(
                        "{}",
                        "[flow] Stagnation limit reached. Terminating interaction."
                            .red()
                            .bold()
                    );
                    break;
                }
            } else {
                stagnant_turns = 0;
                asked_questions.insert(req.clone());
            }
            current_request = req;
        } else {
            println!(
                "{}",
                "[flow] No further actions or questions from the system. Terminating interaction."
                    .green()
            );
            break;
        }

        // Small delay to make the interaction feel more natural

        // Optional: Emit preference schema + metrics (either via CLI flag or env var CCOS_EMIT_PREF_SCHEMA=1)
        let emit_schema = args.emit_pref_schema || std::env::var("CCOS_EMIT_PREF_SCHEMA").map(|v| v == "1" || v.eq_ignore_ascii_case("true")).unwrap_or(false);
        if emit_schema {
            if let Ok(chain_lock) = ccos.get_causal_chain().lock() {
                use rtfs_compiler::ccos::synthesis::preference_schema::extract_with_metrics;
                let (schema, metrics) = extract_with_metrics(&chain_lock);
                println!("\n[pref.schema] params={} coverage={:.2} redundancy={:.2} enum_specificity={:.2}",
                    schema.params.len(), metrics.coverage, metrics.redundancy, metrics.enum_specificity);
                for (name, meta) in schema.params.iter() {
                    let kind = match meta.param_type { rtfs_compiler::ccos::synthesis::preference_schema::ParamType::Enum => "enum", rtfs_compiler::ccos::synthesis::preference_schema::ParamType::String => "string", rtfs_compiler::ccos::synthesis::preference_schema::ParamType::Integer => "int", rtfs_compiler::ccos::synthesis::preference_schema::ParamType::Float => "float", rtfs_compiler::ccos::synthesis::preference_schema::ParamType::Boolean => "bool", rtfs_compiler::ccos::synthesis::preference_schema::ParamType::Unknown => "?" };
                    let enum_desc = if !meta.enum_values.is_empty() { format!(" {:?}", meta.enum_values) } else { String::new() };
                    println!("[pref.param] {} type={} required={} turns={}..{} asked={}{}", name, kind, meta.required, meta.first_turn, meta.last_turn, meta.questions_asked, enum_desc);
                }
            }
        }

        sleep(Duration::from_millis(250)).await;
    }

    render_ascii_graph(root_intent.as_ref(), &known_intents);

    // --- Phase 3: Post-Mortem Analysis and Synthesis ---
    if args.synthesize_capability {
        if let Err(e) = generate_synthesis_summary(&conversation_history, root_intent.as_ref(), &ccos, args.persist_synthesized).await {
            eprintln!("[synthesis] Error: {}", e);
        }
    } else {
        // Legacy placeholder output for comparison (no registration)
        if let Err(e) = legacy_placeholder_synthesis(&conversation_history, root_intent.as_ref()) {
            eprintln!("[synthesis.placeholder] Error: {}", e);
        }
    }


    Ok(())
}

/// Performs a post-mortem analysis of the conversation to synthesize a new capability.
/// Legacy placeholder synthesis output (retained for fallback when --synthesize-capability not set)
fn legacy_placeholder_synthesis(history: &[InteractionTurn], _root_intent_id: Option<&String>) -> Result<(), Box<dyn std::error::Error>> {
    println!("\n\n{}", "--- Capability Synthesis Analysis (Placeholder) ---".bold());
    if history.is_empty() { println!("Conversation history is empty. Nothing to analyze."); return Ok(()); }
    let root_goal = history.get(0).and_then(|t| t.created_intent.as_ref()).map_or("Unknown".to_string(), |i| i.goal.clone());
    println!("{} {}", "Initial Goal:".bold(), root_goal);
    println!("{} {} turns", "Total Interaction Turns:".bold(), history.len());
    println!("(Run again with --synthesize-capability for LLM-driven generation)");
    Ok(())
}

/// Performs a post-mortem analysis of the conversation, calls the delegating arbiter LLM to propose a reusable capability,
/// validates & registers it into the capability marketplace, and optionally persists it.
async fn generate_synthesis_summary(
    history: &[InteractionTurn],
    _root_intent_id: Option<&String>,
    ccos: &Arc<CCOS>,
    persist: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("\n\n{}", "--- Capability Synthesis Analysis (LLM) ---".bold());

    if history.is_empty() {
        println!("Conversation history is empty. Nothing to analyze.");
        return Ok(());
    }

    let root_goal = history.get(0)
        .and_then(|turn| turn.created_intent.as_ref())
        .map_or("Unknown".to_string(), |intent| intent.goal.clone());
    let refinements: Vec<String> = history.iter().skip(1)
        .filter_map(|turn| turn.created_intent.as_ref().map(|i| i.goal.clone()))
        .collect();

    println!("{} {}", "Initial Goal:".bold(), root_goal);
    println!("{} {} turns", "Total Interaction Turns:".bold(), history.len());
    if !refinements.is_empty() {
        println!("{}", "Refinements:".bold());
        for (i, r) in refinements.iter().enumerate() { println!("  {}. {}", i+1, truncate(r, 90)); }
    }

    // --- Quick, local synthesis using the built-in pipeline (Phase 8 minimal)
    println!("{}", "[synthesis] Running quick local synthesis pipeline (schema extraction + artifact generation)...".yellow());
    let interaction_turns_for_synthesis: Vec<rtfs_compiler::ccos::synthesis::InteractionTurn> = history.iter().map(|t| {
        rtfs_compiler::ccos::synthesis::InteractionTurn { turn_index: 0, prompt: t.user_input.clone(), answer: None }
    }).collect();

    let synth_result = rtfs_compiler::ccos::synthesis::synthesize_capabilities(&interaction_turns_for_synthesis, ccos);
    if let Some(col) = &synth_result.collector {
        println!("[synthesis.quick] Collector:\n{}", col);
        if persist {
            let _ = persist_capability_spec("synth.collector", col);
            println!("[synthesis.quick] Persisted synth.collector.rtfs");
        }
    }
    if let Some(plan) = &synth_result.planner {
        println!("[synthesis.quick] Planner:\n{}", plan);
        if persist {
            let _ = persist_capability_spec("synth.planner", plan);
            println!("[synthesis.quick] Persisted synth.planner.rtfs");
        }
    }
    if let Some(stub) = &synth_result.stub {
        println!("[synthesis.quick] Stub:\n{}", stub);
        if persist {
            let _ = persist_capability_spec("synth.stub", stub);
            println!("[synthesis.quick] Persisted synth.stub.rtfs");
        }
    }

    // 1. Build synthesis prompt
    let prompt = build_capability_synthesis_prompt(&root_goal, &refinements);
    println!("{}", "[synthesis] Requesting capability proposal from LLM...".yellow());

    // 2. Obtain raw capability proposal WITHOUT forcing intent parsing first.
    let arbiter = if let Some(a) = ccos.get_delegating_arbiter() { a } else {
        println!("[synthesis] Delegating arbiter not available (enable delegation). Skipping.");
        return Ok(());
    };

    let raw = match arbiter.generate_raw_text(&prompt).await {
        Ok(txt) => txt,
        Err(e) => { eprintln!("[synthesis] Raw capability generation failed: {}", e); return Ok(()); }
    };
    println!("[synthesis] Raw LLM proposal (truncated 300 chars): {}", truncate(&raw, 300));

    // 3. Parser-first attempt: try to parse the raw response into TopLevel ASTs and
    // if we find a TopLevel::Capability, pretty-print it into canonical RTFS source.
    let mut spec = if let Ok(parsed) = rtfs_compiler::parser::parse_with_enhanced_errors(&raw, None) {
        // If parser returns at least one capability top-level node, convert it to canonical RTFS
        let mut found_cap: Option<String> = None;
        for tl in parsed.iter() {
            if let rtfs_compiler::ast::TopLevel::Capability(_) = tl {
                if let Some(s) = rtfs_compiler::ccos::rtfs_bridge::extractors::toplevel_to_rtfs_string(tl) {
                    // Wrap in fenced block for downstream processing
                    found_cap = Some(format!("```rtfs\n{}\n```", s));
                    break;
                }
            }
        }
        if let Some(c) = found_cap { c } else {
            // Fall back to older heuristics if parser didn't yield a capability
            extract_capability_block(&raw).unwrap_or_else(|| extract_capability_spec(&raw).unwrap_or_else(|| raw.clone()))
        }
    } else {
        // If parsing failed (likely because output isn't pure RTFS), fall back to heuristics
        extract_capability_block(&raw).unwrap_or_else(|| extract_capability_spec(&raw).unwrap_or_else(|| raw.clone()))
    };

    // Detect likely truncation: if raw contains a starting ```rtfs fence but no closing fence,
    // or if a (capability appears but parentheses are unbalanced, attempt a targeted completion.
    if raw.contains("```rtfs") && !raw.matches("```rtfs").count().eq(&2) {
        println!("[synthesis] Detected possibly truncated fenced rtfs block; requesting completion...");
        let complete_prompt = format!(
            "The previous response started an RTFS fenced block but was truncated. Here is the full raw response so far:\n\n{}\n\nPlease OUTPUT ONLY the missing remainder of the RTFS fenced block (no fences) so we can append it to the earlier content and produce a valid (capability ...) s-expression. Do NOT add any commentary.",
            raw
        );
        if let Ok((_it, completion_raw)) = arbiter.natural_language_to_intent_with_raw(&complete_prompt, None).await {
            // Append the completion to the original raw and re-extract
            let stitched = format!("{}{}", raw, completion_raw);
            if let Some(block) = extract_capability_block(&stitched) {
                spec = block;
            } else {
                // last resort: set spec to stitched raw so later parsing will attempt
                spec = stitched;
            }
        } else {
            println!("[synthesis] Completion re-prompt failed.");
        }
    } else if raw.contains("(capability") {
        // If there's a capability but parentheses appear unbalanced, try to detect and request completion
        if let Some(idx) = raw.find("(capability") {
            if extract_balanced_sexpr(&raw, idx).is_none() {
                println!("[synthesis] Detected unbalanced (capability s-expression; requesting completion...");
                let complete_prompt = format!(
                    "The previous response began a (capability ...) s-expression but it appears to be incomplete. Here is the raw response so far:\n\n{}\n\nPlease OUTPUT ONLY the missing remainder of the s-expression (no fences), so we can append it and obtain a valid RTFS (capability ...) top-level form. Do NOT include commentary.",
                    raw
                );
                if let Ok((_it, completion_raw)) = arbiter.natural_language_to_intent_with_raw(&complete_prompt, None).await {
                    let stitched = format!("{}{}", raw, completion_raw);
                    if let Some(block) = extract_capability_block(&stitched) {
                        spec = block;
                    } else {
                        spec = stitched;
                    }
                } else {
                    println!("[synthesis] Completion re-prompt failed.");
                }
            }
        }
    }

    // If we didn't get a proper (capability ...) s-expression, re-prompt the LLM asking for only that form
    if !spec.contains("(capability") {
        println!("[synthesis] Initial proposal did not include a (capability ...) block - re-prompting for strict RTFS capability output...");
        let clarify = format!(
            "The previous proposal:\n{}\n\nPlease OUTPUT ONLY a single well-formed RTFS s-expression that defines a capability. The top-level form must start with (capability \"id\" ...) and include :description and optionally :parameters and :steps or :implementation. Wrap the s-expression in a ```rtfs fenced code block. Do NOT include any extra commentary.",
            raw
        );
        if let Ok((_it, clarified_raw)) = arbiter.natural_language_to_intent_with_raw(&clarify, None).await {
            if let Some(block) = extract_capability_block(&clarified_raw) {
                spec = block;
            } else {
                // Last resort: if clarifying response still didn't contain a capability s-expr,
                // attempt to ask the arbiter to generate an RTFS plan for the parsed intent
                println!("[synthesis] Clarified response still lacked a (capability ...) block; attempting to generate an RTFS plan from the parsed intent...");
                // We have no parsed intent yet; attempt to derive an intent ONLY if needed for fallback plan.
                let parsed_intent_opt = match arbiter.natural_language_to_intent_with_raw(&prompt, None).await {
                    Ok((it, _r)) => Some(it),
                    Err(_e) => None,
                };
                let plan_result: Result<_, String> = if let Some(pi) = parsed_intent_opt.as_ref() {
                    match arbiter.intent_to_plan(pi).await {
                        Ok(p) => Ok(p),
                        Err(e) => Err(format!("{}", e)),
                    }
                } else {
                    Err("No parsed intent for fallback plan generation".to_string())
                };
                match plan_result {
                    Ok(plan) => {
                        // Use plan body (Rtfs) as the steps for a synthesized capability
                        if let rtfs_compiler::ccos::types::PlanBody::Rtfs(plan_body) = plan.body {
                            // derive a temporary capability id from root_goal (kebab-case) if none yet
                            let temp_id = extract_capability_id(&spec).unwrap_or_else(|| {
                                root_goal.to_lowercase()
                                    .chars()
                                    .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
                                    .collect::<String>()
                            });
                            // Wrap plan_body into a capability :steps form
                            let wrapped = format!("```rtfs\n(capability \"{}\"\n  :description \"Synthesized capability derived from interaction about '{}'\"\n  :steps {}\n)\n```", temp_id, root_goal.replace('"', "'"), plan_body);
                            spec = wrapped;
                            println!("[synthesis] Built capability spec from generated plan (wrapped).");
                        } else {
                            println!("[synthesis] Generated plan was not RTFS; cannot wrap into capability.");
                        }
                    }
                    Err(e) => {
                        println!("[synthesis] Failed to generate fallback plan (no intent parse or plan error): {}. Using best-effort spec.", e);
                    }
                }
            }
        } else {
            println!("[synthesis] Clarifying re-prompt failed; using best-effort spec.");
        }
    }
    let capability_id = extract_capability_id(&spec).unwrap_or_else(|| {
        // Generate id from goal slug
        root_goal.to_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect::<String>()
    });
    // Use as_str() to avoid moving the String when applying style (String::cyan consumes self)
    println!("[synthesis] Candidate capability id: {}", capability_id.as_str().cyan());

    let marketplace = ccos.get_capability_marketplace();
    if marketplace.has_capability(&capability_id).await {
        println!("[synthesis] Capability '{}' already exists; skipping registration.", capability_id);
        return Ok(());
    }

    // Validate the RTFS capability spec before registering
    match parse_and_validate_capability(&spec) {
        Ok(()) => {
            // 4. Register capability (local) with handler placeholder for now
            let register_result = marketplace.register_local_capability(
                capability_id.clone(),
                capability_id.clone(),
                format!("Synthesized capability derived from interaction about '{}'.", root_goal),
                Arc::new(|value: &Value| {
                    // Handler behavior:
                    // - If CCOS_INTERACTIVE_ASK is set, prompt the user on stdin using the example's prompt()
                    // - Else, attempt to read a canned response from CCOS_USER_ASK_RESPONSE_{KEY}
                    // - Fallback: echo the input in a small result map (original behavior)

                    // Determine a short prompt text from the incoming value
                    let prompt_text = match value {
                        Value::String(s) => s.clone(),
                        Value::Map(m) => {
                            if let Some(Value::String(p)) = m.get(&rtfs_compiler::ast::MapKey::Keyword(rtfs_compiler::ast::Keyword::new("prompt"))) {
                                p.clone()
                            } else if let Some(Value::String(p)) = m.get(&rtfs_compiler::ast::MapKey::String("prompt".to_string())) {
                                p.clone()
                            } else {
                                "Please provide input:".to_string()
                            }
                        }
                        _ => "Please provide input:".to_string(),
                    };

                    // Interactive stdin path
                    if std::env::var("CCOS_INTERACTIVE_ASK").is_ok() {
                        match prompt_user(&format!("(user.ask) {} ", prompt_text)) {
                            Ok(ans) => Ok(Value::String(ans)),
                            Err(e) => Err(rtfs_compiler::runtime::error::RuntimeError::Generic(format!("prompt failed: {}", e)))
                        }
                    } else {
                        // Try canned env response by generating a question key
                        let qkey = generate_question_key(&prompt_text).unwrap_or_else(|| "last_response".to_string());
                        let env_key = format!("CCOS_USER_ASK_RESPONSE_{}", qkey.to_uppercase());
                        if let Ok(env_resp) = std::env::var(&env_key) {
                            Ok(Value::String(env_resp))
                        } else {
                            // Fallback echo map (preserve previous example behavior)
                            Ok(Value::Map({
                                let mut m = std::collections::HashMap::new();
                                m.insert(rtfs_compiler::ast::MapKey::String("status".to_string()), Value::String("executed".to_string()));
                                m.insert(rtfs_compiler::ast::MapKey::String("input".to_string()), value.clone());
                                m
                            }))
                        }
                    }
                })
            ).await;
            match register_result {
                Ok(_) => println!("{} {}", "[synthesis] Registered capability:".green(), capability_id),
                Err(e) => { eprintln!("[synthesis] Registration failed: {}", e); return Ok(()); }
            }
        }
        Err(err_msg) => {
            eprintln!("[synthesis] Capability spec validation failed: {}. Skipping registration.", err_msg);
        }
    }

    // 5. Persist if requested
    if persist {
        if let Err(e) = persist_capability_spec(&capability_id, &spec) {
            eprintln!("[synthesis] Persist error: {}", e);
        } else {
            println!("[synthesis] Persisted spec to generated_capabilities/{}.rtfs", capability_id);
        }
    }

    println!("{}", "[synthesis] Completed.".green());
    Ok(())
}

/// Build a focused synthesis prompt for the LLM
fn build_capability_synthesis_prompt(root_goal: &str, refinements: &[String]) -> String {
    let mut prompt = String::from("You are a capability synthesis engine. Given an initial user goal and its refinements, produce a reusable RTFS capability definition that can be registered in a capability marketplace.\n");
    prompt.push_str("IMPORTANT INSTRUCTIONS:\n");
    prompt.push_str("1) OUTPUT EXACTLY ONE triple-backtick fenced block labeled 'rtfs' that contains exactly one well-formed RTFS s-expression.\n");
    prompt.push_str("2) The top-level form MUST be (capability \"id\" ...). Use kebab-case for ids.\n");
    prompt.push_str("3) Do NOT include any prose, commentary, headings, lists, or extra text outside the single fenced block.\n");
    prompt.push_str("4) Provide :description and optionally :parameters and :steps or :implementation. Keep types simple (string, number, boolean, map, list).\n\n");

    prompt.push_str("Examples (mimic these exactly - each example is a complete, valid response):\n\n");
    prompt.push_str("```rtfs\n(capability \"travel.create-personalized-itinerary\"\n  :description \"Create a personalized travel itinerary given user preferences\"\n  :parameters {:name \"string\" :destination \"string\" :dates \"string\" :budget \"string\"}\n  :steps (do\n    (search.flights :destination \"$destination\")\n    (search.hotels :destination \"$destination\")\n    (optimize.itinerary :preferences $preferences)\n  )\n)\n```\n\n");
    prompt.push_str("```rtfs\n(capability \"weather.get-forecast\"\n  :description \"Return a weather forecast summary for a given location and date range\"\n  :parameters {:location \"string\" :start_date \"string\" :end_date \"string\"}\n  :steps (do\n    (weather.lookup :location $location :range { :from $start_date :to $end_date })\n    (format.forecast-summary :forecast $forecast)\n  )\n)\n```\n\n");
    prompt.push_str("```rtfs\n(capability \"calendar.schedule-meeting\"\n  :description \"Schedule a meeting on the user's calendar given participants, time window, and preferences\"\n  :parameters {:participants :list :title \"string\" :time_window \"string\"}\n  :implementation (do\n    (calendar.find-available-slot :participants $participants :window $time_window)\n    (calendar.create-event :slot $chosen_slot :title $title :participants $participants)\n  )\n)\n```\n\n");

    prompt.push_str(&format!("Initial Goal: {}\n", root_goal));
    if !refinements.is_empty() {
        prompt.push_str("Refinements:\n");
        for r in refinements { prompt.push_str(&format!("- {}\n", r)); }
    }
    prompt.push_str("\nProduce a single capability id in kebab-case derived from the goal and refinements.\n");
    prompt.push_str("Respond ONLY with the fenced rtfs block and nothing else.\n");
    prompt
}

/// Naive extraction: find line starting with 'capability'
fn extract_capability_spec(raw: &str) -> Option<String> {
    if raw.contains("capability") { Some(raw.to_string()) } else { None }
}

fn extract_capability_id(spec: &str) -> Option<String> {
    // Try to find quoted id in (capability "id" ...) form
    if let Some(idx) = spec.find("(capability") {
        if let Some(q1_rel) = spec[idx..].find('"') {
            let start = idx + q1_rel + 1;
            if let Some(q2_rel) = spec[start..].find('"') {
                let end = start + q2_rel;
                return Some(spec[start..end].to_string());
            }
        }
    }

    // Fallback: naive line-based extraction (handles unquoted ids)
    for line in spec.lines() {
        let l = line.trim();
        if l.starts_with("capability ") || l.starts_with("(capability ") {
            let parts: Vec<&str> = l.split_whitespace().collect();
            if parts.len() >= 2 {
                let mut candidate = parts[1].to_string();
                if candidate.starts_with('(') { candidate = candidate[1..].to_string(); }
                candidate = candidate.trim_matches('"').to_string();
                return Some(candidate);
            }
        }
    }
    None
}

/// Try parsing the provided RTFS spec using the project parser and validate it contains a Capability top-level
fn parse_and_validate_capability(spec: &str) -> Result<(), String> {
    // The project's parser expects a full RTFS program; attempt to strip fenced markers if present
    let mut src = spec.to_string();
    // If spec is wrapped in a fenced code block (e.g., ```rtfs or ```plaintext), strip fences
    if let Some(first) = src.find("```") {
        if let Some(second_rel) = src[first + 3..].find("```") {
            src = src[first + 3..first + 3 + second_rel].to_string();
        }
    }

    // Use crate::parser to parse
    match rtfs_compiler::parser::parse_with_enhanced_errors(&src, None) {
        Ok(items) => {
            // Ensure at least one TopLevel::Capability is present
            for tl in items.iter() {
                match tl {
                    rtfs_compiler::ast::TopLevel::Capability(_) => return Ok(()),
                    _ => {}
                }
            }
            Err("Parsed RTFS but no (capability ...) top-level form found".to_string())
        }
        Err(e) => Err(format!("RTFS parse error: {}", e)),
    }
}

/// Try to extract a balanced (capability ...) s-expression or fenced ```rtfs``` block
fn extract_capability_block(raw: &str) -> Option<String> {
    // 1) check for any fenced triple-backtick block (```<label> ... ```)
    if let Some(fence_start) = raw.find("```") {
        if let Some(fence_end_rel) = raw[fence_start + 3..].find("```") {
            let fenced = &raw[fence_start + 3..fence_start + 3 + fence_end_rel];
            if let Some(idx) = fenced.find("(capability") {
                if let Some(block) = extract_balanced_sexpr(fenced, idx) {
                    return Some(block);
                }
            }
        }
    }

    // 2) Search raw for a top-level (capability ...)
    if let Some(idx) = raw.find("(capability") {
        if let Some(block) = extract_balanced_sexpr(raw, idx) {
            return Some(block);
        }
    }

    None
}

/// Minimal balanced s-expression extractor starting at start_idx where '(' is expected
fn extract_balanced_sexpr(text: &str, start_idx: usize) -> Option<String> {
    let bytes = text.as_bytes();
    if bytes.get(start_idx) != Some(&b'(') {
        return None;
    }
    let mut depth: isize = 0;
    for (i, ch) in text[start_idx..].char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    let end = start_idx + i + 1;
                    return Some(text[start_idx..end].to_string());
                }
            }
            _ => {}
        }
    }
    None
}

fn persist_capability_spec(id: &str, spec: &str) -> Result<(), Box<dyn std::error::Error>> {
    use std::fs; use std::path::PathBuf;
    let dir = PathBuf::from("generated_capabilities");
    if !dir.exists() { fs::create_dir_all(&dir)?; }
    let mut file = dir.clone();
    file.push(format!("{}.rtfs", id));
    fs::write(file, spec)?;
    Ok(())
}

/// Enhanced response handling system for multi-turn interactions
struct ResponseHandler {
    collected_responses: HashMap<String, String>,
    pending_questions: Vec<PendingQuestion>,
}

#[derive(Clone, Debug)]
struct PendingQuestion {
    question_id: String,
    prompt: String,
    context: String,
    suggested_response: String,
}

impl ResponseHandler {
    fn new() -> Self {
        Self {
            collected_responses: HashMap::new(),
            pending_questions: Vec::new(),
        }
    }

    /// Analyze a plan execution to identify pending questions and generate responses
    fn analyze_plan_execution(&mut self, ccos: &Arc<CCOS>, plan_id: &str) -> Vec<(String, String)> {
        self.pending_questions.clear();

        // Analyze the causal chain to find PlanPaused actions
        if let Ok(chain) = ccos.get_causal_chain().lock() {
            let actions = chain.get_all_actions();
            for action in actions.iter().rev() {
                if action.plan_id == plan_id && action.action_type == rtfs_compiler::ccos::types::ActionType::PlanPaused {
                    if let Some(question) = self.extract_question_from_action(action) {
                        self.pending_questions.push(question);
                    }
                }
            }
        }

        // Generate responses for pending questions
        let mut responses = Vec::new();
        for question in &self.pending_questions {
            let response = self.generate_response_for_question(question, &self.collected_responses);
            responses.push((question.question_id.clone(), response));
        }

        responses
    }

    /// Extract question details from a PlanPaused action
    fn extract_question_from_action(&self, action: &rtfs_compiler::ccos::types::Action) -> Option<PendingQuestion> {
        if let Some(args) = &action.arguments {
            if args.len() >= 2 {
                if let rtfs_compiler::runtime::values::Value::String(prompt) = &args[1] {
                    let question_id = generate_question_key(prompt).unwrap_or_else(|| "unknown_question".to_string());
                    let response_map = HashMap::from([
                        ("name".to_string(), "John Doe".to_string()),
                        ("destination".to_string(), "Paris".to_string()),
                        ("duration".to_string(), "2".to_string()),
                        ("interests".to_string(), "art, food, history".to_string()),
                        ("dates".to_string(), "July 10-20".to_string()),
                        ("budget".to_string(), "$2000".to_string()),
                    ]);
                    let suggested_response = generate_contextual_response(prompt, &self.collected_responses, &response_map);

                    return Some(PendingQuestion {
                        question_id,
                        prompt: prompt.clone(),
                        context: action.plan_id.clone(),
                        suggested_response,
                    });
                }
            }
        }
        None
    }

    /// Generate a response for a specific question
    fn generate_response_for_question(&self, question: &PendingQuestion, collected_responses: &HashMap<String, String>) -> String {
        // Use collected responses if available, otherwise use suggested response
        if let Some(collected) = collected_responses.get(&question.question_id) {
            collected.clone()
        } else {
            question.suggested_response.clone()
        }
    }

    /// Extract and store responses from execution results
    fn extract_and_store_responses(&mut self, result: &rtfs_compiler::ccos::types::ExecutionResult) {
        match &result.value {
            rtfs_compiler::runtime::values::Value::String(response_value) => {
                // Try to identify if this contains structured response data
                if let Some(response_data) = self.parse_response_data(&response_value) {
                    for (key, value) in response_data {
                        self.collected_responses.insert(key, value);
                    }
                } else {
                    // Store as a general response if it looks like user-provided content
                    if self.is_user_response(&response_value) {
                        self.collected_responses.insert("last_response".to_string(), response_value.clone());
                    }
                }
            }
            rtfs_compiler::runtime::values::Value::Map(map) => {
                // Handle structured response data
                for (key, value) in map {
                    if let rtfs_compiler::runtime::values::Value::String(val_str) = value {
                        let key_str = key.to_string();
                        self.collected_responses.insert(key_str, val_str.clone());
                    }
                }
            }
            _ => {}
        }
    }

    /// Parse response data from string format
    fn parse_response_data(&self, response_value: &str) -> Option<HashMap<String, String>> {
        // Look for patterns like "response-from-step-name:value" or structured formats
        let mut responses = HashMap::new();

        // Simple pattern: look for response references in the text
        for line in response_value.lines() {
            if line.contains("response-from-") {
                if let Some(colon_idx) = line.find(':') {
                    let key = line[..colon_idx].trim().to_string();
                    let value = line[colon_idx + 1..].trim().to_string();
                    responses.insert(key, value);
                }
            }
        }

        if responses.is_empty() {
            None
        } else {
            Some(responses)
        }
    }

    /// Check if a string value looks like a user response
    fn is_user_response(&self, value: &str) -> bool {
        // Simple heuristics for identifying user responses
        value.contains("Hello") ||
        value.contains("recommend") ||
        value.contains("Based on") ||
        (value.len() > 10 && !value.contains("Error") && !value.contains("failed"))
    }
}

/// Extract context variables from a successful plan execution result
fn extract_context_from_result(result: &rtfs_compiler::ccos::types::ExecutionResult) -> HashMap<String, String> {
    let mut context = HashMap::new();
    
    match &result.value {
        rtfs_compiler::runtime::values::Value::Map(map) => {
            // Extract structured data from the result map
            for (key, value) in map {
                if let rtfs_compiler::runtime::values::Value::String(val_str) = value {
                    let key_str = key.to_string();
                    // Only include meaningful context variables (skip system fields)
                    if !key_str.starts_with("_") && !key_str.starts_with("system") {
                        context.insert(key_str, val_str.clone());
                    }
                }
            }
        }
        rtfs_compiler::runtime::values::Value::String(response_value) => {
            // Try to parse structured response data from string
            if let Some(response_data) = parse_response_data(response_value) {
                for (key, value) in response_data {
                    context.insert(key, value);
                }
            } else {
                // Store as a general response if it looks like user-provided content
                if is_user_response(response_value) {
                    context.insert("last_response".to_string(), response_value.clone());
                }
            }
        }
        _ => {
            // For other value types, try to convert to string
            let value_str = format!("{:?}", result.value);
            if value_str.len() > 0 && !value_str.contains("Error") {
                context.insert("result".to_string(), value_str);
            }
        }
    }
    
    context
}

/// Parse response data from string format (helper function)
fn parse_response_data(response_value: &str) -> Option<HashMap<String, String>> {
    let mut responses = HashMap::new();

    // Look for patterns like "response-from-step-name:value" or structured formats
    for line in response_value.lines() {
        if line.contains("response-from-") {
            if let Some(colon_idx) = line.find(':') {
                let key = line[..colon_idx].trim().to_string();
                let value = line[colon_idx + 1..].trim().to_string();
                responses.insert(key, value);
            }
        }
    }

    if responses.is_empty() {
        None
    } else {
        Some(responses)
    }
}

/// Check if a string value looks like a user response (standalone function)
fn is_user_response(value: &str) -> bool {
    // Simple heuristics for identifying user responses
    value.contains("Hello") ||
    value.contains("recommend") ||
    value.contains("Based on") ||
    (value.len() > 10 && !value.contains("Error") && !value.contains("failed"))
}

/// Extract pending questions from a plan and generate appropriate responses based on context
fn extract_pending_questions_and_generate_responses(
    ccos: &Arc<CCOS>,
    plan_id: &str,
    collected_responses: &HashMap<String, String>,
) -> HashMap<String, String> {
    let mut response_handler = ResponseHandler::new();

    // Transfer collected responses to the handler
    response_handler.collected_responses = collected_responses.clone();

    // Analyze the plan execution and generate responses
    let responses = response_handler.analyze_plan_execution(ccos, plan_id);

    // Convert to HashMap format expected by caller
    responses.into_iter().collect()
}

/// Extract question prompt from a PlanPaused action
fn extract_question_prompt_from_action(action: &rtfs_compiler::ccos::types::Action) -> Option<String> {
    if let Some(args) = &action.arguments {
        if args.len() >= 2 {
            match &args[1] {
                rtfs_compiler::runtime::values::Value::String(prompt) => {
                    return Some(prompt.clone());
                }
                rtfs_compiler::runtime::values::Value::Map(map) => {
                    // Try common keys used for prompts
                    if let Some(p) = get_map_string_value(map, "prompt") {
                        return Some(p.clone());
                    }
                    if let Some(p) = get_map_string_value(map, "question") {
                        return Some(p.clone());
                    }
                    if let Some(p) = get_map_string_value(map, "text") {
                        return Some(p.clone());
                    }

                    // Fallback: return the first string value found in the map
                    for (_k, v) in map.iter() {
                        if let rtfs_compiler::runtime::values::Value::String(s) = v {
                            return Some(s.clone());
                        }
                    }
                }
                _ => {}
            }
        }
    }
    None
}

/// Helper: get a string value for a key from a runtime Value::Map whose keys are MapKey.
fn get_map_string_value<'a>(map: &'a std::collections::HashMap<rtfs_compiler::ast::MapKey, Value>, key: &str) -> Option<&'a String> {
    if let Some(value) = map.get(&rtfs_compiler::ast::MapKey::Keyword(rtfs_compiler::ast::Keyword::new(key))) {
        if let Value::String(s) = value {
            return Some(s);
        }
    }
    None
}

/// Generate a contextual response based on the question prompt and collected responses
fn generate_contextual_response(
    question_prompt: &str,
    _collected_responses: &HashMap<String, String>,
    response_map: &HashMap<String, String>,
) -> String {
    // Check if the question prompt matches any key in the response map
    for (key, response) in response_map {
        if question_prompt.contains(key) {
            return response.clone();
        }
    }

    // Default response for unknown questions
    "Yes, that sounds good".to_string()
}

/// Generate a unique key for a question to use in environment variable naming
fn generate_question_key(question_prompt: &str) -> Option<String> {
    // Simple key generation based on question content
    if question_prompt.contains("name") {
        Some("name".to_string())
    } else if question_prompt.contains("destination") {
        Some("destination".to_string())
    } else if question_prompt.contains("duration") {
        Some("duration".to_string())
    } else if question_prompt.contains("interests") {
        Some("interests".to_string())
    } else if question_prompt.contains("dates") {
        Some("dates".to_string())
    } else if question_prompt.contains("budget") {
        Some("budget".to_string())
    } else {
        // Generate a hash-based key for unknown questions
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut hasher = DefaultHasher::new();
        question_prompt.hash(&mut hasher);
        Some(format!("q_{:x}", hasher.finish()))
    }
}


/// Clean up response environment variables after use
fn cleanup_response_env_vars() {
    // Remove any CCOS_USER_ASK_RESPONSE_* environment variables
    let keys_to_remove = std::env::vars()
        .filter_map(|(key, _)| {
            if key.starts_with("CCOS_USER_ASK_RESPONSE_") {
                Some(key)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();

    for key in keys_to_remove {
        std::env::remove_var(&key);
    }
}

/// Find the latest PlanPaused action for a given plan_id and return the
/// checkpoint id (first argument) if present.
fn find_latest_plan_checkpoint(ccos: &Arc<CCOS>, plan_id: &str) -> Option<String> {
    // Access the causal chain to find actions related to this plan
    let chain = ccos.get_causal_chain();
    let history = chain.lock().unwrap();
    let plan_actions = history.get_actions_for_plan(&plan_id.to_string());

    // Find the most recent 'PlanPaused' action
    let latest_checkpoint = plan_actions
        .iter()
        .filter_map(|action| {
            // Match PlanPaused actions and extract a checkpoint id from metadata if present.
            if let rtfs_compiler::ccos::types::Action { action_type, metadata, .. } = action {
                if *action_type == rtfs_compiler::ccos::types::ActionType::PlanPaused {
                    if let Some(Value::String(cp)) = metadata.get("checkpoint_id") {
                        return Some(cp.clone());
                    }
                }
                None
            } else {
                None
            }
        })
        .last();

    latest_checkpoint
}


/// Check if the final value from a plan execution signals that refinement is complete.
fn is_refinement_exhausted(value: &Value) -> bool {
    if let Value::Map(map) = value {
        // Match against MapKey::Keyword("status")
        if let Some(status_val) = map.get(&rtfs_compiler::ast::MapKey::Keyword(rtfs_compiler::ast::Keyword::new("status"))) {
            if let Value::String(s) = status_val {
                return s == "refinement_exhausted";
            }
        }
    }
    false
}


fn snapshot_intent_ids(ccos: &Arc<CCOS>) -> HashSet<String> {
    // Fallback: list intents snapshot and extract ids
    let snapshot = ccos.list_intents_snapshot();
    snapshot.into_iter().map(|i| i.intent_id).collect()
}

fn fetch_intent_goal(ccos: &Arc<CCOS>, id: &str) -> Option<String> {
    // Use snapshot or direct get_intent
    if let Ok(ig) = ccos.get_intent_graph().lock() {
        let id_str = id.to_string();
        if let Some(intent) = ig.get_intent(&id_str) {
            return Some(intent.goal.clone());
        }
    }
    None
}

fn render_ascii_graph(root: Option<&String>, intents: &HashMap<String, String>) {
    println!("\n{}", "Current Intent Graph".bold());
    println!("{}", "---------------------".bold());
    if intents.is_empty() { println!("(empty)"); return; }

    if let Some(root_id) = root {
        println!("{} {}", format!("ROOT {}", short(root_id)).bold().yellow(), display_goal(intents.get(root_id)));
        // Phase 1: naive — treat all non-root as direct descendants (will evolve later)
        for (id, goal) in intents.iter() {
            if id == root_id { continue; }
            println!("  └─ {} {}", short(id).cyan(), display_goal(Some(goal)));
        }
    } else {
        for (id, goal) in intents.iter() {
            println!("{} {}", short(id), display_goal(Some(goal)));
        }
    }
}

fn display_goal(goal_opt: Option<&String>) -> String {
    goal_opt.map(|g| truncate(g, 70)).unwrap_or_else(|| "(no goal)".into())
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max { s.to_string() } else { format!("{}…", &s[..max]) }
}

// Removed serde_json-based truncation; runtime Value is rendered via Display already.

fn short(id: &str) -> String {
    if id.len() <= 10 { id.to_string() } else { format!("{}", &id[..10]) }
}

fn prompt_user(label: &str) -> io::Result<String> {
    print!("{}", label);
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(input.trim_end().to_string())
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
            "[config] applied profile '{}' provider={} model={}",
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
