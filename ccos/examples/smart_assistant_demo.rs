// Governed smart assistant demo with recursive capability synthesis.
//
// This demo integrates the RecursiveSynthesizer to automatically generate
// missing capabilities and their dependencies when executing user goals.
//
#![allow(dead_code)]
// Key features:
// - Natural language goal → Intent → Plan → Orchestrator RTFS
// - Automatic capability discovery (Marketplace → MCP → OpenAPI → Recursive Synthesis)
// - Missing capabilities trigger recursive synthesis with dependency resolution
// - Synthesized capabilities are registered in the marketplace for reuse
//
// Previous version (without recursive synthesis) is saved as smart_assistant_demo_v1.rs

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};

use ccos::arbiter::DelegatingCognitiveEngine;
use ccos::capabilities::{MCPSessionHandler, SessionPoolManager};
use ccos::capability_marketplace::types::CapabilityManifest;
use ccos::config::types::{AgentConfig, LlmProfile};
use ccos::discovery::{
    CapabilityNeed, DiscoveryEngine, DiscoveryHints, DiscoveryResult, FoundCapability,
};
use ccos::examples_common::capability_helpers::{
    count_token_matches, minimum_token_matches, preload_discovered_capabilities,
    score_manifest_against_tokens, tokenize_identifier,
};
use ccos::intent_graph::config::IntentGraphConfig;
use ccos::synthesis::missing_capability_resolver::{
    MissingCapabilityRequest, MissingCapabilityResolver, ResolutionResult,
};
use ccos::synthesis::schema_serializer::type_expr_to_rtfs_compact;
use ccos::types::{Intent, Plan, PlanBody};
use ccos::{PlanAutoRepairOptions, CCOS};
use chrono::Utc;
use clap::{Parser, ValueEnum};
use crossterm::style::Stylize;
use once_cell::sync::Lazy;
use rtfs::ast::{Expression, Keyword, Literal, MapKey, MapTypeEntry, PrimitiveType, TypeExpr};
use rtfs::config::profile_selection::expand_profiles;
use rtfs::parser::parse_expression;
use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use rtfs::runtime::values::Value;
use serde_json::{self, Value as JsonValue};
use std::time::SystemTime;
use toml;

const GENERIC_CLASS_PREFIXES: &[&str] = &["general", "core", "default", "misc", "step", "task"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum NormalizationSource {
    CandidateHint,
    StepIdSlug,
    StepNameTokens,
    DescriptionTokens,
}

impl NormalizationSource {
    fn label(self) -> &'static str {
        match self {
            NormalizationSource::CandidateHint => "candidate hints",
            NormalizationSource::StepIdSlug => "step id slug",
            NormalizationSource::StepNameTokens => "step name tokens",
            NormalizationSource::DescriptionTokens => "description tokens",
        }
    }
}

impl fmt::Display for NormalizationSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

#[derive(Debug, Clone)]
struct NormalizationEvent {
    step_id: String,
    original_class: String,
    normalized_class: String,
    source: NormalizationSource,
}

#[derive(Debug, Clone, Default)]
struct PlanNormalizationTelemetry {
    rewrites: Vec<NormalizationEvent>,
}

impl PlanNormalizationTelemetry {
    fn record(&mut self, event: NormalizationEvent) {
        self.rewrites.push(event);
    }
}

static PLAN_NORMALIZATION_TELEMETRY: Lazy<Mutex<PlanNormalizationTelemetry>> =
    Lazy::new(|| Mutex::new(PlanNormalizationTelemetry::default()));

fn reset_plan_normalization_telemetry() {
    if let Ok(mut telemetry) = PLAN_NORMALIZATION_TELEMETRY.lock() {
        telemetry.rewrites.clear();
    }
}

fn record_normalization_event(
    step_id: &str,
    original_class: &str,
    normalized_class: &str,
    source: NormalizationSource,
) {
    if let Ok(mut telemetry) = PLAN_NORMALIZATION_TELEMETRY.lock() {
        telemetry.record(NormalizationEvent {
            step_id: step_id.to_string(),
            original_class: original_class.to_string(),
            normalized_class: normalized_class.to_string(),
            source,
        });
    }
}

fn print_normalization_telemetry() {
    let snapshot = {
        let telemetry = PLAN_NORMALIZATION_TELEMETRY
            .lock()
            .expect("normalization telemetry lock poisoned");
        telemetry.clone()
    };

    if snapshot.rewrites.is_empty() {
        return;
    }

    println!("\n{}", "📈 Capability-class normalization telemetry".bold());
    println!("   • Total rewrites: {}", snapshot.rewrites.len());

    let mut counts: BTreeMap<NormalizationSource, usize> = BTreeMap::new();
    for event in &snapshot.rewrites {
        *counts.entry(event.source).or_insert(0) += 1;
    }
    for (source, count) in counts {
        println!("   • {}: {}", source, count);
    }

    for event in snapshot.rewrites.iter().take(5) {
        println!(
            "     - Step {}: '{}' → '{}' ({})",
            event.step_id.as_str().cyan(),
            event.original_class,
            event.normalized_class,
            event.source
        );
    }
    if snapshot.rewrites.len() > 5 {
        println!(
            "     - {}",
            format!(
                "... {} additional normalization(s)",
                snapshot.rewrites.len() - 5
            )
            .dim()
        );
    }

    reset_plan_normalization_telemetry();
}

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

    /// Execute a saved plan by its plan_id instead of generating a new one
    #[arg(long)]
    execute_plan: Option<String>,

    /// Inject a known plan error before execution (for auto-repair demos)
    #[arg(long, value_enum)]
    inject_plan_error: Option<InjectPlanError>,
}

#[derive(Copy, Clone, Debug, ValueEnum)]
enum InjectPlanError {
    SimpleMapSyntax,
    ComplexStructure,
}

fn inject_plan_error_source(original: &str, fixture: InjectPlanError) -> String {
    match fixture {
        InjectPlanError::SimpleMapSyntax => {
            let mut mutated = original.to_string();
            mutated = mutated.replacen(":username \"mandubian\"", ":username = mandubian", 1);
            mutated = mutated.replacen(":projects (", ":projects = (", 1);
            mutated
        }
        InjectPlanError::ComplexStructure => {
            let mut mutated = inject_plan_error_source(original, InjectPlanError::SimpleMapSyntax);
            if let Some(pos) = mutated.rfind(')') {
                mutated.remove(pos);
            }
            mutated
        }
    }
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
    primitive_annotations: Option<JsonValue>,
}

fn canonicalize_capability_class(step: &mut ProposedStep) {
    let original = step.capability_class.trim().to_string();
    let class_lower = original.to_ascii_lowercase();
    let first_segment = class_lower
        .split(|c| c == '.' || c == ':')
        .next()
        .unwrap_or("");
    let is_generic_prefix = GENERIC_CLASS_PREFIXES.contains(&first_segment);
    let is_generic = original.is_empty() || !class_lower.contains('.') || is_generic_prefix;

    if !is_generic {
        return;
    }

    let mut source = None;

    if let Some(candidate) = step
        .candidate_capabilities
        .iter()
        .find(|cand| cand.contains('.') || cand.contains(':'))
    {
        step.capability_class = candidate.clone();
        source = Some(NormalizationSource::CandidateHint);
    } else if step.id.contains('.') || step.id.contains(':') {
        step.capability_class = step.id.clone();
        source = Some(NormalizationSource::StepIdSlug);
    } else if let Some(from_name) = canonicalize_from_text(&step.name) {
        step.capability_class = from_name;
        source = Some(NormalizationSource::StepNameTokens);
    } else if let Some(desc) = &step.description {
        if let Some(from_desc) = canonicalize_from_text(desc) {
            step.capability_class = from_desc;
            source = Some(NormalizationSource::DescriptionTokens);
        }
    }

    if step.capability_class != original {
        let src = source.unwrap_or(NormalizationSource::StepNameTokens);
        record_normalization_event(&step.id, &original, &step.capability_class, src);
        println!(
            "  {} Normalized capability class '{}' → '{}'",
            "ℹ️".blue(),
            original,
            step.capability_class
        );
    }
}

fn canonicalize_from_text(text: &str) -> Option<String> {
    let mut tokens = Vec::new();
    let mut seen = HashSet::new();

    for token in text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '.')) {
        let tk = token.trim().to_ascii_lowercase();
        if tk.is_empty() || STOPWORDS.contains(&tk.as_str()) {
            continue;
        }
        if seen.insert(tk.clone()) {
            tokens.push(tk);
        }
    }

    if tokens.len() < 2 {
        return None;
    }

    if tokens.len() > 4 {
        tokens.truncate(4);
    }

    Some(tokens.join("."))
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
async fn main() -> DemoResult<()> {
    let args = Args::parse();

    if let Err(error) = run_demo(args).await {
        eprintln!("{} {}", "✖ Demo failed:".bold().red(), error);
        std::process::exit(1);
    }

    Ok(())
}
async fn run_demo(args: Args) -> DemoResult<()> {
    let agent_config = load_agent_config(&args.config)?;
    apply_llm_profile(&agent_config, args.profile.as_deref())?;

    // Print architecture summary before initializing
    print_architecture_summary(&agent_config, args.profile.as_deref());

    // Enable file storage for plans (defaults to demo_storage/plans if not set)
    let plan_archive_path = std::env::var("CCOS_PLAN_ARCHIVE_PATH")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from("demo_storage/plans"));

    // Ensure the directory exists
    if let Err(e) = std::fs::create_dir_all(&plan_archive_path) {
        eprintln!(
            "⚠️  Warning: Failed to create plan archive directory {:?}: {}",
            plan_archive_path, e
        );
    } else {
        println!("📁 Plan archive: {}", plan_archive_path.display());
    }

    let ccos = Arc::new(
        CCOS::new_with_agent_config_and_configs_and_debug_callback(
            IntentGraphConfig::default(),
            Some(plan_archive_path),
            Some(agent_config.clone()),
            None,
        )
        .await
        .map_err(runtime_error)?,
    );

    let missing_capability_resolver = ccos.get_missing_capability_resolver();

    configure_session_pool(&ccos).await?;

    // Preload any MCP/discovered capabilities up front so discovery doesn't try to resynthesize them.
    let marketplace = ccos.get_capability_marketplace();
    
    // 1. Preload discovered (MCP-like) capabilities
    let discovered_root = std::path::Path::new("capabilities/servers/approved");
    if discovered_root.exists() {
        match preload_discovered_capabilities(&marketplace, discovered_root).await {
            Ok(count) => {
                if count > 0 {
                    println!(
                        "  {} Preloaded {} discovered capability manifest(s) before planning",
                        "✓".green(),
                        count
                    );
                }
            }
            Err(e) => eprintln!(
                "  {} Failed to preload discovered capabilities before planning: {}",
                "⚠️".yellow(),
                e
            ),
        }
    }

    // 2. Preload already synthesized capabilities (Learning Persistence)
    let generated_root = std::path::Path::new("capabilities/generated");
    if generated_root.exists() {
        match preload_discovered_capabilities(&marketplace, generated_root).await {
            Ok(count) => {
                if count > 0 {
                    println!(
                        "  {} Preloaded {} generated capability manifest(s) from previous sessions (Learning Mode)",
                        "✓".green(),
                        count
                    );
                }
            }
            Err(_) => { /* Ignore errors for generated - might be empty or invalid */ }
        }
    }

    let total_caps_after_preload = marketplace.list_capabilities().await.len();
    println!(
        "  {} Marketplace has {} capability manifest(s) registered pre-planning",
        "ℹ️".blue(),
        total_caps_after_preload
    );

    // If execute_plan is provided, load and execute it instead of generating a new plan
    if let Some(plan_id) = args.execute_plan {
        let plan_id_clone = plan_id.clone();
        println!(
            "\n{} {}",
            "🔄 Executing saved plan:".bold(),
            plan_id_clone.cyan()
        );
        println!("{}", "=".repeat(80));

        let orchestrator = Arc::clone(&ccos.orchestrator);
        match orchestrator.get_plan_by_id(&plan_id) {
            Ok(Some(plan)) => {
                println!("  ✓ Found plan: {}", plan.plan_id);
                if let Some(name) = &plan.name {
                    println!("     Name: {}", name);
                }

                // Create runtime context with parameters
                let mut context = rtfs::runtime::security::RuntimeContext::full();

                // Extract common parameters from plan metadata if available
                // For GitHub issues, add owner, repository, authentication, filter_topic
                context.add_cross_plan_param(
                    "owner".to_string(),
                    rtfs::runtime::values::Value::String("mandubian".to_string()),
                );
                context.add_cross_plan_param(
                    "repository".to_string(),
                    rtfs::runtime::values::Value::String("ccos".to_string()),
                );
                context.add_cross_plan_param(
                    "language".to_string(),
                    rtfs::runtime::values::Value::String("rtfs".to_string()),
                );
                context.add_cross_plan_param(
                    "filter_topic".to_string(),
                    rtfs::runtime::values::Value::String("rtfs".to_string()),
                );
                context.add_cross_plan_param(
                    "output-format".to_string(),
                    rtfs::runtime::values::Value::String("list".to_string()),
                );
                context.add_cross_plan_param(
                    "source".to_string(),
                    rtfs::runtime::values::Value::String("github".to_string()),
                );

                // Add authentication token if available
                if let Ok(token) = std::env::var("MCP_AUTH_TOKEN") {
                    context.add_cross_plan_param(
                        "authentication".to_string(),
                        rtfs::runtime::values::Value::String(token),
                    );
                } else if let Ok(token) = std::env::var("GITHUB_TOKEN") {
                    context.add_cross_plan_param(
                        "authentication".to_string(),
                        rtfs::runtime::values::Value::String(token),
                    );
                }

                // Execute the plan
                println!("\n{}", "🚀 Executing Plan".bold());
                println!("{}", "=".repeat(80));
                match ccos.validate_and_execute_plan(plan, &context).await {
                    Ok(exec_result) => {
                        if exec_result.success {
                            println!(
                                "\n{}",
                                "✅ Plan execution completed successfully!".bold().green()
                            );
                            println!("{}", "Result:".bold());
                            println!("{:?}", exec_result.value);
                        } else {
                            println!(
                                "\n{}",
                                "⚠️  Plan execution completed with warnings".bold().yellow()
                            );
                            if let Some(error) = exec_result.metadata.get("error") {
                                println!("Error: {:?}", error);
                            }
                            println!("Result: {:?}", exec_result.value);
                        }
                    }
                    Err(e) => {
                        println!("\n{}", "❌ Plan execution failed".bold().red());
                        println!("Error: {}", e);
                        return Err(Box::new(io::Error::new(
                            io::ErrorKind::Other,
                            format!("Plan execution failed: {}", e),
                        )));
                    }
                }
                return Ok(());
            }
            Ok(None) => {
                eprintln!("❌ Plan not found: {}", plan_id);
                return Err(Box::new(io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("Plan {} not found in archive", plan_id),
                )));
            }
            Err(e) => {
                eprintln!("❌ Failed to load plan: {}", e);
                return Err(Box::new(io::Error::new(
                    io::ErrorKind::Other,
                    format!("Failed to load plan: {}", e),
                )));
            }
        }
    }

    let delegating = ccos
        .get_delegating_engine()
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

    // Always show the full raw intent response
    println!("\n{}", "📄 Full Intent Response from LLM".bold());
    println!("{}", "─".repeat(80));
    println!("{}", raw_intent);
    println!("{}", "─".repeat(80));

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

    println!(
        "\n{}",
        "📋 Generating initial plan from intent...".bold().cyan()
    );

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
            return Err(Box::new(RuntimeError::Generic(format!(
                "❌ Arbiter returned no plan steps. Cannot proceed without a valid plan."
            )))
            .into());
        }
        Err(err) => {
            return Err(Box::new(RuntimeError::Generic(format!(
                "❌ Failed to synthesize steps:\n\n{}",
                err
            )))
            .into());
        }
    };

    let matches = match_proposed_steps(&ccos, &plan_steps).await?;
    annotate_steps_with_matches(&mut plan_steps, &matches);

    // Check for missing capabilities and trigger re-planning if needed
    let missing_count = matches
        .iter()
        .filter(|m| m.status == MatchStatus::Missing)
        .count();
    if missing_count > 0 && ccos.get_delegating_engine().is_some() {
        println!(
            "\n{} {} {}",
            "🔄".yellow().bold(),
            "Some capabilities not found:".yellow(),
            format!("({} missing)", missing_count).yellow()
        );

        // Collect discovery hints for all capabilities in the plan
        // Build a map of capability_class -> description for better rationale
        let capability_info: Vec<(String, Option<String>)> = plan_steps
            .iter()
            .map(|s| (s.capability_class.clone(), s.description.clone()))
            .collect();

        let discovery_engine = DiscoveryEngine::new_with_arbiter(
            Arc::clone(&ccos.get_capability_marketplace()),
            Arc::clone(&ccos.get_intent_graph()),
            ccos.get_delegating_engine(),
        );

        let hints = discovery_engine
            .collect_discovery_hints_with_descriptions(&capability_info)
            .await
            .map_err(|e| {
                Box::<dyn std::error::Error>::from(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Failed to collect discovery hints: {}", e),
                ))
            })?;

        if !hints.missing_capabilities.is_empty() {
            println!(
                "  Missing: {}",
                hints.missing_capabilities.join(", ").yellow()
            );
            println!(
                "  Found: {} capabilities",
                hints.found_capabilities.len().to_string().green()
            );

            // Show suggestions if available
            if !hints.suggestions.is_empty() {
                println!("\n  Suggestions:");
                for suggestion in &hints.suggestions {
                    println!("    • {}", suggestion.as_str().cyan());
                }
            }

            println!(
                "\n{}",
                "Asking LLM to replan with available capabilities...".cyan()
            );

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
            reset_plan_normalization_telemetry();
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
                    println!(
                        "  {} New plan generated with {} steps",
                        "✓".green(),
                        new_steps.len().to_string().green()
                    );
                    plan_steps = new_steps;

                    // Re-match with new plan
                    let new_matches = match_proposed_steps(&ccos, &plan_steps).await?;
                    annotate_steps_with_matches(&mut plan_steps, &new_matches);

                    build_register_and_execute_plan(
                        &ccos,
                        missing_capability_resolver.clone(),
                        &agent_config,
                        &args,
                        &goal,
                        &intent,
                        &answers,
                        &plan_steps,
                        &new_matches,
                    )
                    .await?;
                    return Ok(());
                } else {
                    println!("  {} Re-plan failed to generate valid steps, proceeding with original plan", "⚠️".yellow());
                }
            }
        }
    }

    build_register_and_execute_plan(
        &ccos,
        missing_capability_resolver,
        &agent_config,
        &args,
        &goal,
        &intent,
        &answers,
        &plan_steps,
        &matches,
    )
    .await?;
    Ok(())
}

type DemoResult<T> = Result<T, Box<dyn std::error::Error>>;

async fn configure_session_pool(ccos: &Arc<CCOS>) -> DemoResult<()> {
    let mut session_pool = SessionPoolManager::new();
    session_pool.register_handler("mcp", Arc::new(MCPSessionHandler::new()));
    let session_pool = Arc::new(session_pool);

    let marketplace = ccos.get_capability_marketplace();
    marketplace.set_session_pool(session_pool).await;
    Ok(())
}

fn runtime_error(err: RuntimeError) -> Box<dyn std::error::Error> {
    Box::new(err)
}

/// Print architecture summary and configuration
fn print_architecture_summary(config: &AgentConfig, profile_name: Option<&str>) {
    println!("\n{}", "═".repeat(80).bold());
    println!(
        "{}",
        "🏗️  CCOS Smart Assistant - Architecture Summary"
            .bold()
            .cyan()
    );
    println!("{}", "═".repeat(80).bold());

    println!("\n  {} Architecture Overview", "📋".bold());
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
    println!(
        "     • {}: Governs intent extraction, plan generation, execution",
        "DelegatingCognitiveEngine".cyan()
    );
    println!(
        "     • {}: Finds/synthesizes missing capabilities",
        "DiscoveryEngine".cyan()
    );
    println!(
        "     • {}: Recursively generates missing capabilities",
        "RecursiveSynthesizer".cyan()
    );
    println!(
        "     • {}: Manages capability registration and search",
        "CapabilityMarketplace".cyan()
    );
    println!(
        "     • {}: Tracks intent relationships and dependencies",
        "IntentGraph".cyan()
    );

    // Show discovery / search configuration
    let discovery = &config.discovery;
    println!("\n  {} Discovery/Search Settings:", "3.".bold());
    if discovery.use_embeddings {
        let model = discovery
            .embedding_model
            .as_deref()
            .or(discovery.local_embedding_model.as_deref())
            .unwrap_or("unspecified model");
        println!(
            "     • Embedding search: {} ({})",
            "enabled".green(),
            model.cyan()
        );
    } else {
        println!(
            "     • Embedding search: {} (keyword + schema heuristics)",
            "disabled".yellow()
        );
    }
    println!("     • Match threshold: {:.2}", discovery.match_threshold);
    println!(
        "     • Action verb weight / threshold: {:.2} / {:.2}",
        discovery.action_verb_weight, discovery.action_verb_threshold
    );
    println!(
        "     • Capability class weight: {:.2}",
        discovery.capability_class_weight
    );

    // Show LLM profile
    if let Some(llm_profiles) = &config.llm_profiles {
        // `expand_profiles` lives in RTFS and expects RTFS config types.
        // CCOS config types are structurally identical, so convert via serde.
        let rtfs_config: rtfs::config::types::AgentConfig = serde_json::from_value(
            serde_json::to_value(config).expect("Failed to serialize AgentConfig"),
        )
        .expect("Failed to deserialize AgentConfig");
        let (rtfs_profiles, _meta, _why) = expand_profiles(&rtfs_config);
        let profiles: Vec<LlmProfile> = rtfs_profiles
            .into_iter()
            .map(|p| {
                serde_json::from_value(
                    serde_json::to_value(&p).expect("Failed to serialize LlmProfile"),
                )
                .expect("Failed to deserialize LlmProfile")
            })
            .collect();
        let chosen = profile_name
            .map(|s| s.to_string())
            .or_else(|| llm_profiles.default.clone())
            .or_else(|| profiles.first().map(|p| p.name.clone()));

        if let Some(name) = chosen {
            if let Some(profile) = profiles.iter().find(|p| p.name == name) {
                println!("\n  {} LLM Configuration:", "4.".bold());
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
fn print_llm_provider_info(delegating: &DelegatingCognitiveEngine) {
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
        Err(Box::<dyn std::error::Error>::from(
            io::Error::new(io::ErrorKind::InvalidInput, "Goal cannot be empty"),
        ))
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
        let rtfs_config: rtfs::config::types::AgentConfig = serde_json::from_value(
            serde_json::to_value(config).expect("Failed to serialize AgentConfig"),
        )
        .expect("Failed to deserialize AgentConfig");
        let (rtfs_profiles, _meta, _why) = expand_profiles(&rtfs_config);
        let profiles: Vec<LlmProfile> = rtfs_profiles
            .into_iter()
            .map(|p| {
                serde_json::from_value(
                    serde_json::to_value(&p).expect("Failed to serialize LlmProfile"),
                )
                .expect("Failed to deserialize LlmProfile")
            })
            .collect();
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
        }
        "local" => std::env::set_var("CCOS_LLM_PROVIDER", "local"),
        "stub" => {
            eprintln!("⚠️  WARNING: Using stub LLM provider (testing only - not realistic)");
            eprintln!(
                "   Set a real provider in agent_config.toml or use --profile with a real provider"
            );
            std::env::set_var("CCOS_LLM_PROVIDER", "stub");
            std::env::set_var("CCOS_ALLOW_STUB_PROVIDER", "1"); // Allow stub if explicitly requested
        }
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
    delegating: &DelegatingCognitiveEngine,
    goal: &str,
    intent: &Intent,
    _debug: bool,
) -> DemoResult<Vec<ClarifyingQuestion>> {
    let mut prompt = String::with_capacity(2048);
    prompt.push_str("You are the CCOS delegating arbiter refining a user goal.\n");
    prompt.push_str(
        "Your task is to generate clarifying questions to better understand the user's goal.\n\n",
    );
    prompt.push_str("RESPONSE FORMAT: You MUST respond ONLY with an RTFS vector of maps. NO prose, NO explanations, NO code fences.\n\n");
    prompt.push_str("Each map in the vector must have these keys:\n");
    prompt.push_str("  :id - unique identifier (e.g., \"q1\", \"q2\")\n");
    prompt.push_str("  :key - variable name for the answer (e.g., \"target\", \"repository\")\n");
    prompt.push_str("  :prompt - the question text to ask the user\n");
    prompt.push_str("  :rationale - why this question is needed (for audit purposes)\n");
    prompt.push_str("  :answer-kind - one of :text, :list, :number, or :boolean\n");
    prompt.push_str("  :required - :true or :false\n");
    prompt
        .push_str("  :default-answer - optional, a default value if question is not required\n\n");
    prompt.push_str("EXAMPLE FORMAT:\n");
    prompt.push_str("[{:id \"q1\" :key \"repository\" :prompt \"Which GitHub repository should we search?\" :rationale \"Need to know the target repository to list issues\" :answer-kind :text :required true} {:id \"q2\" :key \"filter_term\" :prompt \"What term should we filter issues by?\" :rationale \"Need to know what to search for in issue content\" :answer-kind :text :required true :default-answer \"rtfs\"}]\n\n");
    prompt.push_str("IMPORTANT:\n");
    prompt.push_str(
        "- If the goal is already clear and no questions are needed, return an empty vector: []\n",
    );
    prompt.push_str("- If questions are needed, return at least one question in the vector\n");
    prompt.push_str("- Always include :rationale for each question\n");
    prompt.push_str("- Use RTFS syntax: keywords start with :, strings are in quotes, booleans are :true/:false\n\n");
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
    prompt.push_str("----------------\n\n");
    prompt.push_str("Now generate the clarifying questions as an RTFS vector. Respond ONLY with the vector, nothing else:");

    // Always show the prompt sent to LLM
    println!("\n{}", "❓ Generating Clarifying Questions".bold());
    println!("{}", "─".repeat(80));
    println!("{}", "📤 Prompt sent to LLM:".bold());
    println!("{}", "─".repeat(80));
    println!("{}", prompt);
    println!("{}", "─".repeat(80));

    let response = delegating
        .generate_raw_text(&prompt)
        .await
        .map_err(runtime_error)?;

    // Always show the response received
    println!("\n{}", "📥 Response received from LLM:".bold());
    println!("{}", "─".repeat(80));
    if response.trim().is_empty() {
        println!("{}", "[EMPTY RESPONSE]".red().bold());
    } else {
        println!("{}", response);
    }
    println!("{}", "─".repeat(80));

    let parsed_value = parse_clarifying_response(&response).map_err(|e| {
        // Enhanced error message with full context
        let error_msg = format!(
            "❌ Failed to parse clarifying questions response\n\n\
            📥 LLM Response:\n\
            ┌─────────────────────────────────────────────────────────\n\
            {}\n\
            └─────────────────────────────────────────────────────────\n\n\
            🔍 Parsing error: {}\n\n\
            💡 The LLM should respond with an RTFS vector of maps or an empty vector [] if no questions are needed.",
            response,
            e
        );
        runtime_error(RuntimeError::Generic(error_msg))
    })?;

    let items = extract_question_items(&parsed_value).ok_or_else(|| {
        let error_msg = format!(
            "❌ Clarifying question response did not contain any recognizable question list\n\n\
            📥 LLM Response:\n\
            ┌─────────────────────────────────────────────────────────\n\
            {}\n\
            └─────────────────────────────────────────────────────────\n\n\
            💡 Expected: An RTFS vector of maps or an empty vector []",
            response
        );
        runtime_error(RuntimeError::Generic(error_msg))
    })?;

    let mut questions = Vec::with_capacity(items.len());
    let mut skipped_items = Vec::new();
    for (index, item) in items.into_iter().enumerate() {
        if let Some(question) = value_to_question(&item) {
            questions.push(question);
        } else if let Some(question) = question_from_free_form(&item, index) {
            questions.push(question);
        } else {
            skipped_items.push((index, format!("{:?}", item)));
        }
    }

    if !skipped_items.is_empty() {
        println!(
            "\n{}",
            "⚠️  Warning: Some items from LLM response could not be parsed as questions:".yellow()
        );
        for (idx, item_preview) in &skipped_items {
            let preview = if item_preview.len() > 100 {
                format!("{}...", &item_preview[..100])
            } else {
                item_preview.clone()
            };
            println!("  • Item {}: {}", idx, preview);
        }
    }

    if questions.is_empty() {
        // Check if the response was an empty vector (which is valid)
        if response.trim() == "[]" || response.trim().is_empty() {
            println!(
                "\n{}",
                "ℹ️  No clarifying questions needed - goal is already clear".dim()
            );
            return Ok(Vec::new());
        }
        let error_msg = format!(
            "❌ No clarifying questions parsed from response\n\n\
            📥 LLM Response:\n\
            ┌─────────────────────────────────────────────────────────\n\
            {}\n\
            └─────────────────────────────────────────────────────────\n\n\
            💡 If no questions are needed, the LLM should return an empty vector: []",
            response
        );
        Err(Box::<dyn std::error::Error>::from(
            RuntimeError::Generic(error_msg),
        ))
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

    // Try RTFS parsing first
    match parse_expression(&normalized_for_rtfs) {
        Ok(expr) => Ok(expression_to_value(&expr)),
        Err(_rtfs_err) => {
            // Try JSON as fallback
            match serde_json::from_str::<serde_json::Value>(&sanitized) {
                Ok(json) => Ok(json_to_demo_value(&json)),
                Err(_json_err) => {
                    // Generate user-friendly error message
                    let _response_preview = if sanitized.len() > 300 {
                        format!(
                            "{}...\n[truncated, total length: {} chars]",
                            &sanitized[..300],
                            sanitized.len()
                        )
                    } else {
                        sanitized.clone()
                    };

                    // Show first few lines for context
                    let response_lines: Vec<&str> = sanitized.lines().collect();
                    let line_preview = if response_lines.len() > 8 {
                        format!(
                            "{}\n... [{} more lines]",
                            response_lines[..8].join("\n"),
                            response_lines.len() - 8
                        )
                    } else {
                        sanitized.clone()
                    };

                    Err(RuntimeError::Generic(format!(
                        "❌ Failed to parse LLM response as clarifying questions\n\n\
                        📋 Expected format: An RTFS vector of maps, like:\n\
                        [{{:id \"q1\" :key \"target\" :prompt \"What should we target?\" :rationale \"...\" :answer-kind :text :required true}}]\n\n\
                        📥 Received response:\n\
                        ┌─────────────────────────────────────────────────────────\n\
                        {}\n\
                        └─────────────────────────────────────────────────────────\n\n\
                        💡 Common issues:\n\
                        • Response contains explanatory text before/after the data structure\n\
                        • Missing required fields (:id, :key, :prompt, :rationale, :answer-kind, :required)\n\
                        • Invalid RTFS syntax (unclosed brackets, mismatched quotes, etc.)\n\
                        • Response is empty or contains only whitespace\n\n\
                        🔧 Tip: The LLM should respond ONLY with the data structure, no prose.",
                        line_preview
                    )))
                }
            }
        }
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

        reset_plan_normalization_telemetry();
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
                primitive_annotations: None,
            },
            capability_id: "travel.flights.search".to_string(),
            resolution_strategy: ResolutionStrategy::Found,
            input_bindings: HashMap::new(),
            output_bindings: HashMap::new(),
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
                primitive_annotations: None,
            },
            capability_id: "travel.lodging.reserve".to_string(),
            resolution_strategy: ResolutionStrategy::Found,
            input_bindings: HashMap::new(),
            output_bindings: HashMap::new(),
        };

        let rtfs =
            generate_orchestrator_capability("Book trip", &[step1, step2], "orchestrator.test")
                .expect("rtfs generation");

        // Must contain schemas
        assert!(
            rtfs.contains(":input-schema"),
            "missing input-schema: {}",
            rtfs
        );
        assert!(
            rtfs.contains(":output-schema"),
            "missing output-schema: {}",
            rtfs
        );

        // No legacy $ prefix
        assert!(
            !rtfs.contains(":$"),
            "contains legacy $ variable syntax: {}",
            rtfs
        );

        // Capabilities required vector present with both caps
        assert!(
            rtfs.contains(
                ":capabilities-required [\"travel.flights.search\" \"travel.lodging.reserve\"]"
            ) || rtfs.contains(
                ":capabilities-required [\"travel.lodging.reserve\" \"travel.flights.search\"]"
            ),
            "capabilities-required vector missing or incomplete: {}",
            rtfs
        );

        // Arguments passed as map
        assert!(rtfs.contains("(call :travel.flights.search {"));
        assert!(rtfs.contains(":origin origin"));
        assert!(rtfs.contains(":destination destination"));
        assert!(rtfs.contains(":dates dates"));

        // Output schema should reflect union of all step outputs
        assert!(
            rtfs.contains(":flight_options :any"),
            "output-schema missing flight_options: {}",
            rtfs
        );
        assert!(
            rtfs.contains(":reservation :any"),
            "output-schema missing reservation: {}",
            rtfs
        );

        // Body should bind steps and compose final map using get
        assert!(
            rtfs.contains("(let ["),
            "plan should bind step results with let: {}",
            rtfs
        );
        assert!(
            rtfs.contains("(get step_1 :reservation"),
            "final composition should reference step outputs: {}",
            rtfs
        );
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
                primitive_annotations: None,
            },
            capability_id: "planning.preferences.aggregate".into(),
            resolution_strategy: ResolutionStrategy::Found,
            input_bindings: HashMap::new(),
            output_bindings: HashMap::new(),
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
                primitive_annotations: None,
            },
            capability_id: "travel.activities.plan".into(),
            resolution_strategy: ResolutionStrategy::Found,
            input_bindings: HashMap::new(),
            output_bindings: HashMap::new(),
        };

        let rtfs = generate_orchestrator_capability("Trip", &[s1, s2], "orchestrator.trip")
            .expect("generate");

        // Step 2 should wire :prefs from step_0 output; destination remains a free input
        assert!(
            rtfs.contains(":prefs (get step_0 :prefs)"),
            "prefs should be wired from previous step: {}",
            rtfs
        );
        assert!(
            rtfs.contains(":destination destination"),
            "destination should remain a free symbol input: {}",
            rtfs
        );

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
    delegating: &DelegatingCognitiveEngine,
    goal: &str,
    intent: &Intent,
    collected_answers: &[AnswerRecord],
    question: &ClarifyingQuestion,
    debug: bool,
) -> DemoResult<AnswerRecord> {
    let mut prompt = String::new();
    prompt.push_str(
        "You are answering clarifying questions for a smart assistant based on a user's goal.\n",
    );
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
    delegating: &DelegatingCognitiveEngine,
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

            let answer =
                auto_answer_with_llm(delegating, goal, intent, &collected, question, debug).await?;
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
    delegating: &DelegatingCognitiveEngine,
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
    prompt.push_str(
        "The :capability-class MUST be a fully-qualified identifier (e.g. \"github.issues.list\") with a namespace prefix; never emit generic labels such as \"github\" or \"core\".\n",
    );
    prompt.push_str(
        "If you reference a capability shown in the snapshot, reuse its id exactly; otherwise derive a specific, dotted capability-class that reflects the action.\n",
    );
    prompt.push_str("IMPORTANT: Focus on the GOAL and INTENT below. Generate plan steps that directly address the goal.\n");
    prompt.push_str("If the marketplace snapshot below contains capabilities, use them ONLY if they are relevant to the goal.\n");
    prompt.push_str("If the marketplace snapshot is empty or contains only irrelevant examples, generate steps based on the goal alone.\n");
    prompt.push_str(
        "Do NOT try to force-fit irrelevant capabilities from the snapshot into your plan.\n\n",
    );
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
        prompt
            .push_str("--- Available capabilities (use ONLY if relevant to the goal above) ---\n");
        prompt.push_str("NOTE: These are example capabilities. Only use them if they directly help achieve the goal.\n");
        prompt.push_str(
            "If none of these capabilities are relevant, generate steps based on the goal alone.\n",
        );
        for spec in capabilities {
            prompt.push_str(&format!(
                "  {} -> {} (inputs: [{}], outputs: [{}])\n",
                spec.id,
                spec.description,
                spec.required_inputs.join(", "),
                spec.expected_outputs.join(", ")
            ));
        }
    } else {
        prompt.push_str("--- Available capabilities ---\n");
        prompt.push_str("  (No specific capabilities provided - generate steps based on the goal and intent above)\n");
    }
    prompt.push_str("----------------\n");
    prompt.push_str("Generate plan steps that directly address the goal. Respond only with the RTFS vector of step maps.");

    // Always show the full prompt sent to LLM
    println!("\n{}", "📋 Generating Plan Steps".bold());
    println!("{}", "─".repeat(80));
    println!("{}", "📤 Prompt sent to LLM:".bold());
    println!("{}", "─".repeat(80));
    println!("{}", prompt);
    println!("{}", "─".repeat(80));

    let response = delegating.generate_raw_text(&prompt).await.map_err(|e| {
        // Enhanced error message for LLM generation failure
        let error_msg = format!(
            "❌ Failed to generate plan steps from LLM\n\n\
                📤 Prompt sent:\n\
                ┌─────────────────────────────────────────────────────────\n\
                {}\n\
                └─────────────────────────────────────────────────────────\n\n\
                🔍 Error: {}\n\n\
                💡 This could be due to:\n\
                • LLM API connection issues\n\
                • Rate limiting or quota exceeded\n\
                • Invalid API key or authentication failure",
            prompt, e
        );
        runtime_error(RuntimeError::Generic(error_msg))
    })?;

    // Always show the full response received
    println!("\n{}", "📥 Response received from LLM:".bold());
    println!("{}", "─".repeat(80));
    if response.trim().is_empty() {
        println!("{}", "[EMPTY RESPONSE]".red().bold());
        println!(
            "\n{}",
            "⚠️  WARNING: LLM returned an empty response!"
                .yellow()
                .bold()
        );
        println!("   This usually means the response was truncated due to token limits.");
        println!("   Current max_tokens setting: Check CCOS_LLM_MAX_TOKENS environment variable");
        println!("   Solution: Set CCOS_LLM_MAX_TOKENS to a higher value (e.g., 4096 or 8192)");
        println!("   Example: export CCOS_LLM_MAX_TOKENS=4096");
    } else {
        println!("{}", response);
    }
    println!("{}", "─".repeat(80));

    reset_plan_normalization_telemetry();
    let mut parsed_value = parse_plan_steps_response(&response).map_err(|e| {
        // The error from parse_plan_steps_response already includes a user-friendly message
        // with the full response, so we just need to convert it to Box<dyn std::error::Error + Send + Sync>
        runtime_error(e)
    })?;
    if let Value::Map(map) = &parsed_value {
        if let Some(Value::Vector(steps)) = map_get(map, "steps") {
            parsed_value = Value::Vector(steps.clone());
        }
    }

    match parsed_value {
        Value::Vector(items) => {
            let total_items = items.len();
            let mut steps = Vec::with_capacity(items.len());
            let mut skipped_items = Vec::new();

            for (index, item) in items.into_iter().enumerate() {
                if let Some(step) = value_to_step(&item) {
                    steps.push(step);
                } else if let Some(step) = step_from_free_form(&item, index) {
                    steps.push(step);
                } else {
                    // Item failed to parse - record it for reporting
                    skipped_items.push((index + 1, format!("{:?}", item)));
                }
            }

            // Warn if some items were skipped
            if !skipped_items.is_empty() && !debug {
                eprintln!(
                    "  ⚠️  Warning: {} item(s) from LLM response could not be parsed as plan steps:",
                    skipped_items.len()
                );
                for (idx, item_preview) in &skipped_items {
                    let preview = if item_preview.len() > 100 {
                        format!("{}...", &item_preview[..100])
                    } else {
                        item_preview.clone()
                    };
                    eprintln!("    • Item {}: {}", idx, preview);
                }
            }

            if steps.is_empty() {
                Err(Box::<dyn std::error::Error>::from(
                    RuntimeError::Generic(format!(
                        "No steps parsed from arbiter response ({} items total, all failed to parse)",
                        total_items
                    )),
                ))
            } else {
                if !debug {
                    if skipped_items.is_empty() {
                        println!(
                            "  ✓ Generated {} plan step(s) from LLM response:",
                            steps.len()
                        );
                    } else {
                        println!(
                            "  ✓ Generated {} plan step(s) from LLM response ({} item(s) skipped):",
                            steps.len(),
                            skipped_items.len()
                        );
                    }
                    for (i, step) in steps.iter().enumerate() {
                        println!("    {}. {} ({})", i + 1, step.name, step.capability_class);
                    }
                    // Always show full LLM response for transparency
                    println!("\n  📄 Full LLM plan generation response:");
                    println!("  {}", "─".repeat(78));
                    for line in response.lines() {
                        println!("  {}", line);
                    }
                    println!("  {}", "─".repeat(78));
                }
                Ok(steps)
            }
        }
        other => Err(Box::<dyn std::error::Error>::from(
            RuntimeError::Generic(format!(
                "Plan step response was not a vector: {}",
                format_value(&other)
            )),
        )),
    }
}

fn parse_plan_steps_response(response: &str) -> Result<Value, RuntimeError> {
    let sanitized = strip_code_fences(response);
    let normalized_for_rtfs = strip_commas_outside_strings(&sanitized);

    // Try RTFS parsing first
    match parse_expression(&normalized_for_rtfs) {
        Ok(expr) => Ok(expression_to_value(&expr)),
        Err(rtfs_err) => {
            // Try JSON as fallback
            match serde_json::from_str::<serde_json::Value>(&sanitized) {
                Ok(json) => Ok(json_to_demo_value(&json)),
                Err(json_err) => {
                    // Generate user-friendly error message with full response
                    let rtfs_error_msg = format!("{:?}", rtfs_err);
                    let json_error_msg = format!("{}", json_err);

                    Err(RuntimeError::Generic(format!(
                        "❌ Failed to parse LLM response as plan steps\n\n\
                        📋 Expected format: An RTFS vector of maps, like:\n\
                        [{{:id \"step-1\" :name \"Step Name\" :capability-class \"cap.id\" :required-inputs [...] :expected-outputs [...] :description \"...\"}}]\n\n\
                        Or JSON format:\n\
                        [{{\"id\": \"step-1\", \"name\": \"Step Name\", \"capability-class\": \"cap.id\", \"required-inputs\": [], \"expected-outputs\": [], \"description\": \"...\"}}]\n\n\
                        📥 Received response (full):\n\
                        ┌─────────────────────────────────────────────────────────\n\
                        {}\n\
                        └─────────────────────────────────────────────────────────\n\n\
                        🔍 Parsing errors:\n\
                        • RTFS: {}\n\
                        • JSON: {}\n\n\
                        💡 Common issues:\n\
                        • Response is truncated or incomplete (check LLM token limits)\n\
                        • Response contains explanatory text before/after the data structure\n\
                        • Missing required fields (:id, :name, :capability-class, :required-inputs, :expected-outputs, :description)\n\
                        • Invalid RTFS syntax (unclosed brackets, mismatched quotes, etc.)\n\
                        • Invalid JSON syntax (missing quotes, commas, brackets)\n\
                        • Response is empty or contains only whitespace\n\n\
                        🔧 Tip: The LLM should respond ONLY with the data structure, no prose.",
                        sanitized,
                        rtfs_error_msg,
                        json_error_msg
                    )))
                }
            }
        }
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
    let primitive_annotations = map_get(map, "primitive_annotations")
        .or_else(|| map_get(map, "primitive"))
        .cloned()
        .and_then(|v| serde_json::to_value(&v).ok());

    let mut step = ProposedStep {
        id,
        name,
        capability_class,
        candidate_capabilities,
        required_inputs,
        expected_outputs,
        description,
        primitive_annotations,
    };

    canonicalize_capability_class(&mut step);

    Some(step)
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
        primitive_annotations: None,
    })
}

fn json_to_rtfs_value(json: &JsonValue) -> Value {
    match json {
        JsonValue::Null => Value::Nil,
        JsonValue::Bool(b) => Value::Boolean(*b),
        JsonValue::Number(n) => {
            if let Some(i) = n.as_i64() {
                Value::Integer(i)
            } else if let Some(f) = n.as_f64() {
                Value::Float(f)
            } else {
                Value::Nil
            }
        }
        JsonValue::String(s) => Value::String(s.clone()),
        JsonValue::Array(items) => Value::Vector(items.iter().map(json_to_rtfs_value).collect()),
        JsonValue::Object(map) => {
            let mut rtfs_map = HashMap::new();
            for (k, v) in map {
                rtfs_map.insert(MapKey::String(k.clone()), json_to_rtfs_value(v));
            }
            Value::Map(rtfs_map)
        }
    }
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
    let functional_verbs = [
        "list", "get", "retrieve", "fetch", "search", "find", "create", "update", "delete",
        "format", "process", "analyze",
    ];

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

fn build_needs_capabilities(steps: &[ProposedStep]) -> Value {
    let entries: Vec<Value> = steps
        .iter()
        .map(|step| {
            let rationale = step.description.clone().unwrap_or_else(|| {
                step_name_to_functional_description(&step.name, &step.capability_class)
            });
            let _inferred_need = CapabilityNeed::new(
                step.capability_class.clone(),
                step.required_inputs.clone(),
                step.expected_outputs.clone(),
                rationale,
            );
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
            if let Some(annotations_json) = step.primitive_annotations.clone() {
                map.insert(
                    MapKey::String("primitive_annotations".into()),
                    json_to_rtfs_value(&annotations_json),
                );
            }
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
    input_bindings: HashMap<String, String>,
    output_bindings: HashMap<String, OutputBinding>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
enum ResolutionStrategy {
    Found,
    Stubbed,
    Synthesized,
}

#[derive(Debug, Clone)]
enum OutputBinding {
    MapKey(String),
    EntireValue,
}

/// Build a re-plan prompt with discovery hints
fn build_replan_prompt(goal: &str, intent: &Intent, hints: &DiscoveryHints) -> String {
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
    prompt.push_str("CRITICAL: You MUST preserve all original requirements from the goal, even if some capabilities are missing.\n");
    prompt.push_str("Strategies to preserve requirements:\n");
    prompt.push_str("  1. Use capability parameters (e.g., if 'github.issues.list' supports 'labels', 'state', or 'q' query parameters, use them for filtering)\n");
    prompt.push_str("  2. If filtering/formatting/display operations are needed but missing, you can still include them in the plan - they will be synthesized locally\n");
    prompt.push_str("  3. Combine available capabilities creatively to achieve the goal\n");
    prompt.push_str("\nExample: If the goal requires 'filter issues by RTFS language' and filtering capability is missing:\n");
    prompt.push_str("  - Option 1: Use 'github.issues.list' with 'labels' parameter if RTFS-related issues have labels\n");
    prompt.push_str("  - Option 2: Use 'github.issues.list' with 'q' query parameter to search for 'RTFS' in titles/bodies\n");
    prompt.push_str("  - Option 3: Still include a filtering step - it will be synthesized as a local operation\n");
    prompt.push_str("\nRespond ONLY with an RTFS vector where each element is a map describing a proposed capability step.\n");
    prompt.push_str("Each map must include :id :name :capability-class :required-inputs (vector of strings) :expected-outputs (vector of strings) and optional :candidate-capabilities (vector of capability ids) :description.\n");
    prompt.push_str(
        "The :capability-class must be fully-qualified (include the provider namespace, e.g. \"github.issues.list\"); do not emit generic labels such as \"github\".\n",
    );
    prompt.push_str("When specifying capability calls, use the exact capability IDs from the 'Available Capabilities' section above.\n");
    prompt.push_str("Include parameter values in :required-inputs when they are known (e.g., if filtering is needed, specify the parameter name).\n");

    prompt
}

/// Create an LLM-friendly explanation of a parser/RTFS error with actionable guidance.
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
    missing_capability_resolver: Option<Arc<MissingCapabilityResolver>>,
    steps: &[ProposedStep],
    matches: &[CapabilityMatch],
    interactive: bool,
) -> DemoResult<Vec<ResolvedStep>> {
    let mut resolved = Vec::with_capacity(steps.len());
    let marketplace = ccos.get_capability_marketplace();
    let intent_graph = ccos.get_intent_graph();
    let delegating_arbiter = ccos.get_delegating_engine();
    let resolver_ref = missing_capability_resolver.as_ref();
    let manifests = marketplace.list_capabilities().await;

    for step in steps {
        if let Some(manifest) = find_manifest_for_step(step, &manifests) {
            let input_bindings = compute_input_bindings_for_step(step, Some(&manifest));
            let output_bindings = compute_output_bindings_for_step(step, Some(&manifest));
            resolved.push(ResolvedStep {
                original: step.clone(),
                capability_id: manifest.id.clone(),
                resolution_strategy: ResolutionStrategy::Found,
                input_bindings,
                output_bindings,
            });
            continue;
        }

        println!(
            "  [resolver] Step '{}' ({}) not initially matched, checking prior discovery records",
            step.name, step.capability_class
        );

        // Check if already matched (found in marketplace or synthesized)
        if let Some(match_record) = matches.iter().find(|m| m.step_id == step.id) {
            if let Some(cap_id) = &match_record.matched_capability {
                // Check if it was synthesized based on the note
                let strategy = if match_record
                    .note
                    .as_ref()
                    .map(|n| n.contains("Synthesized"))
                    .unwrap_or(false)
                {
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

                let manifest = fetch_manifest_for_step(&marketplace, cap_id, step).await;
                let input_bindings = compute_input_bindings_for_step(step, manifest.as_ref());
                let output_bindings = compute_output_bindings_for_step(step, manifest.as_ref());
                resolved.push(ResolvedStep {
                    original: step.clone(),
                    capability_id: cap_id.clone(),
                    resolution_strategy: strategy,
                    input_bindings,
                    output_bindings,
                });
                continue;
            }
        }

        if let Some(resolver) = resolver_ref {
            let mut context_map = HashMap::new();
            context_map.insert("step_id".to_string(), step.id.clone());
            context_map.insert("step_name".to_string(), step.name.clone());
            context_map.insert(
                "capability_class".to_string(),
                step.capability_class.clone(),
            );
            if let Some(desc) = &step.description {
                context_map.insert("step_description".to_string(), desc.clone());
            }
            context_map.insert("source".to_string(), "smart_assistant_demo".to_string());
            if !step.required_inputs.is_empty() {
                context_map.insert(
                    "required_inputs".to_string(),
                    step.required_inputs.join(","),
                );
            }
            if !step.expected_outputs.is_empty() {
                context_map.insert(
                    "expected_outputs".to_string(),
                    step.expected_outputs.join(","),
                );
            }

            let request = MissingCapabilityRequest {
                capability_id: step.capability_class.clone(),
                arguments: Vec::new(),
                context: context_map,
                requested_at: SystemTime::now(),
                attempt_count: 0,
            };

            match resolver
                .resolve_capability(&request)
                .await
                .map_err(runtime_error)?
            {
                ResolutionResult::Resolved {
                    capability_id,
                    resolution_method,
                    ..
                } => {
                    if let Some(manifest) =
                        fetch_manifest_for_step(&marketplace, &capability_id, step).await
                    {
                        let input_bindings = compute_input_bindings_for_step(step, Some(&manifest));
                        let output_bindings =
                            compute_output_bindings_for_step(step, Some(&manifest));
                        resolved.push(ResolvedStep {
                            original: step.clone(),
                            capability_id: manifest.id.clone(),
                            resolution_strategy: ResolutionStrategy::Found,
                            input_bindings,
                            output_bindings,
                        });
                        println!(
                            "  ✅ [resolver:{}] Resolved '{}' as '{}'",
                            resolution_method, step.capability_class, manifest.id
                        );
                        continue;
                    } else if let Some(manifest) = marketplace.get_capability(&capability_id).await
                    {
                        let input_bindings = compute_input_bindings_for_step(step, Some(&manifest));
                        let output_bindings =
                            compute_output_bindings_for_step(step, Some(&manifest));
                        resolved.push(ResolvedStep {
                            original: step.clone(),
                            capability_id: manifest.id.clone(),
                            resolution_strategy: ResolutionStrategy::Found,
                            input_bindings,
                            output_bindings,
                        });
                        println!(
                            "  ✅ [resolver:{}] Registered '{}' for '{}'",
                            resolution_method, manifest.id, step.capability_class
                        );
                        continue;
                    } else {
                        eprintln!(
                            "  ⚠️  Resolver reported '{}' resolved via {}, but manifest not found in marketplace",
                            capability_id, resolution_method
                        );
                    }
                }
                ResolutionResult::Failed { reason, .. } => {
                    eprintln!(
                        "  ⚠️  Resolver failed for {}: {}",
                        step.capability_class, reason
                    );
                }
                ResolutionResult::PermanentlyFailed { reason, .. } => {
                    eprintln!(
                        "  ❌  Resolver permanently failed for {}: {}",
                        step.capability_class, reason
                    );
                }
            }
        }

        // Not found in marketplace - try recursive synthesis
        if delegating_arbiter.is_some() {
            println!(
                "{} {}",
                "🔄 Attempting recursive synthesis for:".cyan(),
                step.capability_class.as_str().bold()
            );

            let capability_class = derive_capability_class_hint(step);

            // Generate a more descriptive rationale that will match better with capability descriptions
            // Use step name, description, or construct a functional description from the step
            let rationale = if let Some(ref desc) = step.description {
                // If we have a description, use it (it's already functional)
                desc.clone()
            } else {
                // Otherwise, convert step name to a functional description
                // e.g., "List GitHub Repository Issues" -> "List issues in a GitHub repository"
                // This works better for semantic matching than "Need for step: X"
                let functional_desc =
                    step_name_to_functional_description(&step.name, &capability_class);
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
                ccos.get_delegating_engine(),
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
                    println!(
                        "  [resolver] Discovered manifest {} for step '{}' ({})",
                        cap_id, step.name, step.capability_class
                    );

                    // Test the synthesized capability with dummy data
                    // TODO: Re-implement test_and_correct_capability function
                    // if let Some(delegating) = &delegating_arbiter {
                    //     if let Err(e) =
                    //         test_and_correct_capability(ccos, delegating, &manifest, &step).await
                    //     {
                    //         eprintln!(
                    //             "{} {} {}",
                    //             "⚠️  Capability testing/correction failed:".yellow(),
                    //             e,
                    //             "(proceeding with synthesized version)".dim()
                    //         );
                    //     }
                    // }

                    let input_bindings = compute_input_bindings_for_step(step, Some(&manifest));
                    let output_bindings = compute_output_bindings_for_step(step, Some(&manifest));
                    resolved.push(ResolvedStep {
                        original: step.clone(),
                        capability_id: cap_id.clone(),
                        resolution_strategy: ResolutionStrategy::Synthesized,
                        input_bindings,
                        output_bindings,
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

                    let input_bindings = compute_input_bindings_for_step(step, Some(&manifest));
                    let output_bindings = compute_output_bindings_for_step(step, Some(&manifest));
                    resolved.push(ResolvedStep {
                        original: step.clone(),
                        capability_id: cap_id,
                        resolution_strategy: ResolutionStrategy::Synthesized, // Treat as synthesized for now
                        input_bindings,
                        output_bindings,
                    });
                    continue;
                }
                Ok(DiscoveryResult::NotFound) => {
                    return Err(Box::<dyn std::error::Error>::from(
                        RuntimeError::Generic(format!(
                            "❌ Capability '{}' not found and synthesis failed.",
                            step.capability_class
                        )),
                    ));
                }
                Err(e) => {
                    return Err(Box::<dyn std::error::Error>::from(
                        RuntimeError::Generic(format!(
                            "❌ Failed to synthesize capability '{}':\n\n{}",
                            step.capability_class, e
                        )),
                    ));
                }
            }
        } else {
            return Err(Box::<dyn std::error::Error>::from(RuntimeError::Generic(
                format!(
                    "❌ No delegating arbiter available for synthesis. Cannot synthesize capability '{}'.",
                    step.capability_class
                ),
            )));
        }
    }

    println!(
        "  [resolver] Completed resolution for {} step(s)",
        resolved.len()
    );

    Ok(resolved)
}

async fn fetch_manifest_for_step(
    marketplace: &Arc<ccos::capability_marketplace::CapabilityMarketplace>,
    capability_id: &str,
    step: &ProposedStep,
) -> Option<CapabilityManifest> {
    if let Some(manifest) = marketplace.get_capability(capability_id).await {
        return Some(manifest);
    }

    for candidate in &step.candidate_capabilities {
        if let Some(manifest) = marketplace.get_capability(candidate).await {
            return Some(manifest);
        }
    }

    let manifests = marketplace.list_capabilities().await;
    let tokens: Vec<&str> = step
        .capability_class
        .split(|c: char| c == '.' || c == ':' || c == '/' || c == '-')
        .filter(|part| !part.is_empty())
        .collect();

    manifests
        .iter()
        .find(|manifest| {
            tokens.iter().all(|token| {
                manifest
                    .id
                    .to_ascii_lowercase()
                    .contains(&token.to_ascii_lowercase())
            })
        })
        .cloned()
}

fn compute_input_bindings_for_step(
    step: &ProposedStep,
    manifest: Option<&CapabilityManifest>,
) -> HashMap<String, String> {
    let mut bindings = HashMap::new();
    let input_remap: HashMap<String, String> = manifest
        .and_then(|m| m.metadata.get("mcp_input_remap"))
        .and_then(|raw| serde_json::from_str(raw).ok())
        .unwrap_or_default();

    if let Some(manifest) = manifest {
        if let Some(TypeExpr::Map { entries, .. }) = &manifest.input_schema {
            let mut candidate_keys: Vec<String> = entries
                .iter()
                .filter(|entry| !entry.optional)
                .map(|entry| entry.key.0.clone())
                .collect();
            candidate_keys.extend(
                entries
                    .iter()
                    .filter(|entry| entry.optional)
                    .map(|entry| entry.key.0.clone()),
            );

            for input in &step.required_inputs {
                let (base_input, _) = parse_input_assignment(input);
                let selected = input_remap
                    .get(&base_input)
                    .or_else(|| input_remap.get(input))
                    .cloned()
                    .unwrap_or_else(|| {
                        find_best_input_key(&base_input, &candidate_keys)
                            .unwrap_or_else(|| base_input.clone())
                    });
                bindings.insert(input.clone(), selected.clone());
                if base_input != *input {
                    bindings.entry(base_input.clone()).or_insert(selected);
                }
            }

            // Ensure every required input has a binding even if manifest did not specify it
            for input in &step.required_inputs {
                let (base_input, _) = parse_input_assignment(input);
                bindings
                    .entry(input.clone())
                    .or_insert_with(|| base_input.clone());
                bindings
                    .entry(base_input.clone())
                    .or_insert_with(|| base_input.clone());
            }

            return bindings;
        }
    }

    for input in &step.required_inputs {
        let (base_input, _) = parse_input_assignment(input);
        bindings.insert(input.clone(), base_input.clone());
        if base_input != *input {
            bindings.entry(base_input.clone()).or_insert(base_input);
        }
    }

    bindings
}
fn compute_output_bindings_for_step(
    step: &ProposedStep,
    manifest: Option<&CapabilityManifest>,
) -> HashMap<String, OutputBinding> {
    let mut bindings = HashMap::new();
    let manifest_keys = manifest
        .and_then(|m| m.output_schema.as_ref())
        .map(collect_output_keys_from_schema)
        .unwrap_or_default();

    for output in &step.expected_outputs {
        if let Some(actual_key) = find_best_input_key(output, &manifest_keys) {
            bindings.insert(output.clone(), OutputBinding::MapKey(actual_key));
        } else if manifest_keys.len() == 1 {
            bindings.insert(
                output.clone(),
                OutputBinding::MapKey(manifest_keys[0].clone()),
            );
        } else {
            bindings.insert(output.clone(), OutputBinding::MapKey(output.clone()));
        }
    }

    bindings
}

fn collect_output_keys_from_schema(schema: &TypeExpr) -> Vec<String> {
    match schema {
        TypeExpr::Map { entries, .. } => entries
            .iter()
            .map(|entry| entry.key.0.trim_start_matches(':').to_string())
            .collect(),
        TypeExpr::Vector(inner) | TypeExpr::Optional(inner) => {
            collect_output_keys_from_schema(inner)
        }
        TypeExpr::Union(options) => options
            .iter()
            .flat_map(collect_output_keys_from_schema)
            .collect(),
        _ => Vec::new(),
    }
}

const STOPWORDS: &[&str] = &[
    "a", "an", "and", "for", "from", "in", "of", "on", "the", "to", "with",
];

fn manifest_is_incomplete(manifest: &CapabilityManifest) -> bool {
    if manifest
        .metadata
        .get("status")
        .map(|status| {
            status.eq_ignore_ascii_case("incomplete") || status.eq_ignore_ascii_case("stub")
        })
        .unwrap_or(false)
    {
        return true;
    }

    let local_rtfs_synth = manifest
        .metadata
        .get("synthesis_method")
        .or_else(|| manifest.metadata.get("synthesis-method"))
        .map(|method| method.eq_ignore_ascii_case("local_rtfs"))
        .unwrap_or(false);
    // LocalSynthesizer was removed - local_rtfs synthesis no longer exists
    // Treat all local_rtfs synthesized capabilities as incomplete
    if local_rtfs_synth {
        return true;
    }

    manifest.version.to_ascii_lowercase().contains("incomplete")
        || manifest
            .name
            .to_ascii_lowercase()
            .starts_with("[incomplete]")
}

fn find_manifest_for_step(
    step: &ProposedStep,
    manifests: &[CapabilityManifest],
) -> Option<CapabilityManifest> {
    if let Some(manifest) = step
        .candidate_capabilities
        .iter()
        .filter_map(|candidate| {
            manifests
                .iter()
                .find(|m| m.id == *candidate && !manifest_is_incomplete(m))
        })
        .next()
    {
        return Some(manifest.clone());
    }

    let tokens = collect_step_tokens(step);
    if tokens.is_empty() {
        return None;
    }

    manifests
        .iter()
        .filter(|manifest| !manifest_is_incomplete(manifest))
        .filter_map(|manifest| {
            let score = score_manifest_against_tokens(manifest, &tokens);
            if score == 0 {
                return None;
            }
            let matches = count_token_matches(manifest, &tokens);
            if matches < minimum_token_matches(tokens.len()) {
                return None;
            }
            Some((score, matches, manifest))
        })
        .max_by(|a, b| {
            a.0.cmp(&b.0)
                .then(a.1.cmp(&b.1))
                .then(b.2.id.len().cmp(&a.2.id.len()))
        })
        .map(|(_, _, manifest)| manifest.clone())
}

fn collect_step_tokens(step: &ProposedStep) -> Vec<String> {
    let mut set = HashSet::new();
    for text in [
        step.capability_class.as_str(),
        step.id.as_str(),
        step.name.as_str(),
    ] {
        set.extend(tokenize_identifier(text));
    }
    for candidate in &step.candidate_capabilities {
        set.extend(tokenize_identifier(candidate));
    }
    set.into_iter().filter(|token| token.len() > 1).collect()
}

fn derive_capability_class_hint(step: &ProposedStep) -> String {
    let base = normalize_identifier_for_class(&step.capability_class);
    let mut ordered_tokens: Vec<String> = Vec::new();
    ordered_tokens.extend(tokens_from_str(&step.name));
    if let Some(desc) = &step.description {
        ordered_tokens.extend(tokens_from_str(desc));
    }
    ordered_tokens.extend(
        step.required_inputs
            .iter()
            .map(|input| normalize_identifier_for_class(input)),
    );
    ordered_tokens.extend(
        step.expected_outputs
            .iter()
            .map(|output| normalize_identifier_for_class(output)),
    );

    let mut selected = Vec::new();
    let mut seen = HashSet::new();

    for token in ordered_tokens {
        if token.is_empty()
            || token == base
            || STOPWORDS.contains(&token.as_str())
            || !seen.insert(token.clone())
        {
            continue;
        }
        selected.push(token);
        if selected.len() >= 3 {
            break;
        }
    }

    if selected.is_empty() {
        base
    } else {
        format!("{}.{}", base, selected.join("."))
    }
}

fn tokens_from_str(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_ascii_alphanumeric())
        .filter_map(|segment| {
            let normalized = normalize_identifier_for_class(segment);
            if normalized.is_empty() {
                None
            } else {
                Some(normalized)
            }
        })
        .collect()
}

fn normalize_identifier_for_class(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '.')
        .map(|ch| ch.to_ascii_lowercase())
        .collect()
}

fn find_best_input_key(input: &str, candidates: &[String]) -> Option<String> {
    if candidates.is_empty() {
        return None;
    }

    // Exact case-sensitive match
    if let Some(candidate) = candidates.iter().find(|c| c.as_str() == input) {
        return Some(candidate.clone());
    }

    let input_lower = input.to_ascii_lowercase();

    // Case-insensitive match
    if let Some(candidate) = candidates
        .iter()
        .find(|c| c.to_ascii_lowercase() == input_lower)
    {
        return Some(candidate.clone());
    }

    let normalized_input = normalize_identifier_for_match(input);

    // Normalized equality match
    if let Some(candidate) = candidates
        .iter()
        .find(|c| normalize_identifier_for_match(c) == normalized_input)
    {
        return Some(candidate.clone());
    }

    // Singularization equality match
    let singular_input = singularize_identifier(&normalized_input);
    if let Some(candidate) = candidates
        .iter()
        .find(|c| singularize_identifier(&normalize_identifier_for_match(c)) == singular_input)
    {
        return Some(candidate.clone());
    }

    // Prefix/contains heuristics
    if let Some(candidate) = candidates.iter().find(|c| {
        let normalized_candidate = normalize_identifier_for_match(c);
        normalized_candidate.starts_with(&normalized_input)
            || normalized_input.starts_with(&normalized_candidate)
            || normalized_candidate.contains(&normalized_input)
            || normalized_input.contains(&normalized_candidate)
    }) {
        return Some(candidate.clone());
    }

    None
}

fn normalize_identifier_for_match(value: &str) -> String {
    let mut normalized = String::with_capacity(value.len());
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            normalized.push(ch.to_ascii_lowercase());
        }
    }
    normalized
}

fn singularize_identifier(value: &str) -> String {
    if value.ends_with("ies") && value.len() > 3 {
        let stem = &value[..value.len() - 3];
        format!("{}y", stem)
    } else if value.ends_with('s') && value.len() > 1 {
        value[..value.len() - 1].to_string()
    } else {
        value.to_string()
    }
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
                Value::String(format!(
                    "{{pending: stub for {}}}",
                    step_copy.capability_class
                )),
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
    println!(
        "{} {}",
        "🎯 ROOT:".bold().cyan(),
        intent.goal.as_str().bold()
    );

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
                println!(
                    "{} {} {}",
                    connector,
                    icon,
                    step.capability_id.as_str().green()
                );
            }
            ResolutionStrategy::Synthesized => {
                println!(
                    "{} {} {}",
                    connector,
                    icon,
                    step.capability_id.as_str().cyan()
                );
            }
            ResolutionStrategy::Stubbed => {
                println!(
                    "{} {} {}",
                    connector,
                    icon,
                    step.capability_id.as_str().yellow()
                );
            }
        }

        // Print step details
        if !is_last {
            println!(
                "{}{}   {} {}",
                indent,
                "│".dim(),
                "Name:".dim(),
                step.original.name.as_str()
            );
        } else {
            println!(
                "{}{}   {} {}",
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
            println!(
                "{}",
                format!("{}{}   {}", indent, indent_char, io_text).dim()
            );
        }
    }

    println!("{}", "─".repeat(80).dim());

    // Add legend
    println!("\n{}", "Legend:".dim());
    println!(
        "   ✅ {}  {}",
        "Found".green(),
        "- Capability exists in marketplace".dim()
    );
    println!(
        "   🔄 {}  {}",
        "Synthesized".cyan(),
        "- Capability generated recursively".dim()
    );
    println!(
        "   ⚠️  {}  {}",
        "Stubbed".yellow(),
        "- Placeholder for future implementation".dim()
    );
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

    if marketplace.get_capability(capability_id).await.is_some() {
        println!(
            "  {} Updating existing orchestrator capability: {}",
            "ℹ️".blue(),
            capability_id.cyan()
        );
        if let Err(e) = marketplace.remove_capability(capability_id).await {
            eprintln!(
                "  {} Failed to remove prior orchestrator {}: {}",
                "⚠️".yellow(),
                capability_id.cyan(),
                e
            );
        }
    }

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
            "Auto-generated capability that orchestrates multiple steps into a coordinated plan"
                .to_string(),
            handler,
        )
        .await;

    // Persist the orchestrator RTFS code to disk so it can be executed later by id
    {
        let dir = Path::new("capabilities/generated");
        let persist_result: DemoResult<()> = (|| {
            fs::create_dir_all(dir)?;
            let file_path = dir.join(format!("{}.rtfs", capability_id));
            if !file_path.exists() {
                fs::write(&file_path, orchestrator_rtfs.as_bytes())?;
            }
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
        let persist_cap_result: DemoResult<()> = (|| {
            let capability_rtfs =
                convert_plan_to_capability_rtfs(capability_id, orchestrator_rtfs)?;
            let cap_dir = Path::new("capabilities/generated").join(capability_id);
            fs::create_dir_all(&cap_dir)?;
            let cap_file = cap_dir.join("capability.rtfs");
            if !cap_file.exists() {
                fs::write(&cap_file, capability_rtfs.as_bytes())?;
            }
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

    println!("  📦 Registered as capability: {}", capability_id.cyan());

    Ok(())
}

/// Convert a consolidated RTFS (plan ...) into a Capability RTFS with :implementation holding the plan :body
fn convert_plan_to_capability_rtfs(capability_id: &str, plan_rtfs: &str) -> DemoResult<String> {
    use chrono::Utc;
    let created_at = Utc::now().to_rfc3339();

    // Extract fields from plan
    let body_do = extract_s_expr_after_key(plan_rtfs, ":body")
        .or_else(|| extract_do_block(plan_rtfs))
        .ok_or_else(|| {
            Box::<dyn std::error::Error>::from(RuntimeError::Generic(
                "Could not extract :body from plan".to_string(),
            ))
        })?;
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
    out.push_str(
        "  :description \"Auto-generated orchestrator capability from smart_assistant plan\"\n",
    );
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
fn extract_do_block(text: &str) -> Option<String> {
    extract_block_with_head(text, "do")
}

/// Extract plan properties from a (plan ...) form
struct ExtractedPlanProperties {
    body: String,
    input_schema: Option<rtfs::runtime::values::Value>,
    output_schema: Option<rtfs::runtime::values::Value>,
    capabilities_required: Vec<String>,
    annotations: HashMap<String, rtfs::runtime::values::Value>,
}
async fn build_register_and_execute_plan(
    ccos: &Arc<CCOS>,
    missing_capability_resolver: Option<Arc<MissingCapabilityResolver>>,
    agent_config: &AgentConfig,
    args: &Args,
    goal: &str,
    intent: &Intent,
    answers: &[AnswerRecord],
    plan_steps: &[ProposedStep],
    matches: &[CapabilityMatch],
) -> DemoResult<()> {
    let needs_value = build_needs_capabilities(plan_steps);

    // Resolve missing capabilities and build orchestrating agent
    let mut resolved_steps = resolve_and_stub_capabilities(
        ccos,
        missing_capability_resolver.clone(),
        plan_steps,
        matches,
        args.interactive,
    )
    .await?;
    println!(
        "[trace] resolve_and_stub_capabilities returned {} step(s)",
        resolved_steps.len()
    );
    enrich_resolved_steps_with_sampling(ccos, &mut resolved_steps, intent, answers).await;
    let planner_capability_id = derive_orchestrator_capability_id(goal, &resolved_steps);
    println!(
        "[trace] derived orchestrator capability id: {}",
        planner_capability_id
    );
    let generated =
        generate_orchestrator_capability(goal, &resolved_steps, &planner_capability_id)?;
    let orchestrator_rtfs = generated.plan_rtfs.clone();
    println!(
        "[trace] generated orchestrator RTFS ({} bytes)",
        orchestrator_rtfs.len()
    );

    // Register the orchestrator as a reusable capability in the marketplace
    register_orchestrator_in_marketplace(ccos, &planner_capability_id, &orchestrator_rtfs).await?;
    println!("[trace] registered orchestrator in marketplace");

    // Extract all properties from (plan ...) form before creating the plan
    let plan_props = ExtractedPlanProperties {
        body: generated.body.clone(),
        input_schema: generated.input_schema.clone(),
        output_schema: generated.output_schema.clone(),
        capabilities_required: generated.capabilities_required.clone(),
        annotations: generated.annotations.clone(),
    };
    println!("[trace] extracted plan properties without parser");
    let mut plan = Plan::new_with_schemas(
        None,
        vec![intent.intent_id.clone()],
        PlanBody::Rtfs(plan_props.body),
        plan_props.input_schema,
        plan_props.output_schema,
        HashMap::new(), // policies
        plan_props.capabilities_required,
        plan_props.annotations,
    );
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
        Value::String(planner_capability_id.clone()),
    );
    plan.metadata
        .insert("goal".to_string(), Value::String(goal.to_string()));

    if let Some(fixture) = args.inject_plan_error {
        if let PlanBody::Rtfs(ref mut body) = plan.body {
            let original = body.clone();
            let mutated = inject_plan_error_source(&original, fixture);
            if mutated != original {
                println!(
                    "\n{} Injecting {:?} plan error to exercise auto-repair",
                    "⚠️".yellow(),
                    fixture
                );
                plan.metadata.insert(
                    "injected_plan_error".to_string(),
                    Value::String(format!("{:?}", fixture)),
                );
                plan.metadata.insert(
                    "injected_plan_error_original".to_string(),
                    Value::String(original),
                );
                *body = mutated;
            } else {
                println!(
                    "\n{} Unable to inject {:?} plan error (pattern not found); continuing with original plan",
                    "ℹ️".blue(),
                    fixture
                );
            }
        }
    }

    print_plan_draft(plan_steps, matches, &plan);

    // Print resolution summary
    let found_count = resolved_steps
        .iter()
        .filter(|s| s.resolution_strategy == ResolutionStrategy::Found)
        .count();
    let synthesized_count = resolved_steps
        .iter()
        .filter(|s| s.resolution_strategy == ResolutionStrategy::Synthesized)
        .count();
    let stubbed_count = resolved_steps
        .iter()
        .filter(|s| s.resolution_strategy == ResolutionStrategy::Stubbed)
        .count();

    println!("\n{}", "📊 Capability Resolution Summary".bold());
    println!(
        "   • Found: {} capabilities",
        found_count.to_string().green()
    );
    if synthesized_count > 0 {
        println!(
            "   • {}: {} capabilities (with dependencies)",
            "Synthesized".bold(),
            synthesized_count.to_string().cyan().bold()
        );
    }
    if stubbed_count > 0 {
        println!(
            "   • Stubbed: {} capabilities (awaiting implementation)",
            stubbed_count.to_string().yellow()
        );
    }

    // Display execution graph visualization
    print_execution_graph(&resolved_steps, intent);

    println!(
        "\n{}",
        "✅ Orchestrator generated and registered in marketplace"
            .bold()
            .green()
    );

    // Save the plan to the plan archive
    let orchestrator = Arc::clone(&ccos.orchestrator);
    match orchestrator.store_plan(&plan) {
        Ok(hash) => {
            let hash_display = hash.clone();
            println!(
                "  💾 Saved plan to archive with hash: {}",
                hash_display.cyan()
            );
            // If using file storage, show the file path
            let plan_archive_path = std::env::var("CCOS_PLAN_ARCHIVE_PATH")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|_| std::path::PathBuf::from("demo_storage/plans"));
            let file_path = plan_archive_path
                .join(format!("{}/{}", &hash[0..2], &hash[2..4]))
                .join(format!("{}.json", hash));
            println!(
                "     File location: {}",
                file_path.display().to_string().dim()
            );
        }
        Err(e) => {
            eprintln!("  ⚠️  Failed to save plan to archive: {}", e);
        }
    }

    // Load only the required capabilities into the RTFS environment before execution
    // Check both generated (synthesized) and discovered (MCP) directories
    println!("\n{}", "📦 Loading Required Capabilities".bold());
    println!("{}", "=".repeat(80));
    let marketplace = ccos.get_capability_marketplace();
    let generated_dir = std::path::Path::new("capabilities/generated");
    let discovered_dir = std::path::Path::new("capabilities/servers/approved");

    if discovered_dir.exists() {
        match preload_discovered_capabilities(&marketplace, discovered_dir).await {
            Ok(count) => {
                if count > 0 {
                    println!(
                        "  {} Preloaded {} discovered capability manifest(s)",
                        "✓".green(),
                        count
                    );
                }
            }
            Err(e) => eprintln!(
                "  {} Failed to preload discovered capabilities: {}",
                "⚠️".yellow(),
                e
            ),
        }
    }

    if !plan.capabilities_required.is_empty() {
        let mut loaded_count = 0usize;
        let mut missing_caps = Vec::new();

        for cap_id in &plan.capabilities_required {
            println!(
                "  {} Checking required capability: {}",
                "ℹ️".blue(),
                cap_id.as_str()
            );
            // Check if capability is already registered in marketplace
            if marketplace.has_capability(cap_id).await {
                println!(
                    "  {} Capability already available: {}",
                    "✓".green(),
                    cap_id.as_str().green()
                );
                continue;
            }

            let mut found = false;

            // Try to load from generated directory (synthesized capabilities)
            let cap_dir = generated_dir.join(cap_id);
            let cap_file = cap_dir.join("capability.rtfs");

            if cap_file.exists() {
                match marketplace
                    .import_capabilities_from_rtfs_dir(&cap_dir)
                    .await
                {
                    Ok(count) => {
                        if count > 0 {
                            loaded_count += count;
                            println!(
                                "  {} Loaded from generated: {}",
                                "✓".green(),
                                cap_id.as_str().green()
                            );
                            found = true;
                        }
                    }
                    Err(e) => {
                        eprintln!(
                            "  {} Failed to load {} from generated: {}",
                            "⚠️".yellow(),
                            cap_id.as_str().yellow(),
                            e
                        );
                    }
                }
            }

            // If not found in generated, try discovered directory (MCP capabilities)
            if !found {
                if discovered_dir.exists() {
                    if let Ok(entries) = std::fs::read_dir(discovered_dir) {
                        for entry in entries.flatten() {
                            let path = entry.path();
                            if path.is_dir() {
                                match marketplace.import_capabilities_from_rtfs_dir(&path).await {
                                    Ok(count) => {
                                        if count > 0 {
                                            let all_caps = marketplace.list_capabilities().await;
                                            if all_caps.iter().any(|cap| cap.id == cap_id.as_str())
                                            {
                                                loaded_count += count;
                                                println!(
                                                    "  {} Loaded from discovered: {}",
                                                    "✓".green(),
                                                    cap_id.as_str().green()
                                                );
                                                found = true;
                                                break;
                                            }
                                        }
                                    }
                                    Err(_) => {
                                        // Continue searching
                                    }
                                }
                            }
                        }
                    }
                }
            }

            if !found {
                let all_caps = marketplace.list_capabilities().await;
                let keywords: Vec<&str> = cap_id.split('.').collect();
                let matching_cap = all_caps.iter().find(|cap| {
                    keywords.iter().all(|kw| {
                        cap.id
                            .to_ascii_lowercase()
                            .contains(&kw.to_ascii_lowercase())
                    })
                });

                if let Some(matching) = matching_cap {
                    println!(
                        "  {} Found matching capability: {} (registered as {})",
                        "✓".green(),
                        cap_id.as_str().green(),
                        matching.id.as_str().cyan()
                    );

                    let wrapper_id = cap_id.clone();
                    let actual_id = matching.id.clone();

                    if wrapper_id.as_str() == actual_id.as_str() {
                        println!(
                            "  {} Capability already available under required id: {}",
                            "✓".green(),
                            wrapper_id.as_str().green()
                        );
                        found = true;
                    } else {
                        let mut alias_manifest = matching.clone();
                        alias_manifest.id = wrapper_id.as_str().to_string();
                        alias_manifest
                            .metadata
                            .insert("alias_of".to_string(), actual_id.as_str().to_string());
                        alias_manifest.metadata.insert(
                            "alias_created_by".to_string(),
                            "smart_assistant_demo".to_string(),
                        );
                        alias_manifest.name = format!("{} (alias)", alias_manifest.name)
                            .chars()
                            .take(120)
                            .collect();

                        match marketplace
                            .register_capability_manifest(alias_manifest)
                            .await
                        {
                            Ok(_) => {
                                println!(
                                    "  {} Registered alias capability: {} → {}",
                                    "✓".green(),
                                    wrapper_id.as_str().green(),
                                    actual_id.as_str().cyan()
                                );
                                loaded_count += 1;
                                found = true;
                            }
                            Err(e) => {
                                eprintln!(
                                    "  {} Failed to register alias {} → {}: {}",
                                    "⚠️".yellow(),
                                    wrapper_id.as_str().yellow(),
                                    actual_id.as_str().cyan(),
                                    e
                                );
                            }
                        }
                    }
                }
            }

            if !found {
                missing_caps.push(cap_id.clone());
                println!(
                    "  {} Not found: {} (checked generated and discovered directories)",
                    "⚠️".yellow(),
                    cap_id.as_str().yellow()
                );
            }
        }

        if loaded_count > 0 {
            println!(
                "  {} Loaded {} required capability/capabilities",
                "✓".green(),
                loaded_count.to_string().green()
            );
        }
        if !missing_caps.is_empty() {
            println!(
                "  {} {} capability/capabilities not found or failed to load",
                "⚠️".yellow(),
                missing_caps.len().to_string().yellow()
            );
            println!(
                "  {} Tip: These capabilities may be registered with different IDs (e.g., MCP IDs)",
                "ℹ️".dim()
            );
        }
    } else {
        println!("  {} No capabilities required by this plan", "ℹ️".dim());
    }

    // Execute the generated plan
    println!("\n{}", "🚀 Executing Plan".bold());
    println!("{}", "=".repeat(80));

    let mut context = rtfs::runtime::security::RuntimeContext::full();
    extract_and_bind_plan_inputs(&mut context, intent, answers);

    let mut repair_options = PlanAutoRepairOptions::default();
    let mut context_lines = vec![format!("Goal: {}", goal)];
    if let Some(fixture) = args.inject_plan_error {
        context_lines.push(format!("Injected plan error fixture: {:?}", fixture));
    }
    repair_options.additional_context = Some(context_lines.join("\n"));
    repair_options.debug_responses = args.debug_prompts;

    let _repair_options = repair_options;

    match ccos.validate_and_execute_plan(plan, &context).await {
        Ok(exec_result) => {
            if exec_result.success {
                println!(
                    "\n{}",
                    "✅ Plan execution completed successfully!".bold().green()
                );
                println!("{}", "Result:".bold());
                println!("{:?}", exec_result.value);
            } else {
                println!(
                    "\n{}",
                    "⚠️  Plan execution completed with warnings".bold().yellow()
                );
                if let Some(error) = exec_result.metadata.get("error") {
                    println!("Error: {:?}", error);
                }
                println!("Result: {:?}", exec_result.value);
            }
        }
        Err(e) => {
            println!("\n{}", "❌ Plan execution failed".bold().red());
            println!("Error: {}", e);
        }
    }

    // LEARNING IMPACT ANALYSIS (as promised in documentation)
    println!("\n{}", "═══════════════════════════════════════════════════════════════".dim());
    println!("{}", "                    LEARNING IMPACT ANALYSIS".bold().cyan());
    println!("{}", "═══════════════════════════════════════════════════════════════".dim());

    let total_turns = 1 + answers.len();
    let questions_asked = answers.len();
    
    // Gain analysis: if we run this again with the same intent, turns will be 1 and questions 0.
    println!("┌─────────────────────┬───────────────┬───────────────┬──────────┐");
    println!("│ Metric              │ Before Learn  │ After Learn   │ Gain     │");
    println!("├─────────────────────┼───────────────┼───────────────┼──────────┤");
    println!("│ Interaction Turns   │ {:>13} │ {:>13} │ {:>7}  │", total_turns, 1, format!("{}x", total_turns));
    println!("│ Questions Asked     │ {:>13} │ {:>13} │ {:>7}  │", questions_asked, 0, format!("-{}", questions_asked));
    println!("└─────────────────────┴───────────────┴───────────────┴──────────┘");
    
    if synthesized_count > 0 {
         println!(" {} New knowledge synthesized: {} new capabilities learned and persisted.", "🎓".cyan(), synthesized_count);
    }
    println!(" {} Location: capabilities/generated/", "📁".dim());

    println!("\n{}", "🔁 Architecture snapshot after execution".bold());
    print_architecture_summary(agent_config, args.profile.as_deref());

    Ok(())
}

/// Extract all plan properties from a (plan ...) form
fn extract_plan_properties(plan_rtfs: &str) -> DemoResult<ExtractedPlanProperties> {
    // Try to parse as top-level construct to extract properties from (plan ...) form
    match rtfs::parser::parse(plan_rtfs) {
        Ok(top_levels) => {
            // Look for a Plan top-level construct
            if let Some(rtfs::ast::TopLevel::Plan(plan_def)) = top_levels.first() {
                let mut body = None;
                let mut input_schema = None;
                let mut output_schema = None;
                let mut capabilities_required = Vec::new();
                let mut annotations = HashMap::new();

                // Extract all properties
                for prop in &plan_def.properties {
                    match prop.key.0.as_str() {
                        "body" => {
                            body = Some(ccos::rtfs_bridge::expression_to_rtfs_string(&prop.value));
                        }
                        "input-schema" | "input_schema" => {
                            // Convert expression to Value using normalizer
                            input_schema =
                                ccos::rtfs_bridge::normalizer::expression_to_value_simple(
                                    &prop.value,
                                );
                        }
                        "output-schema" | "output_schema" => {
                            // Convert expression to Value using normalizer
                            output_schema =
                                ccos::rtfs_bridge::normalizer::expression_to_value_simple(
                                    &prop.value,
                                );
                        }
                        "capabilities-required" | "capabilities_required" => {
                            // Extract vector of strings
                            if let rtfs::ast::Expression::Vector(vec) = &prop.value {
                                for expr in vec {
                                    if let rtfs::ast::Expression::Literal(
                                        rtfs::ast::Literal::String(s),
                                    ) = expr
                                    {
                                        capabilities_required.push(s.clone());
                                    }
                                }
                            }
                        }
                        "annotations" => {
                            // Extract map of annotations
                            if let rtfs::ast::Expression::Map(map) = &prop.value {
                                for (key, expr) in map {
                                    let key_str = match key {
                                        rtfs::ast::MapKey::String(s) => s.clone(),
                                        rtfs::ast::MapKey::Keyword(k) => k.0.clone(),
                                        rtfs::ast::MapKey::Integer(i) => i.to_string(),
                                    };
                                    if let Some(value) =
                                        ccos::rtfs_bridge::normalizer::expression_to_value_simple(
                                            expr,
                                        )
                                    {
                                        annotations.insert(key_str, value);
                                    }
                                }
                            }
                        }
                        _ => {
                            // Ignore other properties
                        }
                    }
                }

                Ok(ExtractedPlanProperties {
                    body: body.ok_or_else(|| {
                        runtime_error(RuntimeError::Generic(
                            "Plan has (plan ...) form but no :body property found".to_string(),
                        ))
                    })?,
                    input_schema,
                    output_schema,
                    capabilities_required,
                    annotations,
                })
            } else {
                Err(runtime_error(RuntimeError::Generic(format!(
                    "Expected Plan top-level, got: {:?}",
                    top_levels.first()
                ))))
            }
        }
        Err(e) => Err(runtime_error(RuntimeError::Generic(format!(
            "Failed to parse (plan ...) form: {:?}",
            e
        )))),
    }
}

/// Extract the :body from a (plan ...) form, returning just the executable RTFS code
fn extract_plan_body(plan_rtfs: &str) -> DemoResult<String> {
    extract_plan_properties(plan_rtfs).map(|props| props.body)
}

/// Extract input values from intent/answers and bind them to the runtime context
/// This ensures plan inputs are available during execution
fn extract_and_bind_plan_inputs(
    context: &mut rtfs::runtime::security::RuntimeContext,
    intent: &Intent,
    answers: &[AnswerRecord],
) {
    // Extract values from intent constraints
    for (key, value) in &intent.constraints {
        if let Ok(rtfs_value) = value_to_rtfs_value(value) {
            context.add_cross_plan_param(key.clone(), rtfs_value);
        }
    }

    // Extract values from answers
    for answer in answers {
        if let Ok(rtfs_value) = value_to_rtfs_value(&answer.value) {
            context.add_cross_plan_param(answer.key.clone(), rtfs_value);
        }
    }

    // Also extract common parameters from intent constraints if available
    // This handles cases where parameters are stored in constraints
    if let Some(owner) = intent.constraints.get("owner") {
        if let Ok(rtfs_value) = value_to_rtfs_value(owner) {
            context.add_cross_plan_param("owner".to_string(), rtfs_value);
        }
    }
    if let Some(repository) = intent.constraints.get("repository") {
        if let Ok(rtfs_value) = value_to_rtfs_value(repository) {
            context.add_cross_plan_param("repository".to_string(), rtfs_value);
        }
    }
    if let Some(language) = intent.constraints.get("language") {
        if let Ok(rtfs_value) = value_to_rtfs_value(language) {
            context.add_cross_plan_param("language".to_string(), rtfs_value);
        }
    }
    if let Some(filter_criteria) = intent.constraints.get("filter_criteria") {
        if let Ok(rtfs_value) = value_to_rtfs_value(filter_criteria) {
            context.add_cross_plan_param("language".to_string(), rtfs_value);
        }
    }

    ensure_owner_repo_aliases(context);
}

/// Convert a Value to RTFS runtime Value
fn value_to_rtfs_value(value: &Value) -> DemoResult<rtfs::runtime::values::Value> {
    match value {
        Value::String(s) => Ok(rtfs::runtime::values::Value::String(s.clone())),
        Value::Integer(i) => Ok(rtfs::runtime::values::Value::Integer(*i)),
        Value::Float(f) => Ok(rtfs::runtime::values::Value::Float(*f)),
        Value::Boolean(b) => Ok(rtfs::runtime::values::Value::Boolean(*b)),
        Value::Nil => Ok(rtfs::runtime::values::Value::Nil),
        Value::Keyword(k) => Ok(rtfs::runtime::values::Value::Keyword(k.clone())),
        Value::Symbol(s) => Ok(rtfs::runtime::values::Value::Symbol(s.clone())),
        Value::Vector(v) => {
            let rtfs_vec: Result<Vec<_>, _> = v.iter().map(value_to_rtfs_value).collect();
            Ok(rtfs::runtime::values::Value::Vector(rtfs_vec?))
        }
        Value::Map(m) => {
            let mut rtfs_map = std::collections::HashMap::new();
            for (k, v) in m {
                let rtfs_key = match k {
                    rtfs::ast::MapKey::String(s) => rtfs::ast::MapKey::String(s.clone()),
                    rtfs::ast::MapKey::Keyword(kw) => rtfs::ast::MapKey::Keyword(kw.clone()),
                    rtfs::ast::MapKey::Integer(i) => rtfs::ast::MapKey::Integer(*i),
                };
                rtfs_map.insert(rtfs_key, value_to_rtfs_value(v)?);
            }
            Ok(rtfs::runtime::values::Value::Map(rtfs_map))
        }
        _ => Err(runtime_error(RuntimeError::Generic(format!(
            "Unsupported value type for plan input: {:?}",
            value
        )))),
    }
}

fn ensure_owner_repo_aliases(context: &mut rtfs::runtime::security::RuntimeContext) {
    let repository_value = context.cross_plan_params.get("repository").cloned();

    if let Some(Value::String(repo_str)) = repository_value {
        if let Some((owner, repo)) = repo_str.split_once('/') {
            if !context.cross_plan_params.contains_key("owner") {
                context
                    .cross_plan_params
                    .insert("owner".to_string(), Value::String(owner.to_string()));
            }
            context
                .cross_plan_params
                .insert("repo".to_string(), Value::String(repo.to_string()));
        } else {
            context
                .cross_plan_params
                .entry("repo".to_string())
                .or_insert(Value::String(repo_str.clone()));
        }
    } else if let Some(Value::String(repo_only)) = context.cross_plan_params.get("repo").cloned() {
        context
            .cross_plan_params
            .entry("repository".to_string())
            .or_insert(Value::String(repo_only));
    }
}

async fn sample_mcp_outputs(
    ccos: &Arc<CCOS>,
    resolved_steps: &[ResolvedStep],
    context: &rtfs::runtime::security::RuntimeContext,
) -> DemoResult<()> {
    let marketplace = ccos.get_capability_marketplace();

    for step in resolved_steps {
        let Some(manifest) = marketplace.get_capability(&step.capability_id).await else {
            continue;
        };

        if !manifest.metadata.contains_key("mcp_server_url") {
            continue;
        }

        if manifest
            .metadata
            .get("ccos_sampled_output_schema")
            .is_some()
        {
            continue;
        }

        let required_inputs = &step.original.required_inputs;
        let Some(sample_input) =
            build_sample_input_for_manifest(&manifest, context, required_inputs)
        else {
            eprintln!(
                "  {} Skipping schema sampling for {} (insufficient input data)",
                "ℹ️".dim(),
                manifest.id
            );
            continue;
        };

        match marketplace
            .execute_capability(&manifest.id, &sample_input)
            .await
        {
            Ok(output_value) => {
                let inferred_schema = infer_type_expr_from_value(&output_value);
                marketplace
                    .update_capability_output_schema(&manifest.id, inferred_schema.clone())
                    .await
                    .map_err(runtime_error)?;

                if let Some(alias_of) = manifest.metadata.get("alias_of") {
                    let _ = marketplace
                        .update_capability_output_schema(alias_of, inferred_schema.clone())
                        .await;
                }

                if let Err(e) = persist_mcp_output_schema(&manifest, &inferred_schema) {
                    eprintln!(
                        "  {} Failed to persist output schema for {}: {}",
                        "⚠️".yellow(),
                        manifest.id,
                        e
                    );
                } else {
                    println!(
                        "  {} Sampled MCP output schema for {}",
                        "✓".green(),
                        manifest.id
                    );
                }
            }
            Err(e) => {
                eprintln!(
                    "  {} Sampling call failed for {}: {}",
                    "⚠️".yellow(),
                    manifest.id,
                    e
                );
            }
        }
    }

    Ok(())
}

async fn enrich_resolved_steps_with_sampling(
    ccos: &Arc<CCOS>,
    resolved_steps: &mut [ResolvedStep],
    intent: &Intent,
    answers: &[AnswerRecord],
) {
    let mut sampling_context = rtfs::runtime::security::RuntimeContext::full();
    extract_and_bind_plan_inputs(&mut sampling_context, intent, answers);

    if let Err(e) = sample_mcp_outputs(ccos, resolved_steps, &sampling_context).await {
        eprintln!(
            "  {} Failed to sample MCP capability outputs: {}",
            "⚠️".yellow(),
            e
        );
    }

    let manifests = ccos.get_capability_marketplace().list_capabilities().await;

    for step in resolved_steps.iter_mut() {
        if let Some(manifest) = manifests.iter().find(|m| m.id == step.capability_id) {
            step.input_bindings = compute_input_bindings_for_step(&step.original, Some(manifest));
            step.output_bindings = compute_output_bindings_for_step(&step.original, Some(manifest));
        }
    }
}
fn build_sample_input_for_manifest(
    manifest: &CapabilityManifest,
    context: &rtfs::runtime::security::RuntimeContext,
    fallback_inputs: &[String],
) -> Option<Value> {
    let schema_json = manifest
        .metadata
        .get("mcp_input_schema_json")
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok());

    let required_fields: Vec<String> = if let Some(schema) = &schema_json {
        schema
            .get("required")
            .and_then(|r| r.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .map(|s| s.to_string())
                    .collect()
            })
            .unwrap_or_else(|| fallback_inputs.to_vec())
    } else {
        fallback_inputs.to_vec()
    };

    if required_fields.is_empty() {
        return None;
    }

    let mut map_entries: HashMap<MapKey, Value> = HashMap::new();

    let owner_repo = derive_owner_repo_from_context(context);

    for field in required_fields {
        let value = match field.as_str() {
            "repo" => find_context_value(context, "repo")
                .or_else(|| find_context_value(context, "repository"))
                .or_else(|| {
                    owner_repo
                        .clone()
                        .map(|(_, repo)| Value::String(repo.to_string()))
                }),
            "repository" => find_context_value(context, "repository").or_else(|| {
                owner_repo
                    .clone()
                    .map(|(_, repo)| Value::String(repo.to_string()))
            }),
            "owner" => find_context_value(context, "owner").or_else(|| {
                owner_repo
                    .clone()
                    .map(|(owner, _)| Value::String(owner.to_string()))
            }),
            key => find_context_value(context, key),
        };

        let Some(val) = value else {
            return None;
        };

        map_entries.insert(MapKey::Keyword(Keyword(field.clone())), val);
    }

    Some(Value::Map(map_entries))
}

fn find_context_value(
    context: &rtfs::runtime::security::RuntimeContext,
    key: &str,
) -> Option<Value> {
    context.cross_plan_params.get(key).cloned()
}

fn derive_owner_repo_from_context(
    context: &rtfs::runtime::security::RuntimeContext,
) -> Option<(String, String)> {
    context
        .cross_plan_params
        .get("repository")
        .and_then(|value| value_to_string(value))
        .and_then(|repo| {
            repo.split_once('/')
                .map(|(o, r)| (o.to_string(), r.to_string()))
        })
}

fn infer_type_expr_from_value(value: &Value) -> TypeExpr {
    match value {
        Value::String(_) => TypeExpr::Primitive(PrimitiveType::String),
        Value::Integer(_) => TypeExpr::Primitive(PrimitiveType::Int),
        Value::Float(_) => TypeExpr::Primitive(PrimitiveType::Float),
        Value::Boolean(_) => TypeExpr::Primitive(PrimitiveType::Bool),
        Value::Vector(items) => {
            let element_type = items
                .first()
                .map(|v| infer_type_expr_from_value(v))
                .unwrap_or(TypeExpr::Any);
            TypeExpr::Vector(Box::new(element_type))
        }
        Value::Map(map) => {
            let mut entries: Vec<(String, MapTypeEntry)> = map
                .iter()
                .map(|(key, val)| {
                    let key_str = match key {
                        MapKey::Keyword(k) => k.0.clone(),
                        MapKey::String(s) => s.clone(),
                        MapKey::Integer(i) => i.to_string(),
                    };
                    let entry = MapTypeEntry {
                        key: Keyword(key_str.clone()),
                        value_type: Box::new(infer_type_expr_from_value(val)),
                        optional: false,
                    };
                    (key_str, entry)
                })
                .collect();
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            TypeExpr::Map {
                entries: entries.into_iter().map(|(_, entry)| entry).collect(),
                wildcard: None,
            }
        }
        Value::Nil => TypeExpr::Any,
        _ => TypeExpr::Any,
    }
}

fn persist_mcp_output_schema(manifest: &CapabilityManifest, schema: &TypeExpr) -> DemoResult<()> {
    if !manifest.id.starts_with("mcp.") {
        return Ok(());
    }

    let rest = &manifest.id["mcp.".len()..];
    let parts: Vec<&str> = rest.split('.').collect();

    if parts.len() < 3 {
        return Ok(());
    }

    let namespace = parts[0];
    let server = parts[1];
    let tool = parts[2..].join("_");

    let dir = Path::new("capabilities")
        .join("discovered")
        .join("mcp")
        .join(namespace);
    let file_path = dir.join(format!("{}_{}.rtfs", server, tool));

    if !file_path.exists() {
        return Ok(());
    }

    let contents = fs::read_to_string(&file_path)?;
    let schema_rtfs = type_expr_to_rtfs_compact(schema);

    let mut replaced = false;
    let mut new_lines = Vec::new();

    for line in contents.lines() {
        if line.trim_start().starts_with(":output-schema") {
            new_lines.push(format!("  :output-schema {}", schema_rtfs));
            replaced = true;
        } else {
            new_lines.push(line.to_string());
        }
    }

    if !replaced {
        return Ok(());
    }

    let mut updated = new_lines.join("\n");
    updated.push('\n');
    fs::write(&file_path, updated)?;

    Ok(())
}

/// Extracts the first top-level s-expression immediately following a given keyword key.
fn extract_s_expr_after_key(text: &str, key: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut i = 0usize;
    let mut in_string = false;
    while i + key.len() <= bytes.len() {
        let c = bytes[i] as char;
        if c == '"' {
            in_string = !in_string;
            i += 1;
            continue;
        }
        if !in_string && &text[i..i + key.len()] == key {
            // Move to next '('
            let mut j = i + key.len();
            while j < bytes.len() {
                let cj = bytes[j] as char;
                if cj == '"' {
                    in_string = !in_string;
                    j += 1;
                    continue;
                }
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
        if c == '"' {
            in_string = !in_string;
            i += 1;
            continue;
        }
        if !in_string && &text[i..i + key.len()] == key {
            // Move to next opening delimiter
            let mut j = i + key.len();
            while j < bytes.len() {
                let cj = bytes[j] as char;
                if cj == '"' {
                    in_string = !in_string;
                    j += 1;
                    continue;
                }
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
        if c == '"' {
            in_string = !in_string;
            i += 1;
            continue;
        }
        if !in_string && c == '(' {
            // Check head
            let mut j = i + 1;
            while j < bytes.len() && (bytes[j] as char).is_whitespace() {
                j += 1;
            }
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
    if start >= bytes.len() || (bytes[start] as char) != open {
        return None;
    }
    let mut depth: i32 = 0;
    let mut in_string = false;
    let mut i = start;
    while i < bytes.len() {
        let c = bytes[i] as char;
        if c == '"' {
            in_string = !in_string;
            i += 1;
            continue;
        }
        if !in_string {
            if c == open {
                depth += 1;
            } else if c == close {
                depth -= 1;
                if depth == 0 {
                    return Some(text[start..=i].to_string());
                }
            }
        }
        i += 1;
    }
    None
}

/// Generate an RTFS orchestrator capability that chains all resolved steps.
struct GeneratedOrchestrator {
    plan_rtfs: String,
    body: String,
    input_schema: Option<rtfs::runtime::values::Value>,
    output_schema: Option<rtfs::runtime::values::Value>,
    capabilities_required: Vec<String>,
    annotations: HashMap<String, rtfs::runtime::values::Value>,
}

fn generate_orchestrator_capability(
    goal: &str,
    resolved_steps: &[ResolvedStep],
    plan_id: &str,
) -> DemoResult<GeneratedOrchestrator> {
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
            let trimmed = out.trim();
            if trimmed != out {
                output_to_idx.entry(trimmed.to_string()).or_insert(idx);
            }
        }
    }
    let mut all_outputs: Vec<_> = output_to_idx.keys().cloned().collect();
    all_outputs.sort();

    // Build input-schema map with :any type as default
    let (input_schema, input_schema_value) = if external_inputs.is_empty() {
        ("{}".to_string(), None)
    } else {
        let mut schema_parts = Vec::new();
        let mut sorted_inputs: Vec<_> = external_inputs.iter().collect();
        sorted_inputs.sort();
        let mut map = HashMap::new();
        for input in sorted_inputs {
            let ty = infer_input_type(input);
            schema_parts.push(format!("    :{} :{}", input, ty));
            map.insert(
                rtfs::ast::MapKey::Keyword(rtfs::ast::Keyword(input.clone())),
                rtfs::runtime::values::Value::String(ty.to_string()),
            );
        }
        (
            format!("{{\n{}\n  }}", schema_parts.join("\n")),
            Some(rtfs::runtime::values::Value::Map(map)),
        )
    };

    // Build a proper RTFS 2.0 plan structure with input/output schemas
    let mut rtfs_code = String::new();
    rtfs_code.push_str(&format!("(plan \"{}\"\n", plan_id));
    rtfs_code.push_str("  :language rtfs20\n");
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
    let (output_schema_section, output_schema_value) = if !all_outputs.is_empty() {
        let mut parts = Vec::new();
        let mut map = HashMap::new();
        for key in &all_outputs {
            parts.push(format!("    :{} :any", key));
            map.insert(
                rtfs::ast::MapKey::Keyword(rtfs::ast::Keyword(key.clone())),
                rtfs::runtime::values::Value::String("any".to_string()),
            );
        }
        (
            format!("  :output-schema {{\n{}\n  }}\n", parts.join("\n")),
            Some(rtfs::runtime::values::Value::Map(map)),
        )
    } else {
        (
            "  :output-schema {\n    :result :any\n  }\n".to_string(),
            Some(rtfs::runtime::values::Value::Map(HashMap::from([(
                rtfs::ast::MapKey::Keyword(rtfs::ast::Keyword("result".to_string())),
                rtfs::runtime::values::Value::String("any".to_string()),
            )]))),
        )
    };

    rtfs_code.push_str(&output_schema_section);

    let mut body_code = String::new();
    body_code.push_str("(do\n");

    if resolved_steps.is_empty() {
        body_code.push_str("    (step \"No Steps\" {})\n");
    } else {
        body_code.push_str("    (let [\n");
        for idx in 0..resolved_steps.len() {
            let resolved = &resolved_steps[idx];
            let step_desc = &resolved.original.name;
            // For wiring, compute a map of available outputs from previous steps
            let mut prior_outputs: HashMap<String, usize> = HashMap::new();
            for (pidx, prev) in resolved_steps.iter().enumerate() {
                if pidx >= idx {
                    break;
                }
                for out in &prev.original.expected_outputs {
                    prior_outputs.insert(out.clone(), pidx);
                    let trimmed = out.trim();
                    if trimmed != out {
                        prior_outputs.entry(trimmed.to_string()).or_insert(pidx);
                    }
                }
            }
            let step_args = build_step_call_args(resolved_steps, idx, &prior_outputs)?;
            body_code.push_str(&format!(
                "      step_{} (step \"{}\" (call :{} {}))\n",
                idx,
                step_desc.replace("\"", "\\\""),
                resolved.capability_id,
                step_args
            ));
        }
        body_code.push_str("    ]\n");
        body_code.push_str("      {\n");
        for (i, key) in all_outputs.iter().enumerate() {
            let src_idx = output_to_idx.get(key).cloned().unwrap_or(0);
            let accessor = build_output_accessor(&resolved_steps[src_idx], key, src_idx);
            body_code.push_str(&format!("        :{} {}", key, accessor));
            if i < all_outputs.len() - 1 {
                body_code.push_str("\n");
            }
        }
        body_code.push_str("\n      })\n");
    }

    body_code.push_str("  )");

    let mut annotations_map = HashMap::new();
    annotations_map.insert(
        "goal".to_string(),
        rtfs::runtime::values::Value::String(goal.to_string()),
    );
    annotations_map.insert(
        "step_count".to_string(),
        rtfs::runtime::values::Value::Integer(resolved_steps.len() as i64),
    );

    let mut rtfs_code = String::new();
    rtfs_code.push_str(&format!("(plan \"{}\"\n", plan_id));
    rtfs_code.push_str("  :language rtfs20\n");
    if !cap_ids.is_empty() {
        let caps_vec = cap_ids
            .iter()
            .map(|id| format!("\"{}\"", id))
            .collect::<Vec<_>>()
            .join(" ");
        rtfs_code.push_str(&format!("  :capabilities-required [{}]\n", caps_vec));
    }
    rtfs_code.push_str(&format!("  :input-schema {}\n", input_schema));
    rtfs_code.push_str(&output_schema_section);
    rtfs_code.push_str(&format!(
        "  :annotations {{:goal \"{}\" :step_count {}}}\n",
        goal.replace("\"", "\\\""),
        resolved_steps.len()
    ));
    rtfs_code.push_str(&format!("  :body {}\n", body_code));
    rtfs_code.push_str(")\n");

    Ok(GeneratedOrchestrator {
        plan_rtfs: rtfs_code,
        body: body_code,
        input_schema: input_schema_value,
        output_schema: output_schema_value,
        capabilities_required: cap_ids,
        annotations: annotations_map,
    })
}

fn parse_input_assignment(raw: &str) -> (String, Option<String>) {
    let trimmed = raw.trim();
    if let Some((name, value)) = trimmed.split_once('=') {
        let key = name.trim();
        let val = value.trim();
        if key.is_empty() {
            (trimmed.to_string(), None)
        } else if val.is_empty() {
            (key.to_string(), None)
        } else {
            (key.to_string(), Some(val.to_string()))
        }
    } else {
        (trimmed.to_string(), None)
    }
}

fn literal_to_rtfs_literal(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        "\"\"".to_string()
    } else if trimmed.eq_ignore_ascii_case("true") || trimmed.eq_ignore_ascii_case("false") {
        trimmed.to_ascii_lowercase()
    } else if let Ok(int_val) = trimmed.parse::<i64>() {
        int_val.to_string()
    } else if trimmed.parse::<f64>().is_ok() && trimmed.contains('.') {
        trimmed.to_string()
    } else if trimmed.starts_with(':') {
        let keyword = trimmed.trim_start_matches(':');
        format!(":{}", sanitize_keyword(keyword))
    } else {
        format!(
            "\"{}\"",
            trimmed.replace('\\', "\\\\").replace('\"', "\\\"")
        )
    }
}

fn sanitize_symbol(name: &str) -> String {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        "value".to_string()
    } else {
        trimmed
            .chars()
            .map(|c| if c.is_whitespace() { '_' } else { c })
            .collect()
    }
}

fn build_step_call_args(
    resolved_steps: &[ResolvedStep],
    current_idx: usize,
    prior_outputs: &HashMap<String, usize>,
) -> DemoResult<String> {
    let resolved = &resolved_steps[current_idx];
    let step = &resolved.original;
    // Build map-based arguments without $ prefix: {:key1 val1 :key2 val2}
    if step.required_inputs.is_empty() {
        return Ok("{}".to_string());
    }

    let mut args_parts = vec!["{".to_string()];
    for (i, input) in step.required_inputs.iter().enumerate() {
        let (base_input, literal_value) = parse_input_assignment(input);
        let manifest_key_raw = resolved
            .input_bindings
            .get(input)
            .cloned()
            .or_else(|| resolved.input_bindings.get(&base_input).cloned())
            .unwrap_or_else(|| base_input.clone());
        let manifest_key = sanitize_keyword(&manifest_key_raw);
        if let Some(literal) = literal_value {
            let literal_code = literal_to_rtfs_literal(&literal);
            args_parts.push(format!("    :{} {}", manifest_key, literal_code));
        } else if let Some(pidx) = prior_outputs.get(&base_input) {
            let source_step = &resolved_steps[*pidx];
            let accessor = build_output_accessor(source_step, &base_input, *pidx);
            args_parts.push(format!("    :{} {}", manifest_key, accessor));
        } else {
            let symbol = sanitize_symbol(&base_input);
            args_parts.push(format!("    :{} {}", manifest_key, symbol));
        }
        if i < step.required_inputs.len() - 1 {
            args_parts.push("\n".to_string());
        }
    }
    args_parts.push("\n  }".to_string());

    Ok(args_parts.join(""))
}

fn build_output_accessor(step: &ResolvedStep, output_key: &str, step_idx: usize) -> String {
    let binding = step
        .output_bindings
        .get(output_key)
        .cloned()
        .unwrap_or(OutputBinding::MapKey(output_key.to_string()));
    format_output_accessor(step_idx, binding, output_key)
}

fn format_output_accessor(step_idx: usize, binding: OutputBinding, fallback_key: &str) -> String {
    match binding {
        OutputBinding::EntireValue => format!("step_{}", step_idx),
        OutputBinding::MapKey(actual_key) => {
            let actual_kw = sanitize_keyword(&actual_key);
            let fallback_kw = sanitize_keyword(fallback_key);
            if actual_kw == fallback_kw {
                format!(
                    "(let [res step_{idx}
                           res-map (if (map? res) res {{}})
                           outputs (if (map? res) (let [o (get res :outputs)] (if (map? o) o {{}})) {{}})]
                       (or (get res-map :{key}) (get outputs :{key}) res))",
                    idx = step_idx,
                    key = actual_kw
                )
            } else {
                format!(
                    "(let [res step_{idx}
                           res-map (if (map? res) res {{}})
                           outputs (if (map? res) (let [o (get res :outputs)] (if (map? o) o {{}})) {{}})]
                       (or (get res-map :{akey}) (get outputs :{akey})
                           (get res-map :{fkey}) (get outputs :{fkey})
                           res))",
                    idx = step_idx,
                    akey = actual_kw,
                    fkey = fallback_kw
                )
            }
        }
    }
}

fn sanitize_keyword(name: &str) -> String {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        "value".to_string()
    } else {
        trimmed
            .chars()
            .map(|c| if c.is_whitespace() { '-' } else { c })
            .collect()
    }
}

/// Heuristic input type inference from common parameter names.
fn infer_input_type(name: &str) -> &'static str {
    let n = name.trim().to_ascii_lowercase();
    match n.as_str() {
        // Strings
        "goal" | "origin" | "destination" | "dates" | "lodging_style" | "risk_profile"
        | "date_range" => "string",
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
        Ok(format!("    {{\n{}\n    }}", outputs.join("\n")))
    }
}

fn build_resolved_steps_metadata(resolved_steps: &[ResolvedStep]) -> Value {
    let entries: Vec<Value> = resolved_steps
        .iter()
        .enumerate()
        .map(|(idx, resolved)| {
            let mut map = HashMap::new();
            map.insert(MapKey::String("index".into()), Value::Integer(idx as i64));
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
                Value::String(
                    match resolved.resolution_strategy {
                        ResolutionStrategy::Found => "found",
                        ResolutionStrategy::Stubbed => "stubbed",
                        ResolutionStrategy::Synthesized => "synthesized",
                    }
                    .to_string(),
                ),
            );
            Value::Map(map)
        })
        .collect();
    Value::Vector(entries)
}

fn derive_orchestrator_capability_id(goal: &str, steps: &[ResolvedStep]) -> String {
    const MAX_CLASS_PARTS: usize = 2;

    let goal_sig = sanitize_identifier_for_id(&derive_goal_signature(goal));

    let mut seen = std::collections::HashSet::new();
    let mut class_parts: Vec<String> = Vec::new();
    for step in steps {
        if class_parts.len() >= MAX_CLASS_PARTS {
            break;
        }
        let token = abbreviate_capability_class_for_id(
            step.original.capability_class.trim(),
            step.capability_id.as_str(),
        );
        if !token.is_empty() && seen.insert(token.clone()) {
            class_parts.push(token);
        }
    }

    let base = if class_parts.is_empty() {
        format!("orchestrator.{}", goal_sig)
    } else {
        format!("orchestrator.{}.{}", goal_sig, class_parts.join("-"))
    };

    limit_id_length(&base, 120)
}

fn abbreviate_capability_class_for_id(class: &str, fallback: &str) -> String {
    let source = if class.is_empty() { fallback } else { class };
    let segments: Vec<&str> = source.split('.').filter(|s| !s.trim().is_empty()).collect();

    if segments.is_empty() {
        return sanitize_identifier_for_id(fallback);
    }

    let mut tokens = Vec::new();
    tokens.push(segments[0]);
    if segments.len() > 1 {
        let mut tail = segments[segments.len() - 1];
        if tail == segments[0] && segments.len() > 2 {
            tail = segments[segments.len() - 2];
        }
        if tail != segments[0] {
            tokens.push(tail);
        }
    }

    let abbreviated = tokens.join(".");
    sanitize_identifier_for_id(&abbreviated)
}

fn sanitize_identifier_for_id(value: &str) -> String {
    value
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else if c == '.' || c == '_' || c == '-' {
                c
            } else {
                '.'
            }
        })
        .collect::<String>()
        .trim_matches('.')
        .trim_matches('_')
        .trim_matches('-')
        .to_string()
}
fn derive_goal_signature(goal: &str) -> String {
    // Keep only the most salient tokens (alnum), drop common stopwords, join with dots
    const STOP: &[&str] = &[
        "a", "an", "and", "for", "from", "in", "of", "on", "the", "to", "with", "by", "those",
        "that", "this", "these", "is", "are", "be",
    ];
    let mut tokens: Vec<String> = goal
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter_map(|t| {
            let tk = t.trim().to_ascii_lowercase();
            if tk.is_empty() || STOP.contains(&tk.as_str()) {
                None
            } else {
                Some(tk)
            }
        })
        .collect();
    // Keep up to 5 tokens for brevity
    if tokens.len() > 5 {
        tokens.truncate(5);
    }
    if tokens.is_empty() {
        "goal".to_string()
    } else {
        tokens.join(".")
    }
}

fn limit_id_length(id: &str, max_len: usize) -> String {
    if id.len() <= max_len {
        return id.to_string();
    }
    // Simple tail hash to preserve uniqueness
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(id.as_bytes());
    let hash = format!("{:x}", hasher.finalize());
    let keep = max_len.saturating_sub(9); // 1 for '.', 8 for hash prefix
    format!("{}.{}", &id[..keep], &hash[..8])
}

async fn match_proposed_steps(
    ccos: &Arc<CCOS>,
    steps: &[ProposedStep],
) -> DemoResult<Vec<CapabilityMatch>> {
    let marketplace = ccos.get_capability_marketplace();
    let intent_graph = ccos.get_intent_graph();

    // Create discovery engine for enhanced capability search
    // Pass delegating arbiter if available for recursive synthesis
    let delegating_arbiter = ccos.get_delegating_engine();
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
        let existed_before = marketplace
            .get_capability(&step.capability_class)
            .await
            .is_some();

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

    print_normalization_telemetry();
}

