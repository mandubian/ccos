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

use clap::Parser;
use crossterm::style::Stylize;
use rtfs_compiler::ccos::CCOS;
use rtfs_compiler::ccos::arbiter::ArbiterEngine;
use rtfs_compiler::config::profile_selection::ProfileMeta;
use rtfs_compiler::config::types::{AgentConfig, LlmProfile};
use rtfs_compiler::config::validation::validate_config;
use rtfs_compiler::config::{auto_select_model, expand_profiles};
use rtfs_compiler::ccos::types::StorableIntent;
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

    /// Auto-pick best model within prompt cost budget (USD per 1K tokens)
    #[arg(long)]
    model_auto_prompt_budget: Option<f64>,

    /// Auto-pick best model within completion cost budget (USD per 1K tokens)
    #[arg(long)]
    model_auto_completion_budget: Option<f64>,
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
        println!("\n{}: {}", format!("User Turn {}", turn + 1).cyan(), current_request);

        let before_ids = snapshot_intent_ids(&ccos);
        let request = current_request.clone();

        let _context: Option<std::collections::HashMap<String, Value>> = if let Some(root_id) = &root_intent {
            if let Some(parent_goal) = known_intents.get(root_id) {
                Some(std::collections::HashMap::from([
                    ("parent_intent_id".to_string(), Value::String(root_id.clone())),
                    ("parent_goal".to_string(), Value::String(parent_goal.clone())),
                    ("relationship_type".to_string(), Value::String("refinement_of".to_string())),
                ]))
            } else { None }
        } else { None };

        // For context passing demonstration, we'll use the delegating arbiter directly
        // when we have accumulated context, otherwise fall back to the standard flow
        let plan_and_result = if !accumulated_context.is_empty() {
            // Use delegating arbiter directly to pass context
            if let Some(arbiter) = ccos.get_delegating_arbiter() {
                match arbiter.natural_language_to_intent(&request, None).await {
                    Ok(intent) => {
                        // Convert to storable intent
                        let storable_intent = rtfs_compiler::ccos::types::StorableIntent {
                            intent_id: intent.intent_id.clone(),
                            name: intent.name.clone(),
                            original_request: intent.original_request.clone(),
                            rtfs_intent_source: "".to_string(),
                            goal: intent.goal.clone(),
                            constraints: intent.constraints.iter()
                                .map(|(k, v)| (k.clone(), v.to_string()))
                                .collect(),
                            preferences: intent.preferences.iter()
                                .map(|(k, v)| (k.clone(), v.to_string()))
                                .collect(),
                            success_criteria: intent.success_criteria.as_ref().map(|v| v.to_string()),
                            parent_intent: None,
                            child_intents: vec![],
                            triggered_by: rtfs_compiler::ccos::types::TriggerSource::HumanRequest,
                            generation_context: rtfs_compiler::ccos::types::GenerationContext {
                                arbiter_version: "delegating-1.0".to_string(),
                                generation_timestamp: intent.created_at,
                                input_context: HashMap::new(),
                                reasoning_trace: None,
                            },
                            status: intent.status.clone(),
                            priority: 0,
                            created_at: intent.created_at,
                            updated_at: intent.updated_at,
                            metadata: HashMap::new(),
                        };

                        // Generate plan (context passing not yet exposed in public API)
                        match arbiter.intent_to_plan(&intent).await {
                            Ok(plan) => {
                                if args.verbose {
                                    println!("Generated plan with context: {}", plan.plan_id);
                                    println!("Available context: {:?}", accumulated_context);
                                }
                                
                                // Execute the plan
                                match ccos.get_orchestrator().execute_plan(&plan, &ctx).await {
                                    Ok(result) => Ok((plan, result)),
                                    Err(e) => Err(e),
                                }
                            }
                            Err(e) => Err(e),
                        }
                    }
                    Err(e) => Err(e),
                }
            } else {
                // Fallback to standard flow if no delegating arbiter
                ccos.process_request_with_plan(&request, &ctx).await
            }
        } else {
            // Use standard flow when no context available
            ccos.process_request_with_plan(&request, &ctx).await
        };
        let mut next_request = String::new();
        match plan_and_result {
            Ok((plan, res)) => {
                if args.verbose {
                    println!("{} success={} value={}", "✔ Execution".green(), res.success, res.value);
                }
                // Handle successful plan execution - extract context for future plans
                if res.success {
                    if args.verbose {
                        println!("{}", "Plan execution successful - extracting context...".green());
                    }
                    
                    // Extract context from successful execution
                    let new_context = extract_context_from_result(&res);
                    if !new_context.is_empty() {
                        accumulated_context.extend(new_context.clone());
                        if args.verbose {
                            println!("Extracted context: {:?}", new_context);
                            println!("Accumulated context: {:?}", accumulated_context);
                        }
                    }
                }
                
                // If execution paused (success==false) we attempt to find a PlanPaused
                // action for this plan and resume-and-continue using the orchestrator.
                if !res.success {
                    if let Some(checkpoint_id) = find_latest_plan_checkpoint(&ccos, &plan.plan_id) {
                        if args.verbose { println!("Detected PlanPaused checkpoint={} — resuming...", checkpoint_id); }

                        // Extract pending questions and generate appropriate responses
                        let pending_responses = extract_pending_questions_and_generate_responses(&ccos, &plan.plan_id, &collected_responses);

                        // Set responses for the orchestrator to use
                        for (question_key, response) in pending_responses {
                            std::env::set_var(&format!("CCOS_USER_ASK_RESPONSE_{}", question_key), response);
                        }

                        // Resume and continue until completion or next pause
                        match ccos.get_orchestrator().resume_and_continue_from_checkpoint(&plan, &ctx, &checkpoint_id).await {
                            Ok(resumed) => {
                                if args.verbose { println!("Resume result success={} value={}", resumed.success, resumed.value); }

                                // Extract any new responses from the resumed execution using the enhanced handler
                                let mut response_handler = ResponseHandler::new();
                                response_handler.collected_responses = collected_responses.clone();
                                response_handler.extract_and_store_responses(&resumed);

                                // Update collected responses
                                collected_responses.extend(response_handler.collected_responses);

                                // Check for explicit refinement exhaustion signal from model
                                if let Value::Map(m) = &resumed.value {
                                    if let Some(s) = get_map_string_value(m, "status") {
                                        if s == "refinement_exhausted" {
                                            println!("{}", "[model] Refinement exhausted signal received; stopping.".yellow());
                                            next_request.clear(); // force termination
                                        }
                                    }
                                }

                                // Stagnation detection: inspect PlanPaused prompts and see if any new prompt appeared
                                let mut new_question_seen = false;
                                if let Ok(chain) = ccos.get_causal_chain().lock() {
                                    for action in chain.get_all_actions().iter().rev() {
                                        if action.plan_id == plan.plan_id && action.action_type == rtfs_compiler::ccos::types::ActionType::PlanPaused {
                                            if let Some(prompt) = extract_question_prompt_from_action(action) {
                                                if asked_questions.insert(prompt) {
                                                    new_question_seen = true;
                                                }
                                            }
                                        }
                                    }
                                }
                                if !new_question_seen {
                                    stagnant_turns += 1;
                                } else {
                                    stagnant_turns = 0;
                                }
                                if stagnant_turns >= STAGNATION_LIMIT {
                                    println!("{}", "\n[stagnation] No new refinement questions emerging; assuming refinement exhausted.".yellow());
                                    println!("[stagnation] Model likely needs external capabilities (e.g., travel.search, itinerary.optimize).");
                                    println!("[stagnation] Ending progressive interaction.");
                                    next_request.clear();
                                }

                                next_request = resumed.value.to_string();
                            }
                            Err(e) => {
                                eprintln!("Resume error: {}", e);
                            }
                        }

                        // Clear response environment variables after use
                        cleanup_response_env_vars();
                    }
                } else {
                    // Use the successful execution value as the next user input
                    next_request = res.value.to_string();

                    // Extract any responses from successful execution using the enhanced handler
                    let mut response_handler = ResponseHandler::new();
                    response_handler.collected_responses = collected_responses.clone();
                    response_handler.extract_and_store_responses(&res);

                    // Update collected responses
                    collected_responses.extend(response_handler.collected_responses);

                    // Check for explicit refinement exhaustion signal from model on successful finish
                    if let Value::Map(m) = &res.value {
                        if let Some(s) = get_map_string_value(m, "status") {
                            if s == "refinement_exhausted" {
                                println!("{}", "[model] Refinement exhausted signal received; stopping.".yellow());
                                next_request.clear(); // force termination
                            }
                        }
                    }

                    // Stagnation detection for successful runs as well
                    let mut new_question_seen = false;
                    if let Ok(chain) = ccos.get_causal_chain().lock() {
                        for action in chain.get_all_actions().iter().rev() {
                            if action.plan_id == plan.plan_id && action.action_type == rtfs_compiler::ccos::types::ActionType::PlanPaused {
                                if let Some(prompt) = extract_question_prompt_from_action(action) {
                                    if asked_questions.insert(prompt) {
                                        new_question_seen = true;
                                    }
                                }
                            }
                        }
                    }
                    if !new_question_seen {
                        stagnant_turns += 1;
                    } else {
                        stagnant_turns = 0;
                    }
                    if stagnant_turns >= STAGNATION_LIMIT {
                        println!("{}", "\n[stagnation] No new refinement questions emerging; assuming refinement exhausted.".yellow());
                        println!("[stagnation] Model likely needs external capabilities (e.g., travel.search, itinerary.optimize).");
                        println!("[stagnation] Ending progressive interaction.");
                        next_request.clear();
                    }
                }
            }
            Err(e) => {
                eprintln!("{} {}", "✖ Error processing request:".red(), e);
            }
        }
        sleep(Duration::from_millis(50)).await; // Allow for propagation

        let after_ids = snapshot_intent_ids(&ccos);
        let new_ids: HashSet<_> = after_ids.difference(&before_ids).cloned().collect();

        let mut created_intent_for_turn = None;
        if !new_ids.is_empty() {
            // For simulation, we'll just grab the first new intent.
            let new_id = new_ids.iter().next().unwrap().clone();
            if let Some(intent) = ccos.list_intents_snapshot().into_iter().find(|i| i.intent_id == new_id) {
                known_intents.insert(new_id.clone(), intent.goal.clone());
                created_intent_for_turn = Some(intent);
            }
            if root_intent.is_none() {
                root_intent = Some(new_id);
            }
        }

        conversation_history.push(InteractionTurn {
            user_input: request,
            created_intent: created_intent_for_turn,
        });

        // Termination: stop if the runtime didn't return a meaningful next request
        let trimmed = next_request.trim().to_string();
        if trimmed.is_empty() {
            break;
        }
        // Prevent repeating identical requests (simple loop detection)
        if trimmed == current_request.trim().to_string() {
            break;
        }

        current_request = trimmed;
    }

    render_ascii_graph(root_intent.as_ref(), &known_intents);

    // --- Phase 3: Post-Mortem Analysis and Synthesis ---
    generate_synthesis_summary(&conversation_history, root_intent.as_ref());


    Ok(())
}

/// Performs a post-mortem analysis of the conversation to synthesize a new capability.
fn generate_synthesis_summary(history: &[InteractionTurn], _root_intent_id: Option<&String>) {
    println!("\n\n{}", "--- Capability Synthesis Analysis ---".bold());
    
    if history.is_empty() {
        println!("Conversation history is empty. Nothing to analyze.");
        return;
    }

    let root_goal = history.get(0)
        .and_then(|turn| turn.created_intent.as_ref())
        .map_or("Unknown".to_string(), |intent| intent.goal.clone());

    println!("{} {}", "Initial Goal:".bold(), root_goal);
    println!("{} {} turns", "Total Interaction Turns:".bold(), history.len());
    
    let refinements: Vec<String> = history.iter().skip(1)
        .filter_map(|turn| turn.created_intent.as_ref().map(|i| i.goal.clone()))
        .collect();

    if !refinements.is_empty() {
        println!("\n{}", "Detected Refinements:".bold());
        for (i, goal) in refinements.iter().enumerate() {
            println!("  {}. {}", i + 1, truncate(goal, 80));
        }
    }

    // Placeholder for the synthesized capability
    println!("\n{}", "Synthesized Capability (Placeholder):".bold().green());
    println!("{}", "------------------------------------".green());
    println!("{} plan_trip", "capability".cyan());
    println!("  {} \"Plans a detailed trip based on user preferences.\"", "description".cyan());
    println!("\n  {} {{", "parameters".cyan());
    println!("    destination: String,");
    println!("    duration_weeks: Integer,");
    println!("    month: String,");
    println!("    interests: [String],");
    println!("    dietary_needs: [String]");
    println!("  }}");
    println!("\n  {} {{", "steps".cyan());
    println!("    // 1. Decompose high-level goal into sub-intents.");
    println!("    // 2. Gather parameters (destination, duration, interests).");
    println!("    // 3. Search for activities based on interests.");
    println!("    // 4. Search for dining options based on dietary needs.");
    println!("    // 5. Assemble final itinerary.");
    println!("  }}");
    println!("{}", "------------------------------------".green());
    println!("\n{}", "This demonstrates how CCOS could learn a reusable 'plan_trip' capability from the specific interaction history.".italic());
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
            if let rtfs_compiler::runtime::values::Value::String(prompt) = &args[1] {
                return Some(prompt.clone());
            }
        }
    }
    None
}

/// Helper: get a string value for a key from a runtime Value::Map whose keys are MapKey.
fn get_map_string_value(map: &std::collections::HashMap<rtfs_compiler::ast::MapKey, Value>, key: &str) -> Option<String> {
    for (k, v) in map.iter() {
        // Convert the MapKey to a string without moving its internals. Keyword keys
        // are represented as ":name" by MapKey::to_string(), so strip a leading
        // ':' to compare against plain keys like "status".
        let k_str = k.to_string();
        let k_trim = k_str.trim_start_matches(':');
        if k_trim == key {
            return match v {
                Value::String(s) => Some(s.clone()),
                other => Some(other.to_string()),
            };
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
    if let Ok(chain) = ccos.get_causal_chain().lock() {
        let actions = chain.get_all_actions();
        for a in actions.iter().rev() {
            if a.plan_id == plan_id && a.action_type == rtfs_compiler::ccos::types::ActionType::PlanPaused {
                if let Some(args) = &a.arguments {
                    if let Some(first) = args.first() {
                        // Extract raw string for checkpoint id (Value::String stores without quotes)
                        if let rtfs_compiler::runtime::values::Value::String(s) = first {
                            return Some(s.clone());
                        } else {
                            // Fallback: remove surrounding quotes if Display formatting was used
                            let disp = first.to_string();
                            let trimmed = disp.trim_matches('"').to_string();
                            return Some(trimmed);
                        }
                    }
                }
            }
        }
    }
    None
}


fn snapshot_intent_ids(ccos: &Arc<CCOS>) -> HashSet<String> {
    ccos.list_intents_snapshot().into_iter().map(|i| i.intent_id).collect()
}

fn fetch_intent_goal(ccos: &Arc<CCOS>, id: &str) -> Option<String> {
    ccos.list_intents_snapshot().into_iter().find(|i| i.intent_id == id).map(|i| i.goal)
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

fn prompt(label: &str) -> io::Result<String> {
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
