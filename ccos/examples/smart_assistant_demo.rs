// Governed smart assistant demo with recursive capability synthesis.
//
// This demo integrates the RecursiveSynthesizer to automatically generate
// missing capabilities and their dependencies when executing user goals.
//
// Key features:
// - Natural language goal → Intent → Plan → Orchestrator RTFS
// - Automatic capability discovery (Marketplace → MCP → OpenAPI → Recursive Synthesis)
// - Missing capabilities trigger recursive synthesis with dependency resolution
// - Synthesized capabilities are registered in the marketplace for reuse
//
// Previous version (without recursive synthesis) is saved as smart_assistant_demo_v1.rs

use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::sync::Arc;

use chrono::Utc;
use clap::Parser;
use crossterm::style::Stylize;
use rtfs::ast::{Expression, Keyword, Literal, MapKey};
use ccos::arbiter::delegating_arbiter::DelegatingArbiter;
use ccos::capability_marketplace::types::CapabilityManifest;
use ccos::discovery::{CapabilityNeed, DiscoveryEngine, DiscoveryResult, DiscoveryHints, FoundCapability};
use ccos::intent_graph::config::IntentGraphConfig;
use ccos::types::{Intent, Plan};
use ccos::CCOS;
use rtfs::config::profile_selection::expand_profiles;
use rtfs::config::types::{AgentConfig, LlmProfile};
use rtfs::parser::parse_expression;
use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use rtfs::runtime::values::Value;
use serde_json;
use toml;

#[derive(Parser, Debug)]
#[command(
    name = "smart-assistant-demo",
    version,
    about = "Governed smart assistant demo driven by the delegating arbiter"
)]
struct Args {
    /// Path to an AgentConfig (TOML or JSON) with delegation-enabled profiles
    #[arg(long)]
    config: String,

    /// Optional natural language goal; if omitted you'll be prompted
    #[arg(long)]
    goal: Option<String>,

    /// Explicit LLM profile name to activate
    #[arg(long)]
    profile: Option<String>,

    /// Dump prompts and raw LLM responses for debugging
    #[arg(long, default_value_t = false)]
    debug_prompts: bool,

    /// Interactive mode: prompt user for clarifying question answers (default: auto-answer with LLM)
    #[arg(long, default_value_t = false)]
    interactive: bool,
}

#[derive(Debug, Clone)]
struct ClarifyingQuestion {
    id: String,
    key: String,
    prompt: String,
    rationale: String,
    answer_kind: AnswerKind,
    required: bool,
    default_answer: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AnswerKind {
    Text,
    List,
    Number,
    Boolean,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AnswerSource {
    IntentSeed,
    DelegatingAsk,
    EnvFallback,
    Default,
}

#[derive(Debug, Clone)]
struct AnswerRecord {
    key: String,
    text: String,
    value: Value,
    source: AnswerSource,
}

#[derive(Debug, Clone)]
struct ProposedStep {
    id: String,
    name: String,
    capability_class: String,
    candidate_capabilities: Vec<String>,
    required_inputs: Vec<String>,
    expected_outputs: Vec<String>,
    description: Option<String>,
}

#[derive(Debug, Clone)]
struct CapabilityMatch {
    step_id: String,
    matched_capability: Option<String>,
    status: MatchStatus,
    note: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MatchStatus {
    ExactId,
    MatchedByClass,
    Missing,
}

struct StubCapabilitySpec {
    id: &'static str,
    name: &'static str,
    description: &'static str,
    required_inputs: &'static [&'static str],
    expected_outputs: &'static [&'static str],
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();

    if let Err(error) = run_demo(args).await {
        eprintln!("{} {}", "✖ Demo failed:".bold().red(), error);
        std::process::exit(1);
    }

    Ok(())
}

async fn run_demo(args: Args) -> Result<(), Box<dyn Error>> {
    let agent_config = load_agent_config(&args.config)?;
    apply_llm_profile(&agent_config, args.profile.as_deref())?;

    // Print architecture summary before initializing
    print_architecture_summary(&agent_config, args.profile.as_deref());

    let ccos = Arc::new(
        CCOS::new_with_agent_config_and_configs_and_debug_callback(
            IntentGraphConfig::default(),
            None,
            Some(agent_config.clone()),
            None,
        )
        .await
        .map_err(runtime_error)?,
    );

    let delegating = ccos
        .get_delegating_arbiter()
        .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "Delegating arbiter unavailable"))?;

    // Print LLM provider details after initialization
    print_llm_provider_info(&delegating);

    let stub_specs = register_stub_capabilities(&ccos).await?;

    let goal = determine_goal(&args)?;
    println!("\n{} {}", "🎯 Goal:".bold(), goal.as_str().cyan());

    let (intent, raw_intent) = delegating
        .natural_language_to_intent_with_raw(&goal, None)
        .await
        .map_err(runtime_error)?;

    if args.debug_prompts {
        println!(
            "{}\n{}\n{}",
            "┌─ Raw intent response ───────────────────────────".dim(),
            raw_intent,
            "└─────────────────────────────────────────────────".dim()
        );
    }

    print_intent_summary(&intent);

    let mut questions =
        generate_clarifying_questions(&delegating, &goal, &intent, args.debug_prompts).await?;

    let mut seeded_answers = seed_answers_from_intent(&intent);
    for question in &mut questions {
        if seeded_answers.get(question.key.as_str()).is_some() {
            println!(
                "{} {}",
                "ℹ️  Using intent-provided answer for".dim(),
                question.key.as_str().cyan()
            );
        }
    }

    let answers = conduct_interview(
        &ccos,
        &delegating,
        &goal,
        &intent,
        &questions,
        &mut seeded_answers,
        args.debug_prompts,
        args.interactive,
    )
    .await?;

    println!("\n{}", "📋 Generating initial plan from intent...".bold().cyan());
    
    let mut plan_steps = match propose_plan_steps(
        &delegating,
        &goal,
        &intent,
        &answers,
        &stub_specs,
        args.debug_prompts,
    )
    .await
    {
        Ok(steps) if !steps.is_empty() => {
            println!("  {} Generated {} plan step(s)", "✓".green(), steps.len());
            steps
        }
        Ok(_) => {
            println!(
                "{}",
                "⚠️  Arbiter returned no plan steps; using fallback.".yellow()
            );
            fallback_steps()
        }
        Err(err) => {
            println!("{} {}", "⚠️  Failed to synthesize steps:".yellow(), err);
            fallback_steps()
        }
    };

    let matches = match_proposed_steps(&ccos, &plan_steps).await?;
    annotate_steps_with_matches(&mut plan_steps, &matches);

    // Check for missing capabilities and trigger re-planning if needed
    let missing_count = matches.iter().filter(|m| m.status == MatchStatus::Missing).count();
    if missing_count > 0 && ccos.get_delegating_arbiter().is_some() {
        println!(
            "\n{} {} {}",
            "🔄".yellow().bold(),
            "Some capabilities not found:".yellow(),
            format!("({} missing)", missing_count).yellow()
        );
        
        // Collect discovery hints for all capabilities in the plan
        // Build a map of capability_class -> description for better rationale
        let capability_info: Vec<(String, Option<String>)> = plan_steps.iter()
            .map(|s| (s.capability_class.clone(), s.description.clone()))
            .collect();
        
        let discovery_engine = DiscoveryEngine::new_with_arbiter(
            Arc::clone(&ccos.get_capability_marketplace()),
            Arc::clone(&ccos.get_intent_graph()),
            ccos.get_delegating_arbiter(),
        );
        
        let hints = discovery_engine.collect_discovery_hints_with_descriptions(&capability_info).await
            .map_err(|e| Box::new(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Failed to collect discovery hints: {}", e)
            )))?;
        
        if !hints.missing_capabilities.is_empty() {
            println!("  Missing: {}", hints.missing_capabilities.join(", ").yellow());
            println!("  Found: {} capabilities", hints.found_capabilities.len().to_string().green());
            
            // Show suggestions if available
            if !hints.suggestions.is_empty() {
                println!("\n  Suggestions:");
                for suggestion in &hints.suggestions {
                    println!("    • {}", suggestion.as_str().cyan());
                }
            }
            
            println!("\n{}", "Asking LLM to replan with available capabilities...".cyan());
            
            // Build re-plan prompt
            let replan_prompt = build_replan_prompt(&goal, &intent, &hints);
            
            if args.debug_prompts {
                println!(
                    "{}\n{}\n{}",
                    "┌─ Re-plan prompt ──────────────────────────".dim(),
                    replan_prompt,
                    "└────────────────────────────────────────────".dim()
                );
            }
            
            // Get new plan steps from LLM
            let response = delegating
                .generate_raw_text(&replan_prompt)
                .await
                .map_err(runtime_error)?;
            
            if args.debug_prompts {
                println!(
                    "{}\n{}\n{}",
                    "┌─ Re-plan response ────────────────────────".dim(),
                    response,
                    "└────────────────────────────────────────────".dim()
                );
            }
            
            // Parse the new plan steps
            let mut parsed_value = parse_plan_steps_response(&response).map_err(runtime_error)?;
            if let Value::Map(map) = &parsed_value {
                if let Some(Value::Vector(steps)) = map_get(map, "steps") {
                    parsed_value = Value::Vector(steps.clone());
                }
            }
            
            if let Value::Vector(items) = parsed_value {
                let mut new_steps = Vec::new();
                for (idx, item) in items.iter().enumerate() {
                    if let Some(step) = value_to_step(item) {
                        new_steps.push(step);
                    } else if let Some(step) = step_from_free_form(item, idx) {
                        new_steps.push(step);
                    }
                }
                
                if !new_steps.is_empty() {
                    println!("  {} New plan generated with {} steps", "✓".green(), new_steps.len().to_string().green());
                    plan_steps = new_steps;
                    
                    // Re-match with new plan
                    let new_matches = match_proposed_steps(&ccos, &plan_steps).await?;
                    annotate_steps_with_matches(&mut plan_steps, &new_matches);
                    
                    // Update matches for resolution
                    let matches = new_matches;
                    
                    let needs_value = build_needs_capabilities(&plan_steps);
                    
                    // Resolve missing capabilities and build orchestrating agent
                    let resolved_steps = resolve_and_stub_capabilities(&ccos, &plan_steps, &matches, args.interactive).await?;
                    let orchestrator_rtfs = generate_orchestrator_capability(&goal, &resolved_steps)?;
                    
                    // Register the orchestrator as a reusable capability in the marketplace
                    let planner_capability_id = format!("synth.plan.orchestrator.{}", chrono::Utc::now().timestamp());
                    register_orchestrator_in_marketplace(&ccos, &planner_capability_id, &orchestrator_rtfs).await?;
                    
                    let mut plan = Plan::new_rtfs(orchestrator_rtfs, vec![]);
                    plan.metadata
                        .insert("needs_capabilities".to_string(), needs_value.clone());
                    plan.metadata.insert(
                        "generated_at".to_string(),
                        Value::String(Utc::now().to_rfc3339()),
                    );
                    plan.metadata.insert(
                        "resolved_steps".to_string(),
                        build_resolved_steps_metadata(&resolved_steps),
                    );
                    plan.metadata.insert(
                        "orchestrator_capability_id".to_string(),
                        Value::String(planner_capability_id),
                    );
                    
                    print_plan_draft(&plan_steps, &matches, &plan);
                    
                    // Print resolution summary
                    let found_count = resolved_steps.iter().filter(|s| s.resolution_strategy == ResolutionStrategy::Found).count();
                    let synthesized_count = resolved_steps.iter().filter(|s| s.resolution_strategy == ResolutionStrategy::Synthesized).count();
                    let stubbed_count = resolved_steps.iter().filter(|s| s.resolution_strategy == ResolutionStrategy::Stubbed).count();
                    
                    println!("\n{}", "📊 Capability Resolution Summary".bold());
                    println!("   • Found: {} capabilities", found_count.to_string().green());
                    if synthesized_count > 0 {
                        println!("   • {}: {} capabilities (with dependencies)", 
                                 "Synthesized".bold(), 
                                 synthesized_count.to_string().cyan().bold());
                    }
                    if stubbed_count > 0 {
                        println!("   • Stubbed: {} capabilities (awaiting implementation)", stubbed_count.to_string().yellow());
                    }
                    
                    // Display execution graph visualization
                    print_execution_graph(&resolved_steps, &intent);
                    
                    println!(
                        "\n{}",
                        "✅ Orchestrator generated and registered in marketplace".bold().green()
                    );
                    
                    return Ok(());
                } else {
                    println!("  {} Re-plan failed to generate valid steps, proceeding with original plan", "⚠️".yellow());
                }
            }
        }
    }

    let needs_value = build_needs_capabilities(&plan_steps);
    
    // Resolve missing capabilities and build orchestrating agent
    let resolved_steps = resolve_and_stub_capabilities(&ccos, &plan_steps, &matches, args.interactive).await?;
    let orchestrator_rtfs = generate_orchestrator_capability(&goal, &resolved_steps)?;
    
    // Register the orchestrator as a reusable capability in the marketplace
    let planner_capability_id = format!("synth.plan.orchestrator.{}", chrono::Utc::now().timestamp());
    register_orchestrator_in_marketplace(&ccos, &planner_capability_id, &orchestrator_rtfs).await?;
    
    let mut plan = Plan::new_rtfs(orchestrator_rtfs, vec![]);
    plan.metadata
        .insert("needs_capabilities".to_string(), needs_value.clone());
    plan.metadata.insert(
        "generated_at".to_string(),
        Value::String(Utc::now().to_rfc3339()),
    );
    plan.metadata.insert(
        "resolved_steps".to_string(),
        build_resolved_steps_metadata(&resolved_steps),
    );
    plan.metadata.insert(
        "orchestrator_capability_id".to_string(),
        Value::String(planner_capability_id),
    );

    print_plan_draft(&plan_steps, &matches, &plan);
    
    // Print resolution summary
    let found_count = resolved_steps.iter().filter(|s| s.resolution_strategy == ResolutionStrategy::Found).count();
    let synthesized_count = resolved_steps.iter().filter(|s| s.resolution_strategy == ResolutionStrategy::Synthesized).count();
    let stubbed_count = resolved_steps.iter().filter(|s| s.resolution_strategy == ResolutionStrategy::Stubbed).count();
    
    println!("\n{}", "📊 Capability Resolution Summary".bold());
    println!("   • Found: {} capabilities", found_count.to_string().green());
    if synthesized_count > 0 {
        println!("   • {}: {} capabilities (with dependencies)", 
                 "Synthesized".bold(), 
                 synthesized_count.to_string().cyan().bold());
    }
    if stubbed_count > 0 {
        println!("   • Stubbed: {} capabilities (awaiting implementation)", stubbed_count.to_string().yellow());
    }
    
    // Display execution graph visualization
    print_execution_graph(&resolved_steps, &intent);
    
    println!(
        "\n{}",
        "✅ Orchestrator generated and registered in marketplace".bold().green()
    );

    Ok(())
}

type DemoResult<T> = Result<T, Box<dyn Error>>;

fn runtime_error(err: RuntimeError) -> Box<dyn Error> {
    Box::new(err)
}

/// Print architecture summary and configuration
fn print_architecture_summary(config: &AgentConfig, profile_name: Option<&str>) {
    println!("\n{}", "═".repeat(80).bold());
    println!("{}", "🏗️  CCOS Smart Assistant - Architecture Summary".bold().cyan());
    println!("{}", "═".repeat(80).bold());
    
    println!("\n{}", "📋 Architecture Overview".bold());
    println!("  ┌─────────────────────────────────────────────────────────────┐");
    println!("  │ User Goal → Intent Extraction → Plan Generation → Execution │");
    println!("  └─────────────────────────────────────────────────────────────┘");
    println!("\n  {} Flow:", "1.".bold());
    println!("     • Natural language goal → Intent (with constraints/criteria)");
    println!("     • Intent → Clarifying questions (auto-answered by LLM)");
    println!("     • Refined Intent → Plan steps (with capability needs)");
    println!("     • Capability Discovery:");
    println!("       - Local Marketplace → MCP Registry → OpenAPI → Recursive Synthesis");
    println!("     • Re-planning with hints (if capabilities missing)");
    println!("     • Execution graph construction → Orchestrator RTFS");
    
    println!("\n  {} Key Components:", "2.".bold());
    println!("     • {}: Governs intent extraction, plan generation, execution", "DelegatingArbiter".cyan());
    println!("     • {}: Finds/synthesizes missing capabilities", "DiscoveryEngine".cyan());
    println!("     • {}: Recursively generates missing capabilities", "RecursiveSynthesizer".cyan());
    println!("     • {}: Manages capability registration and search", "CapabilityMarketplace".cyan());
    println!("     • {}: Tracks intent relationships and dependencies", "IntentGraph".cyan());
    
    // Show LLM profile
    if let Some(llm_profiles) = &config.llm_profiles {
        let (profiles, _meta, _why) = expand_profiles(config);
        let chosen = profile_name
            .map(|s| s.to_string())
            .or_else(|| llm_profiles.default.clone())
            .or_else(|| profiles.first().map(|p| p.name.clone()));
        
        if let Some(name) = chosen {
            if let Some(profile) = profiles.iter().find(|p| p.name == name) {
                println!("\n  {} LLM Configuration:", "3.".bold());
                println!("     • Profile: {}", name.cyan());
                println!("     • Provider: {}", profile.provider.as_str().cyan());
                println!("     • Model: {}", profile.model.as_str().cyan());
                if let Some(url) = &profile.base_url {
                    println!("     • Base URL: {}", url.as_str().dim());
                }
            }
        }
    }
    
    println!("\n{}", "═".repeat(80).dim());
}

/// Print detailed LLM provider information after initialization
fn print_llm_provider_info(delegating: &DelegatingArbiter) {
    let _llm_config = delegating.get_llm_config(); // Available for future use
    println!("\n{}", "🤖 Active LLM Provider".bold());
    let provider = std::env::var("CCOS_LLM_PROVIDER").unwrap_or_else(|_| "unknown".to_string());
    let model = std::env::var("CCOS_LLM_MODEL").unwrap_or_else(|_| "unknown".to_string());
    println!("  • Provider: {}", provider.cyan());
    println!("  • Model: {}", model.cyan());
    if let Ok(base_url) = std::env::var("CCOS_LLM_BASE_URL") {
        println!("  • Base URL: {}", base_url.dim());
    }
    println!();
}

fn determine_goal(args: &Args) -> DemoResult<String> {
    if let Some(goal) = args
        .goal
        .as_ref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        return Ok(goal.to_string());
    }

    if let Ok(from_env) = std::env::var("SMART_ASSISTANT_GOAL") {
        let trimmed = from_env.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed.to_string());
        }
    }

    print!("{} ", "What goal should we tackle today?".bold());
    io::stdout().flush()?;
    let mut buffer = String::new();
    io::stdin().read_line(&mut buffer)?;
    let goal = buffer.trim();
    if goal.is_empty() {
        Err(io::Error::new(io::ErrorKind::InvalidInput, "Goal cannot be empty").into())
    } else {
        Ok(goal.to_string())
    }
}

fn load_agent_config(path: &str) -> DemoResult<AgentConfig> {
    let raw = fs::read_to_string(path)?;
    let ext = Path::new(path)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if ext == "json" {
        Ok(serde_json::from_str(&raw)?)
    } else {
        Ok(toml::from_str(&raw)?)
    }
}

fn apply_llm_profile(config: &AgentConfig, profile_name: Option<&str>) -> DemoResult<()> {
    std::env::set_var("CCOS_ENABLE_DELEGATION", "1");

    if let Some(llm_profiles) = &config.llm_profiles {
        let (profiles, _meta, _why) = expand_profiles(config);
        let chosen = profile_name
            .map(|s| s.to_string())
            .or_else(|| llm_profiles.default.clone())
            .or_else(|| profiles.first().map(|p| p.name.clone()));

        if let Some(name) = chosen {
            if let Some(profile) = profiles.iter().find(|p| p.name == name) {
                apply_profile_env(profile);
            }
        } else if let Some(first) = profiles.first() {
            apply_profile_env(first);
        }
    }

    Ok(())
}

fn apply_profile_env(profile: &LlmProfile) {
    std::env::set_var("CCOS_DELEGATING_MODEL", &profile.model);
    std::env::set_var("CCOS_LLM_MODEL", &profile.model);
    std::env::set_var("CCOS_LLM_PROVIDER_HINT", &profile.provider);

    if let Some(url) = &profile.base_url {
        std::env::set_var("CCOS_LLM_BASE_URL", url);
    } else if profile.provider == "openrouter" {
        if std::env::var("CCOS_LLM_BASE_URL").is_err() {
            std::env::set_var("CCOS_LLM_BASE_URL", "https://openrouter.ai/api/v1");
        }
    }

    if let Some(api_key) = profile.api_key.as_ref() {
        set_api_key(&profile.provider, api_key);
    } else if let Some(env) = &profile.api_key_env {
        if let Ok(value) = std::env::var(env) {
            set_api_key(&profile.provider, &value);
        }
    }

    match profile.provider.as_str() {
        "openai" => std::env::set_var("CCOS_LLM_PROVIDER", "openai"),
        "claude" | "anthropic" => std::env::set_var("CCOS_LLM_PROVIDER", "anthropic"),
        "openrouter" => {
            std::env::set_var("CCOS_LLM_PROVIDER", "openrouter");
            // Ensure base URL is set for OpenRouter
            if std::env::var("CCOS_LLM_BASE_URL").is_err() {
                std::env::set_var("CCOS_LLM_BASE_URL", "https://openrouter.ai/api/v1");
            }
        },
        "local" => std::env::set_var("CCOS_LLM_PROVIDER", "local"),
        "stub" => {
            eprintln!("⚠️  WARNING: Using stub LLM provider (testing only - not realistic)");
            eprintln!("   Set a real provider in agent_config.toml or use --profile with a real provider");
            std::env::set_var("CCOS_LLM_PROVIDER", "stub");
            std::env::set_var("CCOS_ALLOW_STUB_PROVIDER", "1"); // Allow stub if explicitly requested
        },
        other => std::env::set_var("CCOS_LLM_PROVIDER", other),
    }
}

fn set_api_key(provider: &str, key: &str) {
    match provider {
        "openrouter" => std::env::set_var("OPENROUTER_API_KEY", key),
        "claude" | "anthropic" => std::env::set_var("ANTHROPIC_API_KEY", key),
        "gemini" => std::env::set_var("GEMINI_API_KEY", key),
        "stub" => {}
        _ => std::env::set_var("OPENAI_API_KEY", key),
    }
}

fn print_intent_summary(intent: &Intent) {
    println!("\n{}", "🧠 Intent summary".bold());
    println!("   • {}", intent.goal.as_str().cyan());
    if !intent.constraints.is_empty() {
        println!("{}", "   • Constraints:".dim());
        for (k, v) in &intent.constraints {
            println!("     - {} = {}", k.as_str().cyan(), format_value(v).dim());
        }
    }
    if !intent.preferences.is_empty() {
        println!("{}", "   • Preferences:".dim());
        for (k, v) in &intent.preferences {
            println!("     - {} = {}", k.as_str().cyan(), format_value(v).dim());
        }
    }
    if let Some(success) = &intent.success_criteria {
        println!("   • Success criteria: {}", format_value(success).dim());
    }
}

async fn generate_clarifying_questions(
    delegating: &DelegatingArbiter,
    goal: &str,
    intent: &Intent,
    debug: bool,
) -> DemoResult<Vec<ClarifyingQuestion>> {
    let mut prompt = String::with_capacity(2048);
    prompt.push_str("You are the CCOS delegating arbiter refining a user goal.\n");
    prompt.push_str("You MUST respond ONLY with an RTFS vector of maps, no prose.\n");
    prompt.push_str("Each map should describe one clarifying question with keys:\n");
    prompt.push_str(
		"  :id :key :prompt :rationale :answer-kind (:text|:list|:number|:boolean) :required (:true/:false) and optional :default-answer.\n",
	);
    prompt
        .push_str("Always include rationale so governance can audit why the question is needed.\n");
    prompt.push_str("The vector MUST contain at least two items if follow-up info is useful.\n");
    prompt.push_str("--- Context ---\n");
    prompt.push_str(&format!("Goal: {}\n", goal));
    if !intent.constraints.is_empty() {
        prompt.push_str("Constraints:\n");
        for (k, v) in &intent.constraints {
            prompt.push_str(&format!("  {} = {}\n", k, format_value(v)));
        }
    }
    if !intent.preferences.is_empty() {
        prompt.push_str("Preferences:\n");
        for (k, v) in &intent.preferences {
            prompt.push_str(&format!("  {} = {}\n", k, format_value(v)));
        }
    }
    if let Some(success) = &intent.success_criteria {
        prompt.push_str(&format!("Success criteria: {}\n", format_value(success)));
    }
    prompt.push_str("----------------\nRespond only with an RTFS vector.");

    let response = delegating
        .generate_raw_text(&prompt)
        .await
        .map_err(runtime_error)?;
    if debug {
        println!(
            "{}\n{}\n{}",
            "┌─ Clarifying question response ───────────────".dim(),
            response,
            "└─────────────────────────────────────────────".dim()
        );
    }

    let parsed_value = parse_clarifying_response(&response).map_err(runtime_error)?;
    let items = extract_question_items(&parsed_value).ok_or_else(|| {
        runtime_error(RuntimeError::Generic(
            "Clarifying question response did not contain any recognizable question list".into(),
        ))
    })?;

    let mut questions = Vec::with_capacity(items.len());
    for (index, item) in items.into_iter().enumerate() {
        if let Some(question) = value_to_question(&item) {
            questions.push(question);
        } else if let Some(question) = question_from_free_form(&item, index) {
            questions.push(question);
        }
    }

    if questions.is_empty() {
        Err(
            RuntimeError::Generic("No clarifying questions parsed from response".to_string())
                .into(),
        )
    } else {
        Ok(questions)
    }
}

fn extract_question_items(value: &Value) -> Option<Vec<Value>> {
    match value {
        Value::Vector(items) | Value::List(items) => Some(items.clone()),
        Value::Map(map) => {
            let keys = [
                "questions",
                "clarifying-questions",
                "clarifying_questions",
                "clarifications",
                "items",
            ];
            for key in keys {
                if let Some(nested) = map_get(map, key) {
                    if let Some(vec) = extract_question_items(nested) {
                        return Some(vec);
                    }
                }
            }
            None
        }
        Value::String(text) => {
            let mut collected = Vec::new();
            for part in text
                .split(|c| c == '\n' || c == ';')
                .map(|segment| segment.trim())
                .filter(|segment| !segment.is_empty())
            {
                let cleaned = part
                    .trim_start_matches(|c: char| c == '-' || c == '*' || c == '•')
                    .trim();
                if !cleaned.is_empty() {
                    collected.push(Value::String(cleaned.to_string()));
                }
            }
            if collected.is_empty() {
                None
            } else {
                Some(collected)
            }
        }
        _ => None,
    }
}

fn parse_clarifying_response(response: &str) -> Result<Value, RuntimeError> {
    let sanitized = strip_code_fences(response);
    // Use comma-stripped form only for RTFS parsing; preserve original for JSON
    let normalized_for_rtfs = strip_commas_outside_strings(&sanitized);
    match parse_expression(&normalized_for_rtfs) {
        Ok(expr) => Ok(expression_to_value(&expr)),
        Err(rtfs_err) => match serde_json::from_str::<serde_json::Value>(&sanitized) {
            Ok(json) => Ok(json_to_demo_value(&json)),
            Err(json_err) => Err(RuntimeError::Generic(format!(
                "Failed to parse clarifying questions via RTFS ({:?}) or JSON ({}).",
                rtfs_err, json_err
            ))),
        },
    }
}

fn json_to_demo_value(value: &serde_json::Value) -> Value {
    match value {
        serde_json::Value::Null => Value::Nil,
        serde_json::Value::Bool(b) => Value::Boolean(*b),
        serde_json::Value::Number(num) => {
            if let Some(i) = num.as_i64() {
                Value::Integer(i)
            } else if let Some(f) = num.as_f64() {
                Value::Float(f)
            } else {
                Value::Nil
            }
        }
        serde_json::Value::String(s) => json_string_to_value(s),
        serde_json::Value::Array(items) => {
            let vec = items.iter().map(json_to_demo_value).collect();
            Value::Vector(vec)
        }
        serde_json::Value::Object(map) => {
            let mut rtfs_map = HashMap::with_capacity(map.len());
            for (key, val) in map {
                rtfs_map.insert(json_key_to_map_key(key), json_to_demo_value(val));
            }
            Value::Map(rtfs_map)
        }
    }
}

fn json_key_to_map_key(key: &str) -> MapKey {
    if let Some(stripped) = key.trim().strip_prefix(':') {
        MapKey::Keyword(Keyword(stripped.to_string()))
    } else {
        MapKey::String(key.trim().to_string())
    }
}

fn json_string_to_value(raw: &str) -> Value {
    let trimmed = raw.trim();
    if let Some(stripped) = trimmed.strip_prefix(':') {
        if stripped.is_empty() {
            Value::String(trimmed.to_string())
        } else {
            Value::Keyword(Keyword(stripped.to_string()))
        }
    } else {
        Value::String(trimmed.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_json_clarifying_questions() {
        let response = r#"
        [
          {
            ":id": "duration-clarify",
            ":key": "duration",
            ":prompt": "How long should the trip last?",
            ":rationale": "Trip length impacts itinerary breadth",
            ":answer-kind": ":text",
            ":required": true
          }
        ]
        "#;

        let value = parse_clarifying_response(response).expect("parse JSON clarifying questions");
        let items = match value {
            Value::Vector(items) => items,
            other => panic!("expected vector, got {:?}", other),
        };
        assert_eq!(items.len(), 1);
        let question = value_to_question(&items[0]).expect("extract clarifying question");
        assert_eq!(question.id, "duration-clarify");
        assert_eq!(question.key, "duration");
        assert_eq!(question.prompt, "How long should the trip last?");
        assert_eq!(question.rationale, "Trip length impacts itinerary breadth");
        assert_eq!(question.answer_kind, AnswerKind::Text);
        assert!(question.required);
    }

    #[test]
    fn parses_json_plan_steps() {
        let response = r#"
        [
          {
            ":id": "search-flights",
            ":name": "Search flights",
            ":capability-class": "travel.flights.search",
            ":required-inputs": ["origin", "destination", "dates", "party_size"],
            ":expected-outputs": ["flight_options"]
          }
        ]
        "#;

        let value = parse_plan_steps_response(response).expect("parse JSON plan steps");
        let items = match value {
            Value::Vector(items) => items,
            other => panic!("expected vector, got {:?}", other),
        };
        assert_eq!(items.len(), 1);
        let step = value_to_step(&items[0]).expect("extract plan step");
        assert_eq!(step.capability_class, "travel.flights.search");
        assert_eq!(
            step.required_inputs,
            vec![
                "origin".to_string(),
                "destination".to_string(),
                "dates".to_string(),
                "party_size".to_string()
            ]
        );
        assert_eq!(step.expected_outputs, vec!["flight_options".to_string()]);
    }

    #[test]
    fn ignores_question_like_freeform_steps() {
        let question = Value::String("What is your budget?".to_string());
        assert!(step_from_free_form(&question, 0).is_none());

        let statement = Value::String("Book lodging".to_string());
        let maybe_step = step_from_free_form(&statement, 0).expect("derive step");
        assert_eq!(maybe_step.capability_class, "freeform.book.lodging");
    }

    #[test]
    fn extracts_questions_from_string_block() {
        let block = Value::String("- Budget?\n- Dates?\nActivities".to_string());
        let items = extract_question_items(&block).expect("string block yields items");
        assert_eq!(items.len(), 3);
    }

    #[test]
    fn extracts_questions_from_map_alias() {
        let mut inner = HashMap::new();
        inner.insert(
            MapKey::Keyword(Keyword("clarifying-questions".to_string())),
            Value::Vector(vec![Value::String("Budget?".into())]),
        );
        let map_value = Value::Map(inner);
        let items = extract_question_items(&map_value).expect("map alias yields items");
        assert_eq!(items.len(), 1);
    }

    #[test]
    fn strips_markdown_code_fence_blocks() {
        let raw = "```clojure\n[:step]\n```";
        let stripped = strip_code_fences(raw);
        assert_eq!(stripped, "[:step]");
    }

    #[test]
    fn generates_rtfs_with_schemas_and_no_dollar_vars() {
        // Build two dummy steps
        let step1 = ResolvedStep {
            original: ProposedStep {
                id: "s1".to_string(),
                name: "Search flights".to_string(),
                capability_class: "travel.flights.search".to_string(),
                candidate_capabilities: vec![],
                required_inputs: vec!["origin".into(), "destination".into(), "dates".into()],
                expected_outputs: vec!["flight_options".into()],
                description: None,
            },
            capability_id: "travel.flights.search".to_string(),
            resolution_strategy: ResolutionStrategy::Found,
        };

        let step2 = ResolvedStep {
            original: ProposedStep {
                id: "s2".to_string(),
                name: "Reserve lodging".to_string(),
                capability_class: "travel.lodging.reserve".to_string(),
                candidate_capabilities: vec![],
                required_inputs: vec!["destination".into(), "dates".into(), "budget".into()],
                expected_outputs: vec!["reservation".into()],
                description: None,
            },
            capability_id: "travel.lodging.reserve".to_string(),
            resolution_strategy: ResolutionStrategy::Found,
        };

        let rtfs = generate_orchestrator_capability(
            "Book trip",
            &[step1, step2],
        )
        .expect("rtfs generation");

    // Must contain schemas
        assert!(rtfs.contains(":input-schema"), "missing input-schema: {}", rtfs);
        assert!(rtfs.contains(":output-schema"), "missing output-schema: {}", rtfs);

        // No legacy $ prefix
        assert!(
            !rtfs.contains(":$"),
            "contains legacy $ variable syntax: {}",
            rtfs
        );

        // Capabilities required vector present with both caps
        assert!(
            rtfs.contains(":capabilities-required [\"travel.flights.search\" \"travel.lodging.reserve\"]")
                || rtfs.contains(":capabilities-required [\"travel.lodging.reserve\" \"travel.flights.search\"]"),
            "capabilities-required vector missing or incomplete: {}",
            rtfs
        );

        // Arguments passed as map
        assert!(rtfs.contains("(call :travel.flights.search {"));
        assert!(rtfs.contains(":origin origin"));
        assert!(rtfs.contains(":destination destination"));
        assert!(rtfs.contains(":dates dates"));

        // Output schema should reflect union of all step outputs
        assert!(rtfs.contains(":flight_options :any"), "output-schema missing flight_options: {}", rtfs);
        assert!(rtfs.contains(":reservation :any"), "output-schema missing reservation: {}", rtfs);

        // Body should bind steps and compose final map using get
        assert!(rtfs.contains("(let ["), "plan should bind step results with let: {}", rtfs);
        assert!(rtfs.contains("(get step_1 :reservation"), "final composition should reference step outputs: {}", rtfs);
    }

    #[test]
    fn wires_inputs_from_previous_step_outputs() {
        // Step 1 produces :prefs
        let s1 = ResolvedStep {
            original: ProposedStep {
                id: "s1".into(),
                name: "Aggregate preferences".into(),
                capability_class: "planning.preferences.aggregate".into(),
                candidate_capabilities: vec![],
                required_inputs: vec!["goal".into()],
                expected_outputs: vec!["prefs".into()],
                description: None,
            },
            capability_id: "planning.preferences.aggregate".into(),
            resolution_strategy: ResolutionStrategy::Found,
        };

        // Step 2 requires :prefs as input
        let s2 = ResolvedStep {
            original: ProposedStep {
                id: "s2".into(),
                name: "Plan activities".into(),
                capability_class: "travel.activities.plan".into(),
                candidate_capabilities: vec![],
                required_inputs: vec!["prefs".into(), "destination".into()],
                expected_outputs: vec!["activity_plan".into()],
                description: None,
            },
            capability_id: "travel.activities.plan".into(),
            resolution_strategy: ResolutionStrategy::Found,
        };

    let rtfs = generate_orchestrator_capability("Trip", &[s1, s2]).expect("generate");

        // Step 2 should wire :prefs from step_0 output; destination remains a free input
        assert!(rtfs.contains(":prefs (get step_0 :prefs)"), "prefs should be wired from previous step: {}", rtfs);
        assert!(rtfs.contains(":destination destination"), "destination should remain a free symbol input: {}", rtfs);

        // Input schema should not require internal-only keys like :prefs (produced by step_0)
        assert!(rtfs.contains(":input-schema"));
        let input_idx = rtfs.find(":input-schema").unwrap();
        let output_idx = rtfs.find(":output-schema").unwrap_or(rtfs.len());
        let input_block = &rtfs[input_idx..output_idx];
        assert!(
            !input_block.contains(":prefs :any"),
            "input-schema should not include internal output keys: {}",
            rtfs
        );
    }
}

fn seed_answers_from_intent(intent: &Intent) -> HashMap<String, AnswerRecord> {
    let mut seeds = HashMap::new();

    for (key, value) in &intent.constraints {
        seeds.insert(
            key.clone(),
            AnswerRecord {
                key: key.clone(),
                text: format_value(value),
                value: value.clone(),
                source: AnswerSource::IntentSeed,
            },
        );
    }

    for (key, value) in &intent.preferences {
        seeds.entry(key.clone()).or_insert(AnswerRecord {
            key: key.clone(),
            text: format_value(value),
            value: value.clone(),
            source: AnswerSource::IntentSeed,
        });
    }

    if let Some(success) = &intent.success_criteria {
        seeds.insert(
            "success_criteria".to_string(),
            AnswerRecord {
                key: "success_criteria".to_string(),
                text: format_value(success),
                value: success.clone(),
                source: AnswerSource::IntentSeed,
            },
        );
    }

    seeds
}

async fn auto_answer_with_llm(
    delegating: &DelegatingArbiter,
    goal: &str,
    intent: &Intent,
    collected_answers: &[AnswerRecord],
    question: &ClarifyingQuestion,
    debug: bool,
) -> DemoResult<AnswerRecord> {
    let mut prompt = String::new();
    prompt.push_str("You are answering clarifying questions for a smart assistant based on a user's goal.\n");
    prompt.push_str("Respond with ONLY the answer value, no explanation or context.\n");
    prompt.push_str("Do NOT use code fences, quotes, or any special formatting.\n");
    prompt.push_str("\nGoal: ");
    prompt.push_str(goal);
    
    if !intent.constraints.is_empty() {
        prompt.push_str("\n\nKnown constraints:");
        for (k, v) in &intent.constraints {
            prompt.push_str(&format!("\n  {} = {}", k, format_value(v)));
        }
    }
    
    if !intent.preferences.is_empty() {
        prompt.push_str("\n\nKnown preferences:");
        for (k, v) in &intent.preferences {
            prompt.push_str(&format!("\n  {} = {}", k, format_value(v)));
        }
    }
    
    if !collected_answers.is_empty() {
        prompt.push_str("\n\nPreviously answered questions:");
        for answer in collected_answers {
            prompt.push_str(&format!("\n  {} = {}", answer.key, answer.text));
        }
    }
    
    prompt.push_str("\n\nCurrent question: ");
    prompt.push_str(&question.prompt);
    prompt.push_str("\nRationale: ");
    prompt.push_str(&question.rationale);
    prompt.push_str("\nAnswer kind: ");
    prompt.push_str(match question.answer_kind {
        AnswerKind::Text => "text",
        AnswerKind::List => "list",
        AnswerKind::Number => "number",
        AnswerKind::Boolean => "boolean",
    });
    if let Some(default) = &question.default_answer {
        prompt.push_str(&format!("\nDefault value: {}", default));
    }
    prompt.push_str("\n\nAnswer: ");

    let response = delegating
        .generate_raw_text(&prompt)
        .await
        .map_err(runtime_error)?;

    if debug {
        println!(
            "{}\n{}\n{}",
            "┌─ Auto-answer response ──────────────────────".dim(),
            response,
            "└─────────────────────────────────────────────".dim()
        );
    }

    // Strip any code fences or extra formatting
    let cleaned = response
        .lines()
        .filter(|line| !line.trim().starts_with("```"))
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string();

    let answer_text = if cleaned.is_empty() {
        question
            .default_answer
            .clone()
            .unwrap_or_else(|| "unknown".to_string())
    } else {
        cleaned
    };

    println!("   → {}", answer_text.as_str().green());

    let answer_value = parse_answer_value(question.answer_kind, &answer_text);
    Ok(AnswerRecord {
        key: question.key.clone(),
        text: answer_text.clone(),
        value: answer_value,
        source: AnswerSource::DelegatingAsk,
    })
}

async fn conduct_interview(
    _ccos: &Arc<CCOS>,
    delegating: &DelegatingArbiter,
    goal: &str,
    intent: &Intent,
    questions: &[ClarifyingQuestion],
    seeded_answers: &mut HashMap<String, AnswerRecord>,
    debug: bool,
    interactive: bool,
) -> DemoResult<Vec<AnswerRecord>> {
    let mut collected = Vec::with_capacity(questions.len());

    for question in questions {
        if let Some(seed) = seeded_answers.remove(question.key.as_str()) {
            collected.push(seed);
            continue;
        }

        if let Ok(env_value) = std::env::var(format!(
            "SMART_ASSISTANT_{}",
            question.key.to_ascii_uppercase()
        )) {
            if !env_value.trim().is_empty() {
                collected.push(AnswerRecord {
                    key: question.key.clone(),
                    text: env_value.clone(),
                    value: parse_answer_value(question.answer_kind, env_value.trim()),
                    source: AnswerSource::EnvFallback,
                });
                continue;
            }
        }

        // Auto-answer with LLM if not in interactive mode
        if !interactive {
            println!("\n{}", "❓ Auto-answering clarifying question".bold());
            println!("{}", question.prompt.as_str().cyan());
            
            let answer = auto_answer_with_llm(delegating, goal, intent, &collected, question, debug).await?;
            collected.push(answer);
            continue;
        }

        println!("\n{}", "❓ Clarifying question".bold());
        println!("{} {}", "   id:".dim(), question.id.as_str().dim());
        println!("{}", question.prompt.as_str().cyan());
        println!(
            "{} {}",
            "   rationale:".dim(),
            question.rationale.as_str().dim()
        );
        if let Some(default) = &question.default_answer {
            println!("{} {}", "   default:".dim(), default.as_str().dim());
        }
        print!("{} ", "→".bold());
        io::stdout().flush()?;
        let mut buffer = String::new();
        io::stdin().read_line(&mut buffer)?;
        let user_input = buffer.trim();

        if user_input.is_empty() {
            if let Some(default) = &question.default_answer {
                collected.push(AnswerRecord {
                    key: question.key.clone(),
                    text: default.clone(),
                    value: parse_answer_value(question.answer_kind, default),
                    source: AnswerSource::Default,
                });
                continue;
            } else if question.required {
                println!(
                    "{}",
                    "   ↳ This answer is required; please provide a response.".red()
                );
                continue;
            } else {
                continue;
            }
        }

        collected.push(AnswerRecord {
            key: question.key.clone(),
            text: user_input.to_string(),
            value: parse_answer_value(question.answer_kind, user_input),
            source: AnswerSource::DelegatingAsk,
        });
    }

    for (_, seed) in seeded_answers.drain() {
        collected.push(seed);
    }

    Ok(collected)
}

async fn propose_plan_steps(
    delegating: &DelegatingArbiter,
    goal: &str,
    intent: &Intent,
    answers: &[AnswerRecord],
    capabilities: &[StubCapabilitySpec],
    debug: bool,
) -> DemoResult<Vec<ProposedStep>> {
    let mut prompt = String::with_capacity(4096);
    prompt.push_str("You are the delegating arbiter drafting an RTFS plan skeleton.\n");
    prompt.push_str("Respond ONLY with an RTFS vector where each element is a map describing a proposed capability step.\n");
    prompt.push_str(
		"Each map must include :id :name :capability-class :required-inputs (vector of strings) :expected-outputs (vector of strings) and optional :candidate-capabilities (vector of capability ids) :description.\n",
	);
    prompt.push_str("Focus on sequencing capabilities from the marketplace context below.\n");
    prompt.push_str("--- Goal & intent ---\n");
    prompt.push_str(&format!("Goal: {}\n", goal));
    if !intent.constraints.is_empty() {
        prompt.push_str("Constraints:\n");
        for (k, v) in &intent.constraints {
            prompt.push_str(&format!("  {} = {}\n", k, format_value(v)));
        }
    }
    if !intent.preferences.is_empty() {
        prompt.push_str("Preferences:\n");
        for (k, v) in &intent.preferences {
            prompt.push_str(&format!("  {} = {}\n", k, format_value(v)));
        }
    }
    if !answers.is_empty() {
        prompt.push_str("--- Clarified parameters ---\n");
        for answer in answers {
            prompt.push_str(&format!(
                "  {} = {} ({:?}, value = {})\n",
                answer.key,
                answer.text,
                answer.source,
                format_value(&answer.value)
            ));
        }
    }
    if !capabilities.is_empty() {
        prompt.push_str("--- Capability marketplace snapshot ---\n");
        for spec in capabilities {
            prompt.push_str(&format!(
                "  {} -> {} (inputs: [{}], outputs: [{}])\n",
                spec.id,
                spec.description,
                spec.required_inputs.join(", "),
                spec.expected_outputs.join(", ")
            ));
        }
    }
    prompt.push_str("----------------\nRespond only with the RTFS vector of step maps.");

    if debug {
        println!(
            "\n{}\n{}\n{}",
            "┌─ Plan generation prompt ───────────────────".dim(),
            prompt,
            "└────────────────────────────────────────────".dim()
        );
    } else {
        // Show a summary even if debug is off
        println!("  📝 Sending plan generation request to LLM...");
        let prompt_lines: Vec<&str> = prompt.lines().collect();
        if prompt_lines.len() > 10 {
            println!("    Prompt length: {} lines", prompt_lines.len());
            println!("    Goal: {}", goal);
            if !intent.constraints.is_empty() {
                println!("    Constraints: {}", intent.constraints.len());
            }
            if !answers.is_empty() {
                println!("    Clarified answers: {}", answers.len());
            }
        }
    }

    let response = delegating
        .generate_raw_text(&prompt)
        .await
        .map_err(runtime_error)?;
    
    if debug {
        println!(
            "\n{}\n{}\n{}",
            "┌─ LLM plan generation response ─────────────".dim(),
            response,
            "└────────────────────────────────────────────".dim()
        );
    } else {
        // Show a summary of the response
        let response_preview = if response.len() > 200 {
            format!("{}...", &response[..200])
        } else {
            response.clone()
        };
        println!("  📨 LLM response received ({} chars)", response.len());
        if response_preview.len() < response.len() {
            println!("    Preview: {}", response_preview.replace('\n', " "));
        }
    }

    let mut parsed_value = parse_plan_steps_response(&response).map_err(runtime_error)?;
    if let Value::Map(map) = &parsed_value {
        if let Some(Value::Vector(steps)) = map_get(map, "steps") {
            parsed_value = Value::Vector(steps.clone());
        }
    }

    match parsed_value {
        Value::Vector(items) => {
            let mut steps = Vec::with_capacity(items.len());
            for (index, item) in items.into_iter().enumerate() {
                if let Some(step) = value_to_step(&item) {
                    steps.push(step);
                } else if let Some(step) = step_from_free_form(&item, index) {
                    steps.push(step);
                }
            }
            if steps.is_empty() {
                Err(
                    RuntimeError::Generic("No steps parsed from arbiter response".to_string())
                        .into(),
                )
            } else {
                if !debug {
                    println!("  🔍 Parsed {} plan step(s) from LLM response:", steps.len());
                    for (i, step) in steps.iter().enumerate() {
                        println!("    {}. {} ({})", i + 1, step.name, step.capability_class);
                    }
                }
                Ok(steps)
            }
        }
        other => Err(RuntimeError::Generic(format!(
            "Plan step response was not a vector: {}",
            format_value(&other)
        ))
        .into()),
    }
}

fn parse_plan_steps_response(response: &str) -> Result<Value, RuntimeError> {
    let sanitized = strip_code_fences(response);
    let normalized_for_rtfs = strip_commas_outside_strings(&sanitized);
    match parse_expression(&normalized_for_rtfs) {
        Ok(expr) => Ok(expression_to_value(&expr)),
        Err(rtfs_err) => match serde_json::from_str::<serde_json::Value>(&sanitized) {
            Ok(json) => Ok(json_to_demo_value(&json)),
            Err(json_err) => Err(RuntimeError::Generic(format!(
                "Failed to parse plan steps via RTFS ({:?}) or JSON ({}).",
                rtfs_err, json_err
            ))),
        },
    }
}

fn strip_code_fences(raw: &str) -> String {
    let trimmed = raw.trim();
    if !trimmed.starts_with("```") {
        return trimmed.to_string();
    }

    let mut lines = trimmed.lines();
    let first = lines.next().unwrap_or("");
    if !first.starts_with("```") {
        return trimmed.to_string();
    }

    let mut body: Vec<&str> = lines.collect();
    while let Some(last) = body.last() {
        if last.trim().starts_with("```") {
            body.pop();
        } else {
            break;
        }
    }

    body.join("\n").trim().to_string()
}

fn strip_commas_outside_strings(raw: &str) -> String {
    let mut result = String::with_capacity(raw.len());
    let mut in_string = false;
    let mut escape = false;

    for ch in raw.chars() {
        if in_string {
            if escape {
                escape = false;
            } else if ch == '\\' {
                escape = true;
            } else if ch == '"' {
                in_string = false;
            }
            result.push(ch);
            continue;
        }

        match ch {
            '"' => {
                in_string = true;
                result.push(ch);
            }
            ',' => {
                // drop commas outside strings so JSON-style listings become RTFS-friendly
            }
            _ => result.push(ch),
        }
    }

    result
}

fn literal_to_value(literal: &Literal) -> Value {
    use Literal::*;
    match literal {
        Integer(i) => Value::Integer(*i),
        Float(f) => Value::Float(*f),
        String(s) => Value::String(s.clone()),
        Boolean(b) => Value::Boolean(*b),
        Keyword(k) => Value::Keyword(k.clone()),
        Symbol(s) => Value::Symbol(s.clone()),
        Timestamp(ts) => Value::Timestamp(ts.clone()),
        Uuid(id) => Value::Uuid(id.clone()),
        ResourceHandle(handle) => Value::ResourceHandle(handle.clone()),
        Nil => Value::Nil,
    }
}

fn expression_to_value(expr: &Expression) -> Value {
    match expr {
        Expression::Literal(lit) => literal_to_value(lit),
        Expression::Symbol(sym) => Value::Symbol(sym.clone()),
        Expression::Vector(items) => {
            let vec = items.iter().map(expression_to_value).collect();
            Value::Vector(vec)
        }
        Expression::List(items) => {
            let list = items.iter().map(expression_to_value).collect();
            Value::List(list)
        }
        Expression::Map(entries) => {
            let mut map = HashMap::new();
            for (key, value) in entries {
                map.insert(key.clone(), expression_to_value(value));
            }
            Value::Map(map)
        }
        Expression::Do(do_expr) => {
            let list = do_expr
                .expressions
                .iter()
                .map(expression_to_value)
                .collect();
            Value::List(list)
        }
        _ => Value::Nil,
    }
}

fn value_to_question(value: &Value) -> Option<ClarifyingQuestion> {
    let Value::Map(map) = value else {
        return None;
    };
    let id = map_get(map, "id").and_then(value_to_string)?;
    let key = map_get(map, "key")
        .and_then(value_to_string)
        .unwrap_or_else(|| id.clone());
    let prompt = map_get(map, "prompt")
        .or_else(|| map_get(map, "question"))
        .and_then(value_to_string)?;
    let rationale = map_get(map, "rationale")
        .and_then(value_to_string)
        .unwrap_or_else(|| "Clarifies scope for planning".to_string());
    let answer_kind = map_get(map, "answer-kind")
        .or_else(|| map_get(map, "answer_kind"))
        .and_then(|value| value_to_string(value))
        .and_then(|kind| parse_answer_kind(&kind))
        .unwrap_or(AnswerKind::Text);
    let required = map_get(map, "required")
        .or_else(|| map_get(map, "is-required"))
        .and_then(value_to_bool)
        .unwrap_or(false);
    let default_answer = map_get(map, "default-answer")
        .or_else(|| map_get(map, "default"))
        .and_then(value_to_string);

    Some(ClarifyingQuestion {
        id,
        key,
        prompt,
        rationale,
        answer_kind,
        required,
        default_answer,
    })
}

fn question_from_free_form(value: &Value, index: usize) -> Option<ClarifyingQuestion> {
    let raw = match value {
        Value::String(s) if !s.trim().is_empty() => s.trim(),
        Value::Keyword(k) if !k.0.trim().is_empty() => k.0.trim(),
        Value::Symbol(sym) if !sym.0.trim().is_empty() => sym.0.trim(),
        _ => return None,
    };

    let prompt = ensure_question_prompt(raw, index);
    let base_slug = slugify(raw);
    let key = if base_slug.is_empty() {
        format!("clarify_{}", index + 1)
    } else {
        base_slug.replace('-', "_")
    };
    let id = slugify_with_prefix(raw, "clarify", index);
    let rationale = "Generated from arbiter free-form clarifying prompt".to_string();
    let answer_kind = infer_answer_kind_from_prompt(&prompt);

    Some(ClarifyingQuestion {
        id,
        key,
        prompt,
        rationale,
        answer_kind,
        required: true,
        default_answer: None,
    })
}

fn value_to_step(value: &Value) -> Option<ProposedStep> {
    let Value::Map(map) = value else {
        return None;
    };
    let id = map_get(map, "id").and_then(value_to_string)?;
    let name = map_get(map, "name")
        .and_then(value_to_string)
        .unwrap_or_else(|| id.clone());
    let capability_class = map_get(map, "capability-class")
        .or_else(|| map_get(map, "class"))
        .and_then(value_to_string)?;
    let required_inputs = map_get(map, "required-inputs")
        .or_else(|| map_get(map, "inputs"))
        .and_then(value_to_string_vec)
        .unwrap_or_default();
    let expected_outputs = map_get(map, "expected-outputs")
        .or_else(|| map_get(map, "outputs"))
        .and_then(value_to_string_vec)
        .unwrap_or_default();
    if required_inputs.is_empty() || expected_outputs.is_empty() {
        return None;
    }
    let candidate_capabilities = map_get(map, "candidate-capabilities")
        .or_else(|| map_get(map, "candidates"))
        .and_then(value_to_string_vec)
        .unwrap_or_default();
    let description = map_get(map, "description").and_then(value_to_string);

    Some(ProposedStep {
        id,
        name,
        capability_class,
        candidate_capabilities,
        required_inputs,
        expected_outputs,
        description,
    })
}

fn step_from_free_form(value: &Value, index: usize) -> Option<ProposedStep> {
    let raw = match value {
        Value::String(s) if !s.trim().is_empty() => s.trim(),
        Value::Keyword(k) if !k.0.trim().is_empty() => k.0.trim(),
        Value::Symbol(sym) if !sym.0.trim().is_empty() => sym.0.trim(),
        _ => return None,
    };

    let cleaned = raw
        .trim_start_matches(|c: char| c == '-' || c == '*' || c == '•')
        .trim();
    if cleaned.is_empty() {
        return None;
    }

    let lower = cleaned.to_ascii_lowercase();
    if cleaned.ends_with('?')
        || lower.starts_with("what ")
        || lower.starts_with("which ")
        || lower.starts_with("who ")
        || lower.starts_with("where ")
        || lower.starts_with("why ")
        || lower.starts_with("how ")
    {
        return None;
    }

    let id = slugify_with_prefix(cleaned, "step", index);
    let class_slug = slugify(cleaned);
    let capability_class = if class_slug.is_empty() {
        format!("freeform.step.{}", index + 1)
    } else {
        format!("freeform.{}", class_slug.replace('-', "."))
    };
    let name = cleaned
        .trim_end_matches(|c: char| c == '.' || c == '?')
        .trim()
        .to_string();

    Some(ProposedStep {
        id,
        name: if name.is_empty() {
            format!("Freeform Step {}", index + 1)
        } else {
            name
        },
        capability_class,
        candidate_capabilities: Vec::new(),
        required_inputs: vec!["goal".to_string()],
        expected_outputs: vec!["notes".to_string()],
        description: Some(cleaned.to_string()),
    })
}

fn ensure_question_prompt(text: &str, index: usize) -> String {
    let trimmed = text
        .trim_start_matches(|c: char| c == '-' || c == '*' || c == '•')
        .trim();
    if trimmed.is_empty() {
        return format!("Could you clarify detail number {}?", index + 1);
    }
    let core = trimmed
        .trim_end_matches(|c: char| c == '.' || c == '!')
        .trim();
    if core.ends_with('?') {
        core.to_string()
    } else {
        format!("{}?", core)
    }
}

fn infer_answer_kind_from_prompt(prompt: &str) -> AnswerKind {
    let lower = prompt.to_ascii_lowercase();
    if lower.contains("yes or no")
        || lower.starts_with("do you")
        || lower.starts_with("is ")
        || lower.starts_with("are ")
    {
        AnswerKind::Boolean
    } else if lower.contains("how many")
        || lower.contains("how much")
        || lower.contains("budget")
        || lower.contains("cost")
        || lower.contains("price")
        || lower.contains("amount")
    {
        AnswerKind::Number
    } else if lower.contains("which")
        || lower.contains("what specific")
        || lower.contains("what kind")
        || lower.contains("list")
        || lower.contains("interests")
    {
        AnswerKind::List
    } else {
        AnswerKind::Text
    }
}

fn slugify_with_prefix(text: &str, prefix: &str, index: usize) -> String {
    let slug = slugify(text);
    if slug.is_empty() {
        format!("{}-{}", prefix, index + 1)
    } else {
        format!("{}-{}-{}", prefix, index + 1, slug)
    }
}

fn slugify(text: &str) -> String {
    let mut slug = String::new();
    let mut last_hyphen = false;
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            last_hyphen = false;
        } else if !last_hyphen {
            slug.push('-');
            last_hyphen = true;
        }
    }
    slug.trim_matches('-').to_string()
}

fn map_get<'a>(map: &'a HashMap<MapKey, Value>, key: &str) -> Option<&'a Value> {
    let normalized = key.trim_matches(':');
    for (map_key, value) in map {
        match map_key {
            MapKey::String(s) if s.eq_ignore_ascii_case(normalized) => return Some(value),
            MapKey::Keyword(k) if k.0.eq_ignore_ascii_case(normalized) => return Some(value),
            MapKey::Integer(i) if i.to_string() == normalized => return Some(value),
            _ => {}
        }
    }
    None
}

fn value_to_string(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Keyword(k) => Some(k.0.clone()),
        Value::Symbol(sym) => Some(sym.0.clone()),
        Value::Integer(i) => Some(i.to_string()),
        Value::Float(f) => Some(f.to_string()),
        Value::Boolean(b) => Some(b.to_string()),
        Value::Timestamp(ts) => Some(ts.clone()),
        Value::Uuid(u) => Some(u.clone()),
        Value::ResourceHandle(h) => Some(h.clone()),
        Value::Vector(vec) => Some(
            vec.iter()
                .filter_map(|v| value_to_string(v))
                .collect::<Vec<_>>()
                .join(", "),
        ),
        _ => None,
    }
}

fn value_to_bool(value: &Value) -> Option<bool> {
    match value {
        Value::Boolean(b) => Some(*b),
        Value::String(s) => match s.trim().to_ascii_lowercase().as_str() {
            "true" | "yes" | "y" | "1" => Some(true),
            "false" | "no" | "n" | "0" => Some(false),
            _ => None,
        },
        Value::Integer(i) => Some(*i != 0),
        Value::Float(f) => Some(*f != 0.0),
        _ => None,
    }
}

fn value_to_string_vec(value: &Value) -> Option<Vec<String>> {
    match value {
        Value::Vector(items) | Value::List(items) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                if let Some(s) = value_to_string(item) {
                    out.push(s);
                }
            }
            Some(out)
        }
        Value::String(s) => Some(
            s.split(',')
                .map(|part| part.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect(),
        ),
        _ => None,
    }
}

fn parse_answer_kind(label: &str) -> Option<AnswerKind> {
    match label.trim().to_ascii_lowercase().as_str() {
        "text" | "string" => Some(AnswerKind::Text),
        "list" | "vector" | "array" => Some(AnswerKind::List),
        "number" | "numeric" | "float" | "int" | "integer" => Some(AnswerKind::Number),
        "bool" | "boolean" => Some(AnswerKind::Boolean),
        _ => None,
    }
}

fn parse_answer_value(kind: AnswerKind, raw: &str) -> Value {
    match kind {
        AnswerKind::Text => Value::String(raw.to_string()),
        AnswerKind::List => {
            let items: Vec<Value> = raw
                .split(|c| c == ',' || c == ';' || c == '\n')
                .map(|part| part.trim())
                .filter(|s| !s.is_empty())
                .map(|s| Value::String(s.to_string()))
                .collect();
            Value::Vector(items)
        }
        AnswerKind::Number => {
            if let Ok(i) = raw.trim().parse::<i64>() {
                Value::Integer(i)
            } else if let Ok(f) = raw.trim().parse::<f64>() {
                Value::Float(f)
            } else {
                Value::String(raw.to_string())
            }
        }
        AnswerKind::Boolean => match raw.trim().to_ascii_lowercase().as_str() {
            "true" | "yes" | "y" | "1" => Value::Boolean(true),
            "false" | "no" | "n" | "0" => Value::Boolean(false),
            _ => Value::String(raw.to_string()),
        },
    }
}

fn format_value(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Integer(i) => i.to_string(),
        Value::Float(f) => format!("{:.2}", f),
        Value::Boolean(b) => b.to_string(),
        Value::Vector(items) => {
            let joined = items
                .iter()
                .map(format_value)
                .collect::<Vec<_>>()
                .join(", ");
            format!("[{}]", joined)
        }
        Value::List(items) => {
            let joined = items
                .iter()
                .map(format_value)
                .collect::<Vec<_>>()
                .join(", ");
            format!("({})", joined)
        }
        Value::Map(map) => {
            let mut entries: Vec<String> = map
                .iter()
                .map(|(k, v)| format!("{}: {}", map_key_to_string(k), format_value(v)))
                .collect();
            entries.sort();
            format!("{{{}}}", entries.join(", "))
        }
        _ => format!("{:?}", value),
    }
}

fn map_key_to_string(key: &MapKey) -> String {
    match key {
        MapKey::String(s) => s.clone(),
        MapKey::Integer(i) => i.to_string(),
        MapKey::Keyword(k) => format!(":{}", k.0),
    }
}

async fn register_stub_capabilities(ccos: &Arc<CCOS>) -> DemoResult<Vec<StubCapabilitySpec>> {
    let specs = stub_capability_specs();
    let marketplace = ccos.get_capability_marketplace();
    let existing = marketplace.list_capabilities().await;
    let existing_ids: HashSet<String> = existing.into_iter().map(|cap| cap.id).collect();

    for spec in &specs {
        if existing_ids.contains(spec.id) {
            continue;
        }

        let id = spec.id.to_string();
        let name = spec.name.to_string();
        let description = spec.description.to_string();
        let handler_id = id.clone();
        let handler = Arc::new(move |_input: &Value| -> RuntimeResult<Value> {
            let mut out = HashMap::new();
            out.insert(
                MapKey::String("status".into()),
                Value::String("ok".to_string()),
            );
            out.insert(
                MapKey::String("capability".into()),
                Value::String(handler_id.clone()),
            );
            out.insert(
                MapKey::String("timestamp".into()),
                Value::String(Utc::now().to_rfc3339()),
            );
            Ok(Value::Map(out))
        });

        if let Err(err) = marketplace
            .register_local_capability(id.clone(), name, description, handler)
            .await
        {
            println!(
                "{} {}",
                "⚠️  Failed to register stub capability:".yellow(),
                err
            );
        }
    }

    Ok(specs)
}

fn stub_capability_specs() -> Vec<StubCapabilitySpec> {
    vec![
        StubCapabilitySpec {
            id: "travel.flights.search",
            name: "Search flights",
            description: "Locates flight options for the selected dates",
            required_inputs: &["origin", "destination", "dates", "party_size"],
            expected_outputs: &["flight_options"],
        },
        StubCapabilitySpec {
            id: "travel.lodging.reserve",
            name: "Reserve lodging",
            description: "Books hotels or rentals in the destination city",
            required_inputs: &["destination", "dates", "budget", "lodging_style"],
            expected_outputs: &["reservation"],
        },
        StubCapabilitySpec {
            id: "travel.activities.plan",
            name: "Plan activities",
            description: "Creates a day-by-day itinerary of activities",
            required_inputs: &["destination", "interests", "dates"],
            expected_outputs: &["activity_plan"],
        },
        StubCapabilitySpec {
            id: "finance.crypto.allocate",
            name: "Allocate crypto portfolio",
            description: "Allocates crypto investments according to risk profile",
            required_inputs: &["budget", "risk_profile", "preferred_assets"],
            expected_outputs: &["allocation_plan"],
        },
        StubCapabilitySpec {
            id: "planning.itinerary.compose",
            name: "Compose itinerary",
            description: "Summarises travel logistics into a single itinerary",
            required_inputs: &["flight_options", "reservation", "activity_plan"],
            expected_outputs: &["itinerary"],
        },
    ]
}

/// Convert a step name to a more functional description for better semantic matching
/// Generic implementation that works for any capability type
fn step_name_to_functional_description(step_name: &str, capability_class: &str) -> String {
    let lower = step_name.to_lowercase();
    let functional_verbs = ["list", "get", "retrieve", "fetch", "search", "find", "create", "update", "delete", "format", "process", "analyze"];
    
    // If step name already contains functional verbs, return as-is
    if functional_verbs.iter().any(|verb| lower.contains(verb)) {
        return step_name.to_string();
    }
    
    // Step name is more like a title, convert to functional form using capability class
    // Extract action from capability class (last segment)
    let parts: Vec<&str> = capability_class.split('.').collect();
    if let Some(action) = parts.last() {
        match *action {
            "list" => format!("List {}", step_name),
            "get" | "retrieve" => format!("Retrieve {}", step_name),
            "create" => format!("Create {}", step_name),
            "update" | "modify" => format!("Update {}", step_name),
            "delete" | "remove" => format!("Delete {}", step_name),
            "search" | "find" => format!("Search for {}", step_name),
            "filter" => format!("Filter {}", step_name),
            _ => format!("{} {}", action, step_name),
        }
    } else {
        format!("Execute: {}", step_name)
    }
}

fn fallback_steps() -> Vec<ProposedStep> {
    vec![
        ProposedStep {
            id: "collect_preferences".to_string(),
            name: "Consolidate preferences".to_string(),
            capability_class: "planning.preferences.aggregate".to_string(),
            candidate_capabilities: vec![],
            required_inputs: vec!["goal".into()],
            expected_outputs: vec!["preferences".into()],
            description: Some("Aggregate clarified inputs".to_string()),
        },
        ProposedStep {
            id: "search_flights".to_string(),
            name: "Search flights".to_string(),
            capability_class: "travel.flights.search".to_string(),
            candidate_capabilities: vec!["travel.flights.search".to_string()],
            required_inputs: vec![
                "origin".into(),
                "destination".into(),
                "dates".into(),
                "party_size".into(),
            ],
            expected_outputs: vec!["flight_options".into()],
            description: Some("Gather flight candidates".to_string()),
        },
        ProposedStep {
            id: "book_lodging".to_string(),
            name: "Book lodging".to_string(),
            capability_class: "travel.lodging.reserve".to_string(),
            candidate_capabilities: vec!["travel.lodging.reserve".to_string()],
            required_inputs: vec!["destination".into(), "dates".into(), "budget".into()],
            expected_outputs: vec!["reservation".into()],
            description: Some("Secure accommodations".to_string()),
        },
        ProposedStep {
            id: "plan_activities".to_string(),
            name: "Plan activities".to_string(),
            capability_class: "travel.activities.plan".to_string(),
            candidate_capabilities: vec!["travel.activities.plan".to_string()],
            required_inputs: vec!["destination".into(), "interests".into(), "dates".into()],
            expected_outputs: vec!["activity_plan".into()],
            description: Some("Outline daily experiences".to_string()),
        },
    ]
}

fn build_needs_capabilities(steps: &[ProposedStep]) -> Value {
    let entries: Vec<Value> = steps
        .iter()
        .map(|step| {
            let mut map = HashMap::new();
            map.insert(
                MapKey::String("class".into()),
                Value::String(step.capability_class.clone()),
            );
            if !step.candidate_capabilities.is_empty() {
                map.insert(
                    MapKey::String("candidates".into()),
                    Value::Vector(
                        step.candidate_capabilities
                            .iter()
                            .map(|id| Value::String(id.clone()))
                            .collect(),
                    ),
                );
            }
            map.insert(
                MapKey::String("required_inputs".into()),
                Value::Vector(
                    step.required_inputs
                        .iter()
                        .map(|k| Value::String(k.clone()))
                        .collect(),
                ),
            );
            map.insert(
                MapKey::String("expected_outputs".into()),
                Value::Vector(
                    step.expected_outputs
                        .iter()
                        .map(|k| Value::String(k.clone()))
                        .collect(),
                ),
            );
            Value::Map(map)
        })
        .collect();
    Value::Vector(entries)
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
struct ResolvedStep {
    original: ProposedStep,
    capability_id: String,
    resolution_strategy: ResolutionStrategy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
enum ResolutionStrategy {
    Found,
    Stubbed,
    Synthesized,
}

/// Build a re-plan prompt with discovery hints
fn build_replan_prompt(
    goal: &str,
    intent: &Intent,
    hints: &DiscoveryHints,
) -> String {
    let mut prompt = String::new();
    prompt.push_str("You are the delegating arbiter drafting an RTFS plan skeleton.\n");
    prompt.push_str("The previous plan requested capabilities that don't exist. Please replan using only available capabilities.\n\n");
    prompt.push_str(&format!("Goal: {}\n\n", goal));
    
    if !intent.constraints.is_empty() {
        prompt.push_str("Constraints:\n");
        for (k, v) in &intent.constraints {
            prompt.push_str(&format!("  {} = {}\n", k, format_value(v)));
        }
        prompt.push_str("\n");
    }
    
    prompt.push_str("Available Capabilities:\n");
    for found_cap in &hints.found_capabilities {
        prompt.push_str(&format!(
            "  * {} ({}) - {}\n",
            found_cap.id, found_cap.provider, found_cap.description
        ));
        if !found_cap.parameters.is_empty() {
            prompt.push_str(&format!(
                "    - Parameters: {}\n",
                found_cap.parameters.join(", ")
            ));
        }
        if !found_cap.hints.is_empty() {
            for hint in &found_cap.hints {
                prompt.push_str(&format!("    - Hint: {}\n", hint));
            }
        }
    }
    
    if !hints.missing_capabilities.is_empty() {
        prompt.push_str("\nMissing Capabilities (not found):\n");
        for missing in &hints.missing_capabilities {
            prompt.push_str(&format!("  * {}\n", missing));
        }
    }
    
    if !hints.suggestions.is_empty() {
        prompt.push_str("\nSuggestions:\n");
        for suggestion in &hints.suggestions {
            prompt.push_str(&format!("  - {}\n", suggestion));
        }
    }
    
    prompt.push_str("\nIMPORTANT: Please generate a new plan that uses ONLY the available capabilities listed above.\n");
    prompt.push_str("If a capability was missing, try to achieve the same goal using the available capabilities and their parameters.\n");
    prompt.push_str("For example, if 'github.issues.list' supports a 'state' parameter (open|closed|all), use it instead of a separate filtering capability.\n\n");
    prompt.push_str("Respond ONLY with an RTFS vector where each element is a map describing a proposed capability step.\n");
    prompt.push_str("Each map must include :id :name :capability-class :required-inputs (vector of strings) :expected-outputs (vector of strings) and optional :candidate-capabilities (vector of capability ids) :description.\n");
    prompt.push_str("When specifying capability calls, use the exact capability IDs from the 'Available Capabilities' section above.\n");
    prompt.push_str("Include parameter values in :required-inputs when they are known (e.g., if filtering is needed, specify the parameter name).\n");
    
    prompt
}

/// Format found capabilities for display
fn format_found_capabilities(found: &[FoundCapability]) -> String {
    let mut result = String::new();
    for cap in found {
        result.push_str(&format!(
            "  * {} ({}) - {}\n",
            cap.id, cap.provider, cap.description
        ));
        if !cap.parameters.is_empty() {
            result.push_str(&format!("    Parameters: {}\n", cap.parameters.join(", ")));
        }
        if !cap.hints.is_empty() {
            for hint in &cap.hints {
                result.push_str(&format!("    - {}\n", hint));
            }
        }
    }
    result
}

/// Resolve missing capabilities by searching marketplace, synthesizing, or creating stubs.
/// Uses recursive synthesis to automatically generate missing capabilities and their dependencies.
async fn resolve_and_stub_capabilities(
    ccos: &Arc<CCOS>,
    steps: &[ProposedStep],
    matches: &[CapabilityMatch],
    interactive: bool,
) -> DemoResult<Vec<ResolvedStep>> {
    let mut resolved = Vec::with_capacity(steps.len());
    let marketplace = ccos.get_capability_marketplace();
    let intent_graph = ccos.get_intent_graph();
    let delegating_arbiter = ccos.get_delegating_arbiter();

    for step in steps {
        // Check if already matched (found in marketplace or synthesized)
        if let Some(match_record) = matches.iter().find(|m| m.step_id == step.id) {
            if let Some(cap_id) = &match_record.matched_capability {
                // Check if it was synthesized based on the note
                let strategy = if match_record.note.as_ref()
                    .map(|n| n.contains("Synthesized"))
                    .unwrap_or(false) {
                    ResolutionStrategy::Synthesized
                } else {
                    ResolutionStrategy::Found
                };
                
                if strategy == ResolutionStrategy::Synthesized {
                    println!(
                        "{} {}",
                        "✅ Synthesized capability:".green(),
                        cap_id.as_str().cyan()
                    );
                }
                
                resolved.push(ResolvedStep {
                    original: step.clone(),
                    capability_id: cap_id.clone(),
                    resolution_strategy: strategy,
                });
                continue;
            }
        }

        // Not found in marketplace - try recursive synthesis
        if delegating_arbiter.is_some() {
            println!(
                "{} {}",
                "🔄 Attempting recursive synthesis for:".cyan(),
                step.capability_class.as_str().bold()
            );

            let capability_class = step.capability_class.clone();
            
            // Generate a more descriptive rationale that will match better with capability descriptions
            // Use step name, description, or construct a functional description from the step
            let rationale = if let Some(ref desc) = step.description {
                // If we have a description, use it (it's already functional)
                desc.clone()
            } else {
                // Otherwise, convert step name to a functional description
                // e.g., "List GitHub Repository Issues" -> "List issues in a GitHub repository"
                // This works better for semantic matching than "Need for step: X"
                let functional_desc = step_name_to_functional_description(&step.name, &capability_class);
                functional_desc
            };
            
            let need = CapabilityNeed::new(
                capability_class.clone(),
                step.required_inputs.clone(),
                step.expected_outputs.clone(),
                rationale,
            );

            let discovery_engine = DiscoveryEngine::new_with_arbiter(
                Arc::clone(&marketplace),
                Arc::clone(&intent_graph),
                delegating_arbiter.clone(),
            );

            match discovery_engine.discover_capability(&need).await {
                Ok(DiscoveryResult::Found(manifest)) => {
                    // Successfully synthesized (or found via discovery)
                    let cap_id = manifest.id.clone();
                    println!(
                        "{} {}",
                        "✅ Synthesized capability:".green(),
                        cap_id.as_str().cyan()
                    );
                    
                    resolved.push(ResolvedStep {
                        original: step.clone(),
                        capability_id: cap_id.clone(),
                        resolution_strategy: ResolutionStrategy::Synthesized,
                    });
                    continue;
                }
                Ok(DiscoveryResult::Incomplete(manifest)) => {
                    // Capability marked as incomplete/not_found
                    let cap_id = manifest.id.clone();
                    println!(
                        "{} {}",
                        "⚠️  Incomplete capability:".yellow().bold(),
                        cap_id.as_str().cyan()
                    );
                    println!(
                        "   {}",
                        "Capability not found in MCP registry or OpenAPI - requires manual implementation".dim()
                    );
                    
                    // Interactive mode: ask user for guidance
                    let user_provided_url = if interactive {
                        prompt_for_capability_url(&step.capability_class, &manifest)
                    } else {
                        None
                    };
                    
                    // If user provided a URL, we could potentially use it
                    // For now, just log it and treat as incomplete
                    if let Some(ref url) = user_provided_url {
                        println!(
                            "   {} {}",
                            "→ User provided URL:".dim(),
                            url.as_str().cyan()
                        );
                        // TODO: Use this URL to attempt introspection
                    }
                    
                    resolved.push(ResolvedStep {
                        original: step.clone(),
                        capability_id: cap_id,
                        resolution_strategy: ResolutionStrategy::Synthesized, // Treat as synthesized for now
                    });
                    continue;
                }
                Ok(DiscoveryResult::NotFound) | Err(_) => {
                    println!(
                        "{} {}",
                        "⚠️  Synthesis failed, falling back to stub:".yellow(),
                        step.capability_class
                    );
                }
            }
        } else {
            println!(
                "{} {}",
                "⚠️  No delegating arbiter available for synthesis, using stub:".yellow(),
                step.capability_class
            );
        }

        // Fallback: create a stub capability if synthesis failed or no arbiter
        let stub_id = format!("stub.{}.v1", step.capability_class);
        register_stub_capability(ccos, step, &stub_id).await?;

        resolved.push(ResolvedStep {
            original: step.clone(),
            capability_id: stub_id,
            resolution_strategy: ResolutionStrategy::Stubbed,
        });
    }

    Ok(resolved)
}

/// Register a temporary stub capability that holds a placeholder for real capability.
#[allow(dead_code)]
async fn register_stub_capability(
    ccos: &Arc<CCOS>,
    step: &ProposedStep,
    stub_id: &str,
) -> DemoResult<()> {
    let marketplace = ccos.get_capability_marketplace();
    let step_copy = step.clone();

    let handler = Arc::new(move |_inputs: &Value| {
        let mut out_map = HashMap::new();
        for output_key in &step_copy.expected_outputs {
            out_map.insert(
                MapKey::String(output_key.clone()),
                Value::String(format!("{{pending: stub for {}}}", step_copy.capability_class)),
            );
        }
        Ok(Value::Map(out_map))
    });

    let _registration_result = marketplace
        .register_local_capability(
            stub_id.to_string(),
            format!("STUB: {}", step.name),
            format!(
                "Placeholder for missing capability {}; awaits real implementation",
                step.capability_class
            ),
            handler,
        )
        .await;

    Ok(())
}

/// Print execution graph visualization as a tree structure
fn print_execution_graph(resolved_steps: &[ResolvedStep], intent: &Intent) {
    println!("\n{}", "🌳 Execution Graph".bold());
    println!("{}", "─".repeat(80).dim());
    
    // Print root intent
    println!("{} {}", "🎯 ROOT:".bold().cyan(), intent.goal.as_str().bold());
    
    // Print dependencies as a tree
    for (idx, step) in resolved_steps.iter().enumerate() {
        let is_last = idx == resolved_steps.len() - 1;
        let connector = if is_last { "└─ " } else { "├─ " };
        let indent = "   ";
        
        // Determine status icon and color
        let icon = match step.resolution_strategy {
            ResolutionStrategy::Found => "✅",
            ResolutionStrategy::Synthesized => "🔄",
            ResolutionStrategy::Stubbed => "⚠️ ",
        };
        
        // Print capability info with appropriate color
        match step.resolution_strategy {
            ResolutionStrategy::Found => {
                println!("{} {} {}", connector, icon, step.capability_id.as_str().green());
            }
            ResolutionStrategy::Synthesized => {
                println!("{} {} {}", connector, icon, step.capability_id.as_str().cyan());
            }
            ResolutionStrategy::Stubbed => {
                println!("{} {} {}", connector, icon, step.capability_id.as_str().yellow());
            }
        }
        
        // Print step details
        if !is_last {
            println!("{}{}   {} {}", 
                indent, 
                "│".dim(), 
                "Name:".dim(), 
                step.original.name.as_str()
            );
        } else {
            println!("{}{}   {} {}", 
                indent, 
                " ".dim(), 
                "Name:".dim(), 
                step.original.name.as_str()
            );
        }
        
        // Show inputs/outputs briefly if available
        if !step.original.required_inputs.is_empty() || !step.original.expected_outputs.is_empty() {
            let mut io_summary = Vec::new();
            if !step.original.required_inputs.is_empty() {
                io_summary.push(format!("inputs: {}", step.original.required_inputs.len()));
            }
            if !step.original.expected_outputs.is_empty() {
                io_summary.push(format!("outputs: {}", step.original.expected_outputs.len()));
            }
            
       let indent_char = if is_last { " " } else { "│" };
       let io_text = io_summary.join(", ");
       println!("{}", format!("{}{}   {}", 
           indent, 
           indent_char, 
           io_text
       ).dim());
        }
    }
    
    println!("{}", "─".repeat(80).dim());
    
    // Add legend
    println!("\n{}", "Legend:".dim());
    println!("   ✅ {}  {}", "Found".green(), "- Capability exists in marketplace".dim());
    println!("   🔄 {}  {}", "Synthesized".cyan(), "- Capability generated recursively".dim());
    println!("   ⚠️  {}  {}", "Stubbed".yellow(), "- Placeholder for future implementation".dim());
}

/// Prompt user for guidance when a capability is incomplete
fn prompt_for_capability_url(
    capability_class: &str,
    _manifest: &CapabilityManifest,
) -> Option<String> {
    println!("\n{}", "💬 User input needed".bold().cyan());
    println!(
        "   The capability '{}' could not be found in any available source.",
        capability_class.bold()
    );
    println!("   Options:");
    println!("   • Press ENTER to continue with incomplete capability");
    println!("   • Provide an API documentation URL (OpenAPI/MCP)");
    println!("   • Provide the name of a known API service\n");
    
    print!("   Your input (or press ENTER to skip): ");
    io::stdout().flush().ok();
    
    let mut input = String::new();
    if io::stdin().read_line(&mut input).is_ok() {
        let trimmed = input.trim();
        if !trimmed.is_empty() {
            Some(trimmed.to_string())
        } else {
            None
        }
    } else {
        None
    }
}

/// Register the orchestrator capability in the marketplace so it can be discovered and reused
async fn register_orchestrator_in_marketplace(
    ccos: &Arc<CCOS>,
    capability_id: &str,
    orchestrator_rtfs: &str,
) -> DemoResult<()> {
    let marketplace = ccos.get_capability_marketplace();
    let rtfs_code = orchestrator_rtfs.to_string();

    // Create a handler that returns the RTFS plan code when invoked
    let handler = Arc::new(move |_inputs: &Value| {
        let mut out_map = HashMap::new();
        out_map.insert(
            MapKey::String("plan".into()),
            Value::String(rtfs_code.clone()),
        );
        out_map.insert(
            MapKey::String("status".into()),
            Value::String("ready".into()),
        );
        Ok(Value::Map(out_map))
    });

    let _registration_result = marketplace
        .register_local_capability(
            capability_id.to_string(),
            "Synthesized Plan Orchestrator".to_string(),
            "Auto-generated capability that orchestrates multiple steps into a coordinated plan".to_string(),
            handler,
        )
        .await;

    // Persist the orchestrator RTFS code to disk so it can be executed later by id
    {
        let dir = Path::new("capabilities/generated");
        let persist_result: Result<(), Box<dyn std::error::Error>> = (|| {
            fs::create_dir_all(dir)?;
            let file_path = dir.join(format!("{}.rtfs", capability_id));
            fs::write(file_path, orchestrator_rtfs.as_bytes())?;
            Ok(())
        })();
        if let Err(e) = persist_result {
            eprintln!(
                "⚠️  Failed to persist orchestrator RTFS for {}: {}",
                capability_id, e
            );
        } else {
            println!(
                "  💾 Saved orchestrator RTFS to capabilities/generated/{}.rtfs",
                capability_id
            );
        }
    }

    // Also convert the plan into a first-class Capability and persist under capabilities/generated/<id>/capability.rtfs
    {
        let persist_cap_result: Result<(), Box<dyn std::error::Error>> = (|| {
            let capability_rtfs = convert_plan_to_capability_rtfs(capability_id, orchestrator_rtfs)?;
            let cap_dir = Path::new("capabilities/generated").join(capability_id);
            fs::create_dir_all(&cap_dir)?;
            let cap_file = cap_dir.join("capability.rtfs");
            fs::write(cap_file, capability_rtfs.as_bytes())?;
            Ok(())
        })();
        if let Err(e) = persist_cap_result {
            eprintln!(
                "⚠️  Failed to persist generated capability for {}: {}",
                capability_id, e
            );
        } else {
            println!(
                "  💾 Saved generated capability to capabilities/generated/{}/capability.rtfs",
                capability_id
            );
        }
    }

    println!(
        "  📦 Registered as capability: {}",
        capability_id.cyan()
    );

    Ok(())
}

/// Convert a consolidated RTFS (plan ...) into a Capability RTFS with :implementation holding the plan :body
fn convert_plan_to_capability_rtfs(capability_id: &str, plan_rtfs: &str) -> DemoResult<String> {
    use chrono::Utc;
    let created_at = Utc::now().to_rfc3339();

    // Extract fields from plan
    let body_do = extract_s_expr_after_key(plan_rtfs, ":body")
        .or_else(|| extract_do_block(plan_rtfs))
        .ok_or_else(|| runtime_error(RuntimeError::Generic("Could not extract :body from plan".to_string())))?;
    let input_schema = extract_block_after_key(plan_rtfs, ":input-schema", '{', '}')
        .unwrap_or_else(|| "{}".to_string());
    let output_schema = extract_block_after_key(plan_rtfs, ":output-schema", '{', '}')
        .unwrap_or_else(|| "{}".to_string());
    let caps_required = extract_block_after_key(plan_rtfs, ":capabilities-required", '[', ']')
        .unwrap_or_else(|| "[]".to_string());

    // Assemble capability
    let mut out = String::new();
    out.push_str(&format!("(capability \"{}\"\n", capability_id));
    out.push_str("  :name \"Synthesized Plan Orchestrator\"\n");
    out.push_str("  :version \"1.0.0\"\n");
    out.push_str("  :description \"Auto-generated orchestrator capability from smart_assistant plan\"\n");
    out.push_str("  :source_url \"ccos://generated\"\n");
    out.push_str("  :discovery_method \"smart_assistant\"\n");
    out.push_str(&format!("  :created_at \"{}\"\n", created_at));
    out.push_str("  :capability_type \"orchestrator\"\n");
    out.push_str("  :permissions []\n");
    out.push_str("  :effects []\n");
    out.push_str(&format!("  :capabilities-required {}\n", caps_required));
    out.push_str(&format!("  :input-schema {}\n", input_schema));
    out.push_str(&format!("  :output-schema {}\n", output_schema));
    out.push_str("  :implementation\n");
    out.push_str("    ");
    out.push_str(&body_do);
    out.push_str("\n)\n");
    Ok(out)
}

/// Extracts the first top-level (do ...) s-expression from a text blob.
fn extract_do_block(text: &str) -> Option<String> { extract_block_with_head(text, "do") }

/// Extracts the first top-level s-expression immediately following a given keyword key.
fn extract_s_expr_after_key(text: &str, key: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut i = 0usize;
    let mut in_string = false;
    while i + key.len() <= bytes.len() {
        let c = bytes[i] as char;
        if c == '"' { in_string = !in_string; i += 1; continue; }
        if !in_string && &text[i..i + key.len()] == key {
            // Move to next '('
            let mut j = i + key.len();
            while j < bytes.len() {
                let cj = bytes[j] as char;
                if cj == '"' { in_string = !in_string; j += 1; continue; }
                if !in_string && cj == '(' {
                    return extract_balanced_from(text, j, '(', ')');
                }
                j += 1;
            }
        }
        i += 1;
    }
    None
}

/// Extract a balanced block (like {...} or [...] or (...)) that follows a key.
fn extract_block_after_key(text: &str, key: &str, open: char, close: char) -> Option<String> {
    let bytes = text.as_bytes();
    let mut i = 0usize;
    let mut in_string = false;
    while i + key.len() <= bytes.len() {
        let c = bytes[i] as char;
        if c == '"' { in_string = !in_string; i += 1; continue; }
        if !in_string && &text[i..i + key.len()] == key {
            // Move to next opening delimiter
            let mut j = i + key.len();
            while j < bytes.len() {
                let cj = bytes[j] as char;
                if cj == '"' { in_string = !in_string; j += 1; continue; }
                if !in_string && cj == open {
                    return extract_balanced_from(text, j, open, close);
                }
                j += 1;
            }
        }
        i += 1;
    }
    None
}

/// Extract the first top-level s-expression whose head matches `head`.
fn extract_block_with_head(text: &str, head: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut i = 0usize;
    let mut in_string = false;
    while i < bytes.len() {
        let c = bytes[i] as char;
        if c == '"' { in_string = !in_string; i += 1; continue; }
        if !in_string && c == '(' {
            // Check head
            let mut j = i + 1;
            while j < bytes.len() && (bytes[j] as char).is_whitespace() { j += 1; }
            if j + head.len() <= bytes.len() && &text[j..j + head.len()] == head {
                return extract_balanced_from(text, i, '(', ')');
            }
        }
        i += 1;
    }
    None
}

/// Helper to extract a balanced region starting at index `start` where `text[start] == open`.
fn extract_balanced_from(text: &str, start: usize, open: char, close: char) -> Option<String> {
    let bytes = text.as_bytes();
    if start >= bytes.len() || (bytes[start] as char) != open { return None; }
    let mut depth: i32 = 0;
    let mut in_string = false;
    let mut i = start;
    while i < bytes.len() {
        let c = bytes[i] as char;
        if c == '"' { in_string = !in_string; i += 1; continue; }
        if !in_string {
            if c == open { depth += 1; }
            else if c == close { depth -= 1; if depth == 0 { return Some(text[start..=i].to_string()); } }
        }
        i += 1;
    }
    None
}

/// Generate an RTFS orchestrator capability that chains all resolved steps.
fn generate_orchestrator_capability(
    goal: &str,
    resolved_steps: &[ResolvedStep],
) -> DemoResult<String> {
    let mut rtfs_code = String::new();
    
    // Compute true external inputs by walking steps and excluding inputs produced by prior steps
    let mut produced: HashSet<String> = HashSet::new();
    let mut external_inputs: HashSet<String> = HashSet::new();
    for step in resolved_steps {
        for input in &step.original.required_inputs {
            if !produced.contains(input) {
                external_inputs.insert(input.clone());
            }
        }
        for out in &step.original.expected_outputs {
            produced.insert(out.clone());
        }
    }
    // Collect unique capability ids (capabilities-required)
    let mut cap_ids_set = std::collections::HashSet::new();
    for step in resolved_steps {
        cap_ids_set.insert(step.capability_id.clone());
    }
    let mut cap_ids: Vec<_> = cap_ids_set.into_iter().collect();
    cap_ids.sort();
    // Collect union of all expected outputs across steps and remember producing step index
    let mut output_to_idx: HashMap<String, usize> = HashMap::new();
    for (idx, step) in resolved_steps.iter().enumerate() {
        for out in &step.original.expected_outputs {
            output_to_idx.insert(out.clone(), idx);
        }
    }
    let mut all_outputs: Vec<_> = output_to_idx.keys().cloned().collect();
    all_outputs.sort();
    
    // Build input-schema map with :any type as default
    let input_schema = if external_inputs.is_empty() {
        "{}".to_string()
    } else {
        let mut schema_parts = Vec::new();
        let mut sorted_inputs: Vec<_> = external_inputs.iter().collect();
        sorted_inputs.sort();
        for input in sorted_inputs {
            let ty = infer_input_type(input);
            schema_parts.push(format!("    :{} :{}", input, ty));
        }
        format!("{{\n{}\n  }}", schema_parts.join("\n"))
    };
    
    // Build a proper RTFS 2.0 plan structure with input/output schemas
    rtfs_code.push_str("(plan\n");
    rtfs_code.push_str(&format!("  :name \"synth.plan.orchestrator.v1\"\n"));
    rtfs_code.push_str(&format!("  :language rtfs20\n"));
    if !cap_ids.is_empty() {
        let caps_vec = cap_ids
            .iter()
            .map(|id| format!("\"{}\"", id))
            .collect::<Vec<_>>()
            .join(" ");
        rtfs_code.push_str(&format!("  :capabilities-required [{}]\n", caps_vec));
    }
    rtfs_code.push_str(&format!("  :input-schema {}\n", input_schema));
    // Build output-schema from the union of all steps' expected outputs; fallback to :result
    if !all_outputs.is_empty() {
        let mut parts = Vec::new();
        for key in &all_outputs {
            parts.push(format!("    :{} :any", key));
        }
        rtfs_code.push_str(&format!(
            "  :output-schema {{\n{}\n  }}\n",
            parts.join("\n")
        ));
    } else {
        rtfs_code.push_str(&format!("  :output-schema {{\n    :result :any\n  }}\n"));
    }
    rtfs_code.push_str(&format!(
        "  :annotations {{:goal \"{}\" :step_count {}}}\n",
        goal.replace("\"", "\\\""),
        resolved_steps.len()
    ));
    rtfs_code.push_str("  :body (do\n");

    if resolved_steps.is_empty() {
        rtfs_code.push_str("    (step \"No Steps\" {})\n");
    } else {
        // Build a let-binding that captures each step's result, then compose a final map from outputs
        rtfs_code.push_str("    (let [\n");
        for (idx, resolved) in resolved_steps.iter().enumerate() {
            let step_desc = &resolved.original.name;
            // For wiring, compute a map of available outputs from previous steps
            let mut prior_outputs: HashMap<String, usize> = HashMap::new();
            for (pidx, prev) in resolved_steps.iter().enumerate() {
                if pidx >= idx { break; }
                for out in &prev.original.expected_outputs {
                    prior_outputs.insert(out.clone(), pidx);
                }
            }
            let step_args = build_step_call_args(&resolved.original, &prior_outputs)?;
            rtfs_code.push_str(&format!(
                "      step_{} (step \"{}\" (call :{} {}))\n",
                idx,
                step_desc.replace("\"", "\\\""),
                resolved.capability_id,
                step_args
            ));
        }
        rtfs_code.push_str("    ]\n");
        // Compose final output map pulling keys from the step that produced them
        rtfs_code.push_str("      {\n");
        for (i, key) in all_outputs.iter().enumerate() {
            let src_idx = output_to_idx.get(key).cloned().unwrap_or(0);
            rtfs_code.push_str(&format!(
                "        :{} (get step_{} :{})",
                key, src_idx, key
            ));
            if i < all_outputs.len() - 1 {
                rtfs_code.push_str("\n");
            }
        }
        rtfs_code.push_str("\n      })\n");
    }
    
    rtfs_code.push_str("  )\n");
    rtfs_code.push_str(")\n");
    
    Ok(rtfs_code)
}

fn build_step_call_args(
    step: &ProposedStep,
    prior_outputs: &HashMap<String, usize>,
) -> DemoResult<String> {
    // Build map-based arguments without $ prefix: {:key1 val1 :key2 val2}
    if step.required_inputs.is_empty() {
        return Ok("{}".to_string());
    }
    
    let mut args_parts = vec!["{".to_string()];
    for (i, input) in step.required_inputs.iter().enumerate() {
        if let Some(pidx) = prior_outputs.get(input) {
            args_parts.push(format!("    :{} (get step_{} :{})", input, pidx, input));
        } else {
            args_parts.push(format!("    :{} {}", input, input));
        }
        if i < step.required_inputs.len() - 1 {
            args_parts.push("\n".to_string());
        }
    }
    args_parts.push("\n  }".to_string());
    
    Ok(args_parts.join(""))
}

/// Heuristic input type inference from common parameter names.
fn infer_input_type(name: &str) -> &'static str {
    let n = name.trim().to_ascii_lowercase();
    match n.as_str() {
        // Strings
        "goal" | "origin" | "destination" | "dates" | "lodging_style" | "risk_profile" | "date_range" => "string",
        // Integers
        "party_size" | "n" | "count" => "integer",
        // Numbers (floats/ints)
        "budget" | "amount" | "price" | "cost" => "number",
        // Lists
        "interests" | "preferred_assets" | "sources" | "tags" => "list",
        // Booleans
        "confirm" | "dry_run" | "dryrun" => "boolean",
        // Default
        _ => "any",
    }
}

#[allow(dead_code)]
fn build_final_output(resolved_steps: &[ResolvedStep]) -> DemoResult<String> {
    if resolved_steps.is_empty() {
        return Ok("    {}".to_string());
    }
    
    let mut outputs = Vec::new();
    for (idx, step) in resolved_steps.iter().enumerate() {
        for output_key in &step.original.expected_outputs {
            outputs.push(format!("      :{} step_{}", output_key, idx));
        }
    }

    if outputs.is_empty() {
        Ok("    {}".to_string())
    } else {
        Ok(format!(
            "    {{\n{}\n    }}",
            outputs.join("\n")
        ))
    }
}

fn build_resolved_steps_metadata(resolved_steps: &[ResolvedStep]) -> Value {
    let entries: Vec<Value> = resolved_steps
        .iter()
        .enumerate()
        .map(|(idx, resolved)| {
            let mut map = HashMap::new();
            map.insert(
                MapKey::String("index".into()),
                Value::Integer(idx as i64),
            );
            map.insert(
                MapKey::String("step_id".into()),
                Value::String(resolved.original.id.clone()),
            );
            map.insert(
                MapKey::String("capability_id".into()),
                Value::String(resolved.capability_id.clone()),
            );
            map.insert(
                MapKey::String("strategy".into()),
                Value::String(match resolved.resolution_strategy {
                    ResolutionStrategy::Found => "found",
                    ResolutionStrategy::Stubbed => "stubbed",
                    ResolutionStrategy::Synthesized => "synthesized",
                }
                .to_string()),
            );
            Value::Map(map)
        })
        .collect();
    Value::Vector(entries)
}

async fn match_proposed_steps(
    ccos: &Arc<CCOS>,
    steps: &[ProposedStep],
) -> DemoResult<Vec<CapabilityMatch>> {
    let marketplace = ccos.get_capability_marketplace();
    let intent_graph = ccos.get_intent_graph();
    
    // Create discovery engine for enhanced capability search
    // Pass delegating arbiter if available for recursive synthesis
    let delegating_arbiter = ccos.get_delegating_arbiter();
    let discovery_engine = DiscoveryEngine::new_with_arbiter(
        Arc::clone(&marketplace),
        Arc::clone(&intent_graph),
        delegating_arbiter,
    );
    
    let manifests = marketplace.list_capabilities().await;
    let mut matches = Vec::with_capacity(steps.len());

    for step in steps {
        let exact = step
            .candidate_capabilities
            .iter()
            .find(|id| manifests.iter().any(|m| &m.id == *id))
            .cloned();

        if let Some(id) = exact {
            matches.push(CapabilityMatch {
                step_id: step.id.clone(),
                matched_capability: Some(id),
                status: MatchStatus::ExactId,
                note: None,
            });
            continue;
        }

        if manifests.iter().any(|m| m.id == step.capability_class) {
            matches.push(CapabilityMatch {
                step_id: step.id.clone(),
                matched_capability: Some(step.capability_class.clone()),
                status: MatchStatus::MatchedByClass,
                note: None,
            });
            continue;
        }

        // Try discovery engine for enhanced search
        // Check if capability existed before discovery (to detect synthesis)
        let existed_before = marketplace.get_capability(&step.capability_class).await.is_some();
        
        let need = CapabilityNeed::new(
            step.capability_class.clone(),
            step.required_inputs.clone(),
            step.expected_outputs.clone(),
            format!("Need for step: {}", step.name),
        );
        
        match discovery_engine.discover_capability(&need).await {
            Ok(ccos::discovery::DiscoveryResult::Found(_manifest)) => {
                // Found via discovery - check if it was synthesized or already existed
                let note = if existed_before {
                    "Found via discovery engine".to_string()
                } else {
                    "Synthesized via discovery engine".to_string()
                };
                matches.push(CapabilityMatch {
                    step_id: step.id.clone(),
                    matched_capability: Some(step.capability_class.clone()),
                    status: MatchStatus::MatchedByClass,
                    note: Some(note),
                });
            }
            Ok(ccos::discovery::DiscoveryResult::Incomplete(_manifest)) => {
                // Capability marked as incomplete/not_found
                matches.push(CapabilityMatch {
                    step_id: step.id.clone(),
                    matched_capability: Some(step.capability_class.clone()),
                    status: MatchStatus::Missing,
                    note: Some("Incomplete/not_found - requires manual implementation".to_string()),
                });
            }
            Ok(ccos::discovery::DiscoveryResult::NotFound) | Err(_) => {
                matches.push(CapabilityMatch {
                    step_id: step.id.clone(),
                    matched_capability: None,
                    status: MatchStatus::Missing,
                    note: Some("No matching capability registered".to_string()),
                });
            }
        }
    }

    Ok(matches)
}

fn annotate_steps_with_matches(steps: &mut [ProposedStep], matches: &[CapabilityMatch]) {
    for step in steps {
        if let Some(found) = matches.iter().find(|m| m.step_id == step.id) {
            if step.description.is_none() {
                if let Some(cap) = &found.matched_capability {
                    step.description = Some(format!("Matched capability {}", cap));
                }
            }
        }
    }
}

fn print_plan_draft(steps: &[ProposedStep], matches: &[CapabilityMatch], plan: &Plan) {
    println!("\n{}", "🗂️  Proposed plan steps".bold());
    for step in steps {
        let status = matches
            .iter()
            .find(|m| m.step_id == step.id)
            .map(|m| match m.status {
                MatchStatus::ExactId => "matched".green().to_string(),
                MatchStatus::MatchedByClass => "matched by class".yellow().to_string(),
                MatchStatus::Missing => "missing".red().to_string(),
            })
            .unwrap_or_else(|| "unknown".into());

        println!(
            " • {} ({}) → {}",
            step.name.as_str().bold(),
            step.capability_class.as_str().cyan(),
            status
        );
        if !step.required_inputs.is_empty() {
            println!(
                "   • Inputs: {}",
                step.required_inputs
                    .iter()
                    .map(|s| format!(":{}", s))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        if let Some(desc) = &step.description {
            println!("   • {}", desc.as_str().dim());
        }
        if let Some(note) = matches
            .iter()
            .find(|m| m.step_id == step.id)
            .and_then(|m| m.note.as_ref())
        {
            println!("   • {}", note.as_str().dim());
        }
    }

    println!("\n{}", "📋 Generated Orchestrator RTFS".bold());
    if let ccos::types::PlanBody::Rtfs(code) = &plan.body {
        println!("{}", code.as_str().cyan());
    }

    println!("\n{}", "🧾 Plan metadata".bold());
    for (key, value) in &plan.metadata {
        println!("   • {} = {}", key.as_str().cyan(), format_value(value));
    }
}

/*
        );
        inputs.insert(
            "sources",
            ScenarioInputSpec {
                key: "sources",
                prompt: "Which data sources do you trust for this report?",
                rationale:
                    "The fetcher pulls only approved sources; consent is required for web/API access.",
                scope: "network.http",
                phase: InputCollectionPhase::PrePlan,
                input_type: InputType::List,
            },
        );
        inputs.insert(
            "date_range",
            ScenarioInputSpec {
                key: "date_range",
                prompt: "What date range should we cover?",
                rationale: "Time window drives API calls and ensures policy-compliant archival depth.",
                scope: "temporal",
                phase: InputCollectionPhase::PrePlan,
                input_type: InputType::String,
            },
        );
        inputs.insert(
            "chart_type",
            ScenarioInputSpec {
                key: "chart_type",
                prompt: "How should the data be visualized (e.g., line, bar, radar)?",
                rationale: "Chart generator must know the visualization contract to reserve resources.",
                scope: "rendering",
                phase: InputCollectionPhase::PrePlan,
                input_type: InputType::String,
            },
        );
        inputs.insert(
            "metric",
            ScenarioInputSpec {
                key: "metric",
                prompt: "Which metric should the chart highlight?",
                rationale: "Aggregations depend on the primary metric selected by the user.",
                scope: "analysis",
                phase: InputCollectionPhase::PrePlan,
                input_type: InputType::String,
            },
        );
        inputs.insert(
            "audience",
            ScenarioInputSpec {
                key: "audience",
                prompt: "Who is the target audience for the report?",
                rationale: "Narrative tone and template vary per audience persona.",
                scope: "consent",
                phase: InputCollectionPhase::PrePlan,
                input_type: InputType::String,
            },
        );
        inputs.insert(
            "delivery_channel",
            ScenarioInputSpec {
                key: "delivery_channel",
                prompt: "Where should we deliver the final report (pdf/email/cloud)?",
                rationale: "Filesystem/network policies enforce consented delivery channels only.",
                scope: "delivery",
                phase: InputCollectionPhase::PrePlan,
                input_type: InputType::String,
            },
        );
        inputs.insert(
            "filters",
            ScenarioInputSpec {
                key: "filters",
                prompt: "Any filters to clean anomalies or focus the dataset?",
                rationale: "Data cleaning step pauses execution until filters are confirmed.",
                scope: "data.retention",
                phase: InputCollectionPhase::OnDemand,
                input_type: InputType::String,
            },
        );

        let steps = vec![
            StepSpec {
                id: "fetch_data",
                name: "Fetch live data",
                capability_id: "demo.data.fetch",
                capability_class: "data.fetch.timeseries",
                required_inputs: vec!["topic", "sources", "date_range"],
                expected_outputs: vec!["dataset"],
                description: "Pulls structured metrics from approved sources.",
            },
            StepSpec {
                id: "clean_data",
                name: "Clean + normalize",
                capability_id: "demo.data.clean",
                capability_class: "data.normalize.generic",
                required_inputs: vec!["dataset", "filters"],
                expected_outputs: vec!["clean_dataset"],
                description: "Applies schema validation and anomaly filtering.",
            },
            StepSpec {
                id: "generate_chart",
                name: "Generate visualization",
                capability_id: "demo.chart.generate",
                capability_class: "data.chart.render",
                required_inputs: vec!["clean_dataset", "chart_type", "metric"],
                expected_outputs: vec!["chart"],
                description: "Produces a chart artifact ready for embedding.",
            },
            StepSpec {
                id: "compose_report",
                name: "Compose PDF report",
                capability_id: "demo.report.compose",
                capability_class: "report.compose.pdf",
                required_inputs: vec!["chart", "audience", "delivery_channel"],
                expected_outputs: vec!["report_path"],
                description: "Formats findings into the chosen delivery channel.",
            },
        ];

        Self { steps, inputs }
    }

    fn input(&self, key: &str) -> Option<&ScenarioInputSpec> {
        self.inputs.get(key)
    }
}

#[derive(Clone, Debug)]
struct StepSpec {
    id: &'static str,
    name: &'static str,
    capability_id: &'static str,
    capability_class: &'static str,
    required_inputs: Vec<&'static str>,
    expected_outputs: Vec<&'static str>,
    description: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InputCollectionPhase {
    PrePlan,
    OnDemand,
}

#[derive(Clone, Copy, Debug)]
enum InputType {
    String,
    List,
}

#[derive(Clone, Debug)]
struct ScenarioInputSpec {
    key: &'static str,
    prompt: &'static str,
    rationale: &'static str,
    scope: &'static str,
    phase: InputCollectionPhase,
    input_type: InputType,
}

impl ScenarioInputSpec {
    fn default_value(&self, goal: &str) -> Value {
        match self.key {
            "topic" => Value::String(goal.to_string()),
            "sources" => Value::Vector(vec![
                Value::String("open_government".to_string()),
                Value::String("industry_reports".to_string()),
            ]),
            "date_range" => Value::String("last 12 months".to_string()),
            "chart_type" => Value::String("line".to_string()),
            "metric" => Value::String("average_growth".to_string()),
            "audience" => Value::String("executive".to_string()),
            "delivery_channel" => Value::String("pdf".to_string()),
            "filters" => Value::String(String::new()),
            _ => Value::Nil,
        }
    }

    fn value_from_text(&self, text: &str) -> Value {
        match self.input_type {
            InputType::String => Value::String(text.trim().to_string()),
            InputType::List => {
                let items: Vec<Value> = text
                    .split(|c| c == ',' || c == ';')
                    .map(|s| Value::String(s.trim().to_string()))
                    .filter(|v| match v {
                        Value::String(s) => !s.is_empty(),
                        _ => true,
                    })
                    .collect();
                Value::Vector(items)
            }
        }
    }
}

// -----------------------------------------------------------------------------
// Learning run orchestration
// -----------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize)]
struct AnswerRecord {
    key: String,
    question: String,
    rationale: String,
    value: Value,
    source: AnswerSource,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
enum AnswerSource {
    UserProvided,
    AssumedDefault,
    Learned,
}

#[derive(Clone, Debug)]
struct DemoRunState {
    goal: String,
    answers: BTreeMap<String, AnswerRecord>,
    outputs: HashMap<String, Value>,
    partials: Vec<PartialExecutionOutcome>,
    steps: Vec<StepExecution>,
}

#[derive(Clone, Debug)]
struct StepExecution {
    id: String,
    name: String,
    status: StepStatus,
    outputs: HashMap<String, Value>,
}

#[derive(Clone, Debug)]
enum StepStatus {
    Completed,
    Partial { missing_inputs: Vec<String>, message: String },
    Failed(String),
}

#[derive(Clone, Debug)]
struct PartialExecutionOutcome {
    step_id: String,
    status: PartialStatus,
    message: String,
    missing_inputs: Vec<String>,
    evidence: HashMap<String, Value>,
}

#[derive(Clone, Debug)]
enum PartialStatus {
    NeedsInput,
}

#[derive(Clone, Debug)]
struct LearningMetrics {
    turns: usize,
    clarifying_questions: usize,
    step_count: usize,
    partials: Vec<PartialExecutionOutcome>,
    answers: Vec<AnswerRecord>,
    synth_capability: Option<SynthesizedCapability>,
}

#[derive(Clone, Debug)]
struct ApplicationMetrics {
    step_count: usize,
    reused_capability_id: Option<String>,
    duration_ms: u128,
}

async fn run_learning_phase(
    ccos: &Arc<CCOS>,
    scenario: &DemoScenario,
    goal: &str,
    persist: bool,
    debug_prompts: bool,
) -> Result<LearningMetrics, Box<dyn std::error::Error>> {
    let mut state = DemoRunState {
        goal: goal.to_string(),
        answers: BTreeMap::new(),
        outputs: HashMap::new(),
        partials: Vec::new(),
        steps: Vec::new(),
    };

    prime_answer(&mut state, scenario, "topic", AnswerSource::UserProvided, goal);

    let preplan_questions = build_questions(scenario, goal, InputCollectionPhase::PrePlan);
    let mut clarifying_questions = 0usize;
    for q in preplan_questions {
        clarifying_questions += 1;
        let answer = ask_with_fallback(ccos, &q, debug_prompts)?;
        state.answers.insert(
            q.key.clone(),
            AnswerRecord {
                key: q.key.clone(),
                question: q.prompt.clone(),
                rationale: q.rationale.clone(),
                value: answer,
                source: AnswerSource::UserProvided,
            },
        );
    }

    let needs_metadata = build_needs_capabilities_value(&state, scenario);
    let mut plan = Plan::new_rtfs("(call :demo.placeholder {})".to_string(), vec![]);
    plan
        .metadata
        .insert("needs_capabilities".to_string(), needs_metadata);

    println!("\n{}", "Initial plan metadata:".bold());
    for step in &scenario.steps {
        println!(
            "  • {} {}",
            step.name.cyan(),
            format!("({})", step.capability_id).dim()
        );
    }

    execute_plan(ccos, scenario, &mut state, debug_prompts).await?;

    let synth = synthesize_capability(scenario, &state);
    if persist {
        persist_capability(&synth.id, &synth.rtfs_spec)?;
        persist_plan(&format!("{}-plan", synth.id), &synth.plan_spec)?;
    }
    register_synthesized_capability(ccos, &synth, scenario).await?;

    Ok(LearningMetrics {
        turns: state.answers.len(),
        clarifying_questions,
        step_count: state.steps.len(),
        partials: state.partials.clone(),
        answers: state.answers.values().cloned().collect(),
        synth_capability: Some(synth),
    })
}

async fn execute_plan(
    ccos: &Arc<CCOS>,
    scenario: &DemoScenario,
    state: &mut DemoRunState,
    debug_prompts: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let marketplace = ccos.get_capability_marketplace();

    for step in &scenario.steps {
        let mut attempts = 0usize;
        loop {
            attempts += 1;
            let input_value = build_step_inputs(step, state, scenario)?;
            if debug_prompts {
                println!(
                    "{} {}",
                    "Executing step".dim(),
                    format!("{} (attempt #{})", step.id, attempts).yellow()
                );
            }

            let result = marketplace
                .execute_capability(step.capability_id, &Value::Map(input_value.clone()))
                .await?;
            let parsed = StepExecutionResult::from_value(step, result.clone())?;

            match parsed.status.clone() {
                StepStatus::Completed => {
                    state.outputs.extend(parsed.outputs.clone());
                    state.steps.push(StepExecution {
                        id: step.id.to_string(),
                        name: step.name.to_string(),
                        status: StepStatus::Completed,
                        outputs: parsed.outputs.clone(),
                    });
                    if debug_prompts {
                        println!(
                            "{} {}",
                            "✓ Completed".green(),
                            step.name.to_string().bold()
                        );
                    }
                    break;
                }
                StepStatus::Partial { missing_inputs, message } => {
                    let partial = PartialExecutionOutcome {
                        step_id: step.id.to_string(),
                        status: PartialStatus::NeedsInput,
                        message: message.clone(),
                        missing_inputs: missing_inputs.clone(),
                        evidence: parsed.outputs.clone(),
                    };
                    state.partials.push(partial.clone());
                    println!(
                        "{} {} {}",
                        "⏸ Partial outcome".yellow().bold(),
                        step.name,
                        message.dim()
                    );
                    collect_on_demand_inputs(ccos, scenario, state, &missing_inputs)?;
                    continue;
                }
                StepStatus::Failed(reason) => {
                    return Err(RuntimeError::Generic(format!(
                        "Step {} failed: {}",
                        step.id, reason
                    ))
                    .into());
                }
            }
        }
    }

    Ok(())
}

fn build_step_inputs(
    step: &StepSpec,
    state: &DemoRunState,
    scenario: &DemoScenario,
) -> Result<HashMap<MapKey, Value>, RuntimeError> {
    let mut map = HashMap::new();
    for key in &step.required_inputs {
        if let Some(answer) = state.answers.get(*key) {
            map.insert(MapKey::String((*key).to_string()), answer.value.clone());
            continue;
        }

        if let Some(value) = state.outputs.get(*key) {
            map.insert(MapKey::String((*key).to_string()), value.clone());
            continue;
        }

        if *key == "dataset" {
            if let Some(out) = state.outputs.get("dataset") {
                map.insert(MapKey::String("dataset".into()), out.clone());
                continue;
            }
        }

        if let Some(spec) = scenario.input(key) {
            map.insert(MapKey::String((*key).to_string()), spec.default_value(&state.goal));
        } else {
            return Err(RuntimeError::Generic(format!(
                "Missing input '{}' for step {}",
                key, step.id
            )));
        }
    }
    Ok(map)
}

fn collect_on_demand_inputs(
    ccos: &Arc<CCOS>,
    scenario: &DemoScenario,
    state: &mut DemoRunState,
    missing: &[String],
) -> Result<(), RuntimeError> {
    for key in missing {
        if let Some(spec) = scenario.input(key) {
            let question = QuestionSpec {
                key: key.to_string(),
                prompt: spec.prompt.to_string(),
                rationale: spec.rationale.to_string(),
                scope: spec.scope.to_string(),
                phase: spec.phase,
                input_type: spec.input_type,
            };
            let value = ask_with_fallback(ccos, &question, false)?;
            state.answers.insert(
                key.clone(),
                AnswerRecord {
                    key: key.clone(),
                    question: question.prompt,
                    rationale: question.rationale,
                    value,
                    source: AnswerSource::UserProvided,
                },
            );
        }
    }
    Ok(())
}

// -----------------------------------------------------------------------------
// Step result parsing and capability handlers
// -----------------------------------------------------------------------------

struct StepExecutionResult {
    status: StepStatus,
    outputs: HashMap<String, Value>,
}

impl StepExecutionResult {
    fn from_value(step: &StepSpec, value: Value) -> Result<Self, RuntimeError> {
        let map = match value {
            Value::Map(m) => m,
            other => {
                return Err(RuntimeError::Generic(format!(
                    "Capability {} returned non-map {:?}",
                    step.capability_id, other.type_name()
                )))
            }
        };

        let status = match map.get(&MapKey::String("status".into())) {
            Some(Value::String(s)) => s.to_string(),
            _ => "complete".to_string(),
        };

        let outputs = map
            .get(&MapKey::String("outputs".into()))
            .and_then(|v| match v {
                Value::Map(m) => Some(m.clone()),
                _ => None,
            })
            .unwrap_or_default()
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect();

        if status == "needs-input" {
            let missing_inputs = map
                .get(&MapKey::String("missing_inputs".into()))
                .and_then(|v| match v {
                    Value::Vector(vec) => Some(
                        vec.iter()
                            .filter_map(|val| match val {
                                Value::String(s) => Some(s.clone()),
                                _ => None,
                            })
                            .collect::<Vec<String>>(),
                    ),
                    _ => None,
                })
                .unwrap_or_default();
            let message = map
                .get(&MapKey::String("message".into()))
                .and_then(|v| match v {
                    Value::String(s) => Some(s.clone()),
                    _ => None,
                })
                .unwrap_or_else(|| "Additional input required".to_string());
            return Ok(Self {
                status: StepStatus::Partial {
                    missing_inputs,
                    message,
                },
                outputs,
            });
        }

        if status == "failed" {
            let message = map
                .get(&MapKey::String("message".into()))
                .and_then(|v| match v {
                    Value::String(s) => Some(s.clone()),
                    _ => None,
                })
                .unwrap_or_else(|| "Capability failed".to_string());
            return Ok(Self {
                status: StepStatus::Failed(message),
                outputs,
            });
        }

        Ok(Self {
            status: StepStatus::Completed,
            outputs,
        })
    }
}

struct DemoCapabilities;

impl DemoCapabilities {
    fn fetch(inputs: &HashMap<MapKey, Value>) -> RuntimeResult<Value> {
        let topic = Self::expect_string(inputs, "topic")?;
        let sources = inputs
            .get(&MapKey::String("sources".into()))
            .cloned()
            .unwrap_or_else(|| Value::Vector(vec![]));
        let date_range = Self::expect_string(inputs, "date_range")?;

        let mut outputs = HashMap::new();
        let mut dataset = HashMap::new();
        dataset.insert(MapKey::String("topic".into()), Value::String(topic.clone()));
        dataset.insert(MapKey::String("records".into()), Value::Integer(128));
        dataset.insert(MapKey::String("date_range".into()), Value::String(date_range));
        dataset.insert(MapKey::String("sources".into()), sources.clone());

        outputs.insert(MapKey::String("dataset".into()), Value::Map(dataset));

        Self::complete(outputs)
    }

    fn clean(inputs: &HashMap<MapKey, Value>) -> RuntimeResult<Value> {
        let dataset = inputs
            .get(&MapKey::String("dataset".into()))
            .cloned()
            .unwrap_or(Value::Nil);
        let filters = inputs
            .get(&MapKey::String("filters".into()))
            .cloned()
            .unwrap_or(Value::String(String::new()));

        match filters {
            Value::String(ref s) if s.trim().is_empty() => {
                Self::needs_input(vec!["filters".to_string()], "Cleaning requires explicit filters")
            }
            _ => {
                let mut clean_map = HashMap::new();
                clean_map.insert(MapKey::String("dataset".into()), dataset);
                clean_map.insert(MapKey::String("applied_filters".into()), filters);
                clean_map.insert(
                    MapKey::String("validation_warnings".into()),
                    Value::Vector(vec![]),
                );
                let mut outputs = HashMap::new();
                outputs.insert(MapKey::String("clean_dataset".into()), Value::Map(clean_map));
                Self::complete(outputs)
            }
        }
    }

    fn chart(inputs: &HashMap<MapKey, Value>) -> RuntimeResult<Value> {
        let chart_type = Self::expect_string(inputs, "chart_type")?;
        let metric = Self::expect_string(inputs, "metric")?;
        let mut outputs = HashMap::new();
        outputs.insert(
            MapKey::String("chart".into()),
            Value::String(format!(
                "artifacts/charts/{}_{}_{}.png",
                chart_type,
                metric,
                chrono::Utc::now().timestamp()
            )),
        );
        outputs.insert(
            MapKey::String("summary".into()),
            Value::String("Trend remains positive over the selected period.".into()),
        );
        Self::complete(outputs)
    }

    fn report(inputs: &HashMap<MapKey, Value>) -> RuntimeResult<Value> {
        let audience = Self::expect_string(inputs, "audience")?;
        let delivery = Self::expect_string(inputs, "delivery_channel")?;
        let mut outputs = HashMap::new();
        outputs.insert(
            MapKey::String("report_path".into()),
            Value::String(format!(
                "artifacts/reports/{}_{}.pdf",
                audience,
                chrono::Utc::now().timestamp()
            )),
        );
        outputs.insert(
            MapKey::String("delivery".into()),
            Value::String(delivery),
        );
        Self::complete(outputs)
    }

    fn expect_string(inputs: &HashMap<MapKey, Value>, key: &str) -> RuntimeResult<String> {
        match inputs.get(&MapKey::String(key.to_string())) {
            Some(Value::String(s)) if !s.is_empty() => Ok(s.clone()),
            Some(Value::String(_)) => Err(RuntimeError::Generic(format!(
                "Input '{}' must be non-empty",
                key
            ))),
            Some(_) => Err(RuntimeError::Generic(format!(
                "Input '{}' must be a string",
                key
            ))),
            None => Err(RuntimeError::Generic(format!("Missing input '{}'", key))),
        }
    }

    fn complete(outputs: HashMap<MapKey, Value>) -> RuntimeResult<Value> {
        let mut map = HashMap::new();
        map.insert(MapKey::String("status".into()), Value::String("complete".into()));
        map.insert(MapKey::String("outputs".into()), Value::Map(outputs));
        Ok(Value::Map(map))
    }

    fn needs_input(missing: Vec<String>, message: &str) -> RuntimeResult<Value> {
        let mut map = HashMap::new();
        map.insert(
            MapKey::String("status".into()),
            Value::String("needs-input".into()),
        );
        map.insert(
            MapKey::String("missing_inputs".into()),
            Value::Vector(missing.into_iter().map(Value::String).collect()),
        );
        map.insert(
            MapKey::String("message".into()),
            Value::String(message.to_string()),
        );
        map.insert(
            MapKey::String("outputs".into()),
            Value::Map(HashMap::new()),
        );
        Ok(Value::Map(map))
    }
}

async fn register_demo_capabilities(ccos: &Arc<CCOS>) -> Result<(), RuntimeError> {
    let marketplace = ccos.get_capability_marketplace();
    let mp_fetch = Arc::new(|input: &Value| match input {
        Value::Map(map) => DemoCapabilities::fetch(map),
        _ => Err(RuntimeError::Generic(
            "demo.data.fetch expects a map argument".to_string(),
        )),
    });
    marketplace
        .register_local_capability(
            "demo.data.fetch".to_string(),
            "Fetch structured data".to_string(),
            "Simulated data acquisition".to_string(),
            mp_fetch,
        )
        .await?;

    let mp_clean = Arc::new(|input: &Value| match input {
        Value::Map(map) => DemoCapabilities::clean(map),
        _ => Err(RuntimeError::Generic(
            "demo.data.clean expects a map argument".to_string(),
        )),
    });
    marketplace
        .register_local_capability(
            "demo.data.clean".to_string(),
            "Clean data".to_string(),
            "Normalizes and validates datasets".to_string(),
            mp_clean,
        )
        .await?;

    let mp_chart = Arc::new(|input: &Value| match input {
        Value::Map(map) => DemoCapabilities::chart(map),
        _ => Err(RuntimeError::Generic(
            "demo.chart.generate expects a map argument".to_string(),
        )),
    });
    marketplace
        .register_local_capability(
            "demo.chart.generate".to_string(),
            "Generate visualization".to_string(),
            "Produces chart artifacts".to_string(),
            mp_chart,
        )
        .await?;

    let mp_report = Arc::new(|input: &Value| match input {
        Value::Map(map) => DemoCapabilities::report(map),
        _ => Err(RuntimeError::Generic(
            "demo.report.compose expects a map argument".to_string(),
        )),
    });
    marketplace
        .register_local_capability(
            "demo.report.compose".to_string(),
            "Compose report".to_string(),
            "Formats narrative and exports PDF".to_string(),
            mp_report,
        )
        .await?;

    Ok(())
}

// -----------------------------------------------------------------------------
// Questions and answer collection
// -----------------------------------------------------------------------------

struct QuestionSpec {
    key: String,
    prompt: String,
    rationale: String,
    scope: String,
    phase: InputCollectionPhase,
    input_type: InputType,
}

fn build_questions(
    scenario: &DemoScenario,
    goal: &str,
    phase: InputCollectionPhase,
) -> Vec<QuestionSpec> {
    scenario
        .inputs
        .values()
        .filter(|spec| spec.phase == phase)
        .map(|spec| QuestionSpec {
            key: spec.key.to_string(),
            prompt: spec.prompt.to_string(),
            rationale: format!("{} (scope: {})", spec.rationale, spec.scope),
            scope: spec.scope.to_string(),
            phase: spec.phase,
            input_type: spec.input_type,
        })
        .filter(|spec| spec.key != "topic" || spec.phase != InputCollectionPhase::PrePlan)
        .map(|mut qs| {
            if qs.key == "topic" {
                qs.prompt = format!("Confirm the goal/topic (current: '{}')", goal);
            }
            qs
        })
        .collect()
}

fn ask_with_fallback(
    _ccos: &Arc<CCOS>,
    question: &QuestionSpec,
    debug_prompts: bool,
) -> Result<Value, RuntimeError> {
    println!(
        "{} {}\n   {}",
        "❓".bold(),
        question.prompt.as_str().bold(),
        question.rationale.as_str().dim()
    );

    if std::env::var("CCOS_INTERACTIVE_ASK").ok().as_deref() == Some("1") {
        print!("   ↳ answer: ");
        io::stdout().flush().map_err(|e| RuntimeError::Generic(e.to_string()))?;
        let mut buffer = String::new();
        io::stdin()
            .read_line(&mut buffer)
            .map_err(|e| RuntimeError::Generic(e.to_string()))?;
        let value = parse_question_input(question, buffer.trim());
        return Ok(value);
    }

    let env_key = format!("SMART_ASSISTANT_{}", question.key.to_ascii_uppercase());
    if let Ok(val) = std::env::var(&env_key) {
        if debug_prompts {
            println!("   using env {} = {}", env_key, val.as_str().cyan());
        }
        return Ok(parse_question_input(question, &val));
    }

    let default_text = default_answer_for(question);
    if debug_prompts {
        println!("   using default {}", default_text.as_str().dim());
    }
    Ok(parse_question_input(question, &default_text))
}

fn parse_question_input(question: &QuestionSpec, text: &str) -> Value {
    match question.input_type {
        InputType::String => Value::String(text.trim().to_string()),
        InputType::List => {
            let items: Vec<Value> = text
                .split(|c| c == ',' || c == ';')
                .map(|s| Value::String(s.trim().to_string()))
                .filter(|v| match v {
                    Value::String(s) => !s.is_empty(),
                    _ => true,
                })
                .collect();
            Value::Vector(items)
        }
    }
}

fn default_answer_for(question: &QuestionSpec) -> String {
    match question.key.as_str() {
        "sources" => "open_government, industry_reports".to_string(),
        "date_range" => "last 12 months".to_string(),
        "chart_type" => "line".to_string(),
        "metric" => "average_growth".to_string(),
        "audience" => "executive".to_string(),
        "delivery_channel" => "pdf".to_string(),
        "filters" => "exclude_outliers=true;aggregate=weekly".to_string(),
        "topic" => DEFAULT_GOAL.to_string(),
        _ => DEFAULT_GOAL.to_string(),
    }
}

fn prime_answer(
    state: &mut DemoRunState,
    scenario: &DemoScenario,
    key: &str,
    source: AnswerSource,
    value_text: &str,
) {
    if let Some(spec) = scenario.input(key) {
        let value = spec.value_from_text(value_text);
        state.answers.insert(
            key.to_string(),
            AnswerRecord {
                key: key.to_string(),
                question: spec.prompt.to_string(),
                rationale: spec.rationale.to_string(),
                value,
                source,
            },
        );
    }
}

// -----------------------------------------------------------------------------
// Capability synthesis
// -----------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct SynthesizedCapability {
    id: String,
    name: String,
    description: String,
    rtfs_spec: String,
    plan_spec: String,
    defaults: HashMap<String, Value>,
}

fn synthesize_capability(scenario: &DemoScenario, state: &DemoRunState) -> SynthesizedCapability {
    let slug = sanitize_id(&state.goal);
    let id = format!("generated.smart_report.{}", slug);
    let name = format!("Smart report planner ({})", state.goal);
    let description = "Synthesized capability that captures the governed workflow for report generation";

    let mut defaults = HashMap::new();
    for (key, answer) in &state.answers {
        defaults.insert(key.clone(), answer.value.clone());
    }

    let rtfs_spec = build_rtfs_capability_spec(&id, &name, description, scenario);
    let plan_spec = build_plan_spec(scenario);

    SynthesizedCapability {
        id,
        name,
        description: description.to_string(),
        rtfs_spec,
        plan_spec,
        defaults,
    }
}

async fn register_synthesized_capability(
    ccos: &Arc<CCOS>,
    synth: &SynthesizedCapability,
    scenario: &DemoScenario,
) -> Result<(), RuntimeError> {
    let defaults = synth.defaults.clone();
    let steps = scenario.steps.clone();

    let closure_marketplace = ccos.get_capability_marketplace();
    let handler = Arc::new(move |inputs: &Value| match inputs {
        Value::Map(map) => {
            let mut merged: HashMap<String, Value> = defaults
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();

            for (k, v) in map {
                merged.insert(k.to_string(), v.clone());
            }

            let mut state = DemoRunState {
                goal: merged
                    .get("topic")
                    .and_then(|v| match v {
                        Value::String(s) => Some(s.clone()),
                        _ => None,
                    })
                    .unwrap_or_else(|| DEFAULT_GOAL.to_string()),
                answers: BTreeMap::new(),
                outputs: HashMap::new(),
                partials: Vec::new(),
                steps: Vec::new(),
            };

            for (key, value) in &merged {
                state.answers.insert(
                    key.clone(),
                    AnswerRecord {
                        key: key.clone(),
                        question: key.clone(),
                        rationale: "synthesized default".to_string(),
                        value: value.clone(),
                        source: AnswerSource::Learned,
                    },
                );
            }

            for step in &steps {
                let input_map = build_step_inputs(step, &state, scenario)
                    .map_err(|e| RuntimeError::Generic(e.to_string()))?;
                let result = match step.capability_id {
                    "demo.data.fetch" => DemoCapabilities::fetch(&input_map),
                    "demo.data.clean" => DemoCapabilities::clean(&input_map),
                    "demo.chart.generate" => DemoCapabilities::chart(&input_map),
                    "demo.report.compose" => DemoCapabilities::report(&input_map),
                    other => {
                        return Err(RuntimeError::Generic(format!(
                            "Unknown capability {} in synthesized pipeline",
                            other
                        )))
                    }
                }?;
                let parsed = StepExecutionResult::from_value(step, result)?;
                if let StepStatus::Completed = parsed.status {
                    state.outputs.extend(parsed.outputs.clone());
                }
            }

            let mut out_map = HashMap::new();
            if let Some(report) = state.outputs.get("report_path") {
                out_map.insert(MapKey::String("report_path".into()), report.clone());
            }
            DemoCapabilities::complete(out_map)
        }
        _ => Err(RuntimeError::Generic(
            "Synthesized capability expects map input".to_string(),
        )),
    });

    closure_marketplace
        .register_local_capability(
            synth.id.clone(),
            synth.name.clone(),
            synth.description.clone(),
            handler,
        )
        .await
}

fn build_rtfs_capability_spec(
    id: &str,
    name: &str,
    description: &str,
    scenario: &DemoScenario,
) -> String {
    let params: Vec<String> = scenario
        .inputs
        .keys()
        .map(|key| format!(":{} \"string\"", key))
        .collect();

    format!(
        r#"(capability "{id}"
  :description "{description}"
  :parameters {{ {params} }}
  :implementation
  :metadata {{:kind :planner}}
)"#,
        id = id,
        description = description,
        params = params.join(" "),
    )
}

fn build_plan_spec(scenario: &DemoScenario) -> String {
    let mut metadata_entries = Vec::new();
    for step in &scenario.steps {
        let required_inputs: Vec<String> = step
            .required_inputs
            .iter()
            .map(|i| format!(":{}", i))
            .collect();
        let expected_outputs: Vec<String> = step
            .expected_outputs
            .iter()
            .map(|o| format!(":{}", o))
            .collect();
        metadata_entries.push(format!(
            "{{:class \"{}\" :required_inputs [{}] :expected_outputs [{}]}}",
            step.capability_class,
            required_inputs.join(" "),
            expected_outputs.join(" ")
        ));
    }

    format!(
        r#"(plan smart_report_plan
  :language "rtfs20"
  :metadata {{:needs_capabilities [{needs}]}}
  :body "(do
    (def fetch (call :demo.data.fetch {{:topic topic :sources sources :date_range date_range}}))
    (def clean (call :demo.data.clean {{:dataset (:dataset fetch) :filters filters}}))
    (def chart (call :demo.chart.generate {{:clean_dataset (:clean_dataset clean) :chart_type chart_type :metric metric}}))
    (call :demo.report.compose {{:chart (:chart chart) :audience audience :delivery_channel delivery_channel}}))"
)"#,
        needs = metadata_entries.join(" ")
    )
}

fn build_needs_capabilities_value(state: &DemoRunState, scenario: &DemoScenario) -> Value {
    let mut entries = Vec::new();
    for step in &scenario.steps {
        let mut map = HashMap::new();
        map.insert(
            MapKey::Keyword(Keyword::new("class")),
            Value::String(step.capability_class.to_string()),
        );
        map.insert(
            MapKey::Keyword(Keyword::new("capability_id")),
            Value::String(step.capability_id.to_string()),
        );
        map.insert(
            MapKey::Keyword(Keyword::new("required_inputs")),
            Value::Vector(
                step.required_inputs
                    .iter()
                    .map(|i| Value::Keyword(Keyword::new(i)))
                    .collect(),
            ),
        );
        map.insert(
            MapKey::Keyword(Keyword::new("expected_outputs")),
            Value::Vector(
                step.expected_outputs
                    .iter()
                    .map(|o| Value::Keyword(Keyword::new(o)))
                    .collect(),
            ),
        );
        entries.push(Value::Map(map));
    }
    Value::Vector(entries)
}

fn sanitize_id(goal: &str) -> String {
    let mut slug: String = goal
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else if c.is_whitespace() || c == '-' {
                '-'
            } else {
                '-'
            }
        })
        .collect();

    while slug.contains("--") {
        slug = slug.replace("--", "-");
    }

    slug = slug.trim_matches('-').to_string();
    if slug.is_empty() {
        format!("smart-report-{}", chrono::Utc::now().timestamp())
    } else {
        format!("{}-{}", slug, chrono::Utc::now().timestamp())
    }
}

fn persist_capability(id: &str, spec: &str) -> Result<(), Box<dyn std::error::Error>> {
    let dir = Path::new("capabilities/generated");
    fs::create_dir_all(dir)?;
    let file_path = dir.join(format!("{}.rtfs", id));
    fs::write(file_path, spec.as_bytes())?;
    Ok(())
}

fn persist_plan(id: &str, plan_code: &str) -> Result<(), Box<dyn std::error::Error>> {
    let dir = Path::new("capabilities/generated");
    fs::create_dir_all(dir)?;
    let file_path = dir.join(format!("{}-plan.rtfs", id));
    fs::write(file_path, plan_code.as_bytes())?;
    Ok(())
}

async fn run_application_phase(
    ccos: &Arc<CCOS>,
    scenario: &DemoScenario,
    previous_goal: &str,
    debug_prompts: bool,
) -> Result<ApplicationMetrics, Box<dyn std::error::Error>> {
    let start = std::time::Instant::now();
    let new_goal = std::env::var("SMART_ASSISTANT_SECOND_GOAL")
        .unwrap_or_else(|_| format!("{} – refreshed", previous_goal));

    let marketplace = ccos.get_capability_marketplace();
    let manifests = marketplace.list_capabilities().await;
    let synthesized = manifests
        .iter()
        .filter(|m| m.id.starts_with("generated.smart_report"))
        .max_by_key(|m| &m.id)
        .cloned();

    if let Some(capability) = synthesized {
        println!(
            "{} {}",
            "Reusing synthesized capability:".bold(),
            capability.id.cyan()
        );

        let mut input_map = HashMap::new();
        input_map.insert(
            MapKey::String("topic".into()),
            Value::String(new_goal.clone()),
        );

        let result = marketplace
            .execute_capability(&capability.id, &Value::Map(input_map))
            .await?;

        if let Value::Map(map) = result {
            if let Some(Value::String(path)) = map.get(&MapKey::String("report_path".into())) {
                println!(
                    "{} {}",
                    "📄 Report delivered:".bold().green(),
                    path.cyan()
                );
            }
        }

        Ok(ApplicationMetrics {
            step_count: 1,
            reused_capability_id: Some(capability.id.clone()),
            duration_ms: start.elapsed().as_millis(),
        })
    } else {
        println!(
            "{}",
            "⚠️  No synthesized capability registered; executing baseline plan with defaults.".yellow()
        );

        let mut state = DemoRunState {
            goal: new_goal.clone(),
            answers: BTreeMap::new(),
            outputs: HashMap::new(),
            partials: Vec::new(),
            steps: Vec::new(),
        };

        for spec in scenario.inputs.values() {
            let mut answer_value = spec.default_value(&new_goal);
            if spec.key == "filters" {
                answer_value = spec.value_from_text("exclude_outliers=true;aggregate=weekly");
            }
            state.answers.insert(
                spec.key.to_string(),
                AnswerRecord {
                    key: spec.key.to_string(),
                    question: spec.prompt.to_string(),
                    rationale: spec.rationale.to_string(),
                    value: answer_value,
                    source: AnswerSource::AssumedDefault,
                },
            );
        }

        execute_plan(ccos, scenario, &mut state, debug_prompts).await?;
        Ok(ApplicationMetrics {
            step_count: state.steps.len(),
            reused_capability_id: None,
            duration_ms: start.elapsed().as_millis(),
        })
    }
}

fn print_learning_summary(metrics: &LearningMetrics) {
    println!("\n{}", "📚 Learning metrics".bold());
    println!("   • Clarifying questions: {}", metrics.clarifying_questions);
    println!("   • Steps executed: {}", metrics.step_count);
    println!("   • Partial outcomes: {}", metrics.partials.len());
    if !metrics.partials.is_empty() {
        print_partial_outcomes(&metrics.partials);
    }
}

fn print_partial_outcomes(partials: &[PartialExecutionOutcome]) {
    for outcome in partials {
        println!(
            "     - {} → {} ({})",
            outcome.step_id.cyan(),
            outcome.message,
            outcome
                .missing_inputs
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
}

fn print_application_summary(learn: &LearningMetrics, apply: &ApplicationMetrics) {
    println!("\n{}", "⚖️  Comparison".bold());
    println!(
        "   • Baseline steps: {} → reuse steps: {}",
        learn.step_count,
        apply.step_count
    );
    if let Some(id) = &apply.reused_capability_id {
        println!("   • Reused capability: {}", id.cyan());
    }
    println!("   • Reuse runtime: {} ms", apply.duration_ms);
}

// -----------------------------------------------------------------------------
// Configuration helpers
// -----------------------------------------------------------------------------

fn load_agent_config(path: &str) -> Result<AgentConfig, Box<dyn std::error::Error>> {
    let raw = fs::read_to_string(path)?;
    let ext = Path::new(path)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("toml")
        .to_ascii_lowercase();
    if ext == "toml" || ext == "tml" {
        Ok(toml::from_str(&raw)?)
    } else {
        Ok(serde_json::from_str(&raw)?)
    }
}

fn apply_llm_profile(
    config: &AgentConfig,
    profile_name: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    std::env::set_var("CCOS_ENABLE_DELEGATION", "1");

    if let Some(_) = &config.llm_profiles {
        let (profiles, _meta, _why) = expand_profiles(config);
        let chosen = profile_name
            .map(ToOwned::to_owned)
            .or_else(|| config.llm_profiles.as_ref()?.default.clone())
            .or_else(|| profiles.first().map(|p| p.name.clone()));

        if let Some(name) = chosen {
            if let Some(profile) = profiles.iter().find(|p| p.name == name) {
                apply_profile_env(profile);
            }
        } else if let Some(first) = profiles.first() {
            apply_profile_env(first);
        }
    }

    Ok(())
}

fn apply_profile_env(profile: &LlmProfile) {
    std::env::set_var("CCOS_DELEGATING_MODEL", &profile.model);
    std::env::set_var("CCOS_LLM_MODEL", &profile.model);
    std::env::set_var("CCOS_LLM_PROVIDER_HINT", &profile.provider);

    if let Some(url) = &profile.base_url {
        std::env::set_var("CCOS_LLM_BASE_URL", url);
    } else if profile.provider == "openrouter" {
        std::env::set_var("CCOS_LLM_BASE_URL", "https://openrouter.ai/api/v1");
    }

    if let Some(api_key) = profile.api_key.as_ref() {
        set_api_key(&profile.provider, api_key);
    } else if let Some(env) = &profile.api_key_env {
        if let Ok(value) = std::env::var(env) {
            set_api_key(&profile.provider, &value);
        }
    }

    match profile.provider.as_str() {
        "openai" => std::env::set_var("CCOS_LLM_PROVIDER", "openai"),
        "claude" | "anthropic" => std::env::set_var("CCOS_LLM_PROVIDER", "anthropic"),
        "openrouter" => std::env::set_var("CCOS_LLM_PROVIDER", "openrouter"),
        "local" => std::env::set_var("CCOS_LLM_PROVIDER", "local"),
        "stub" => std::env::set_var("CCOS_LLM_PROVIDER", "stub"),
        other => std::env::set_var("CCOS_LLM_PROVIDER", other),
    }
}

fn set_api_key(provider: &str, key: &str) {
    match provider {
        "openrouter" => std::env::set_var("OPENROUTER_API_KEY", key),
        "claude" | "anthropic" => std::env::set_var("ANTHROPIC_API_KEY", key),
        "gemini" => std::env::set_var("GEMINI_API_KEY", key),
        "stub" => {}
        _ => std::env::set_var("OPENAI_API_KEY", key),
    }
}

*/
