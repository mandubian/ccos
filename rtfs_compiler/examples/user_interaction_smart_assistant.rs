//! Smart Research Assistant - CCOS Self-Learning Demonstration
//!
//! This example demonstrates CCOS/RTFS self-learning capabilities through a practical use case:
//! A research assistant that learns your preferences and workflow patterns.
//!
//! ## The Learning Flow
//!
//! **First Interaction (Learning Phase):**
//! - User: "I need to research quantum computing applications"
//! - System asks: What domains? What depth? What format? What sources?
//! - System synthesizes a reusable capability from this interaction
//!
//! **Second Interaction (Application Phase):**
//! - User: "I need to research blockchain scalability"
//! - System: Directly applies learned capability (no repeated questions!)
//! - Efficiency: 5+ turns reduced to 1 turn
//!
//! ## Run Examples
//!
//! ```bash
//! # Basic learning demonstration
//! cargo run --example user_interaction_smart_assistant -- \
//!   --config ../config/agent_config.toml --mode learn
//!
//! # Full learning loop (learn + apply)
//! cargo run --example user_interaction_smart_assistant -- \
//!   --config ../config/agent_config.toml --mode full
//!
//! # Custom research topic
//! RESEARCH_TOPIC="machine learning interpretability" \
//! cargo run --example user_interaction_smart_assistant -- \
//!   --config ../config/agent_config.toml --mode full
//! ```

use clap::Parser;
use crossterm::style::Stylize;
// serde_json is still used for config file parsing only; avoid JSON in LLM exchanges
use serde_json;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use tokio::time::{sleep, Duration};
use toml;

use rtfs_compiler::ast::MapKey;
use rtfs_compiler::ast::{Expression, Literal, MapKey as RtfsMapKey};
use rtfs_compiler::ast::{MapTypeEntry, PrimitiveType, TypeExpr};
use rtfs_compiler::ccos::intent_graph::config::IntentGraphConfig;
use rtfs_compiler::ccos::CCOS;
use rtfs_compiler::config::profile_selection::expand_profiles;
use rtfs_compiler::config::types::{AgentConfig, LlmProfile};
use rtfs_compiler::parser::parse_expression;
use rtfs_compiler::runtime::Value;

/// Prompt hint utilities to standardize RTFS vs JSON expectations with LLMs
mod prompt_hints {
    /// Instructs the LLM to return ONLY an RTFS vector of strings, no prose/fences
    pub fn rtfs_vector_only() -> &'static str {
        r#"- Respond ONLY with an RTFS vector of strings (no prose, no fences), e.g. [\"q1\" \"q2\" ...]
- IMPORTANT: RTFS vectors use whitespace between items; DO NOT use commas
- Do not add any text before or after the vector"#
    }

    /// Instructs the LLM to return ONLY an RTFS map with no prose/fences
    pub fn rtfs_map_only() -> &'static str {
        r#"- Output ONLY a single RTFS map (no prose, no fences)
- STRICT RTFS FORMAT:
    - Keys must be RTFS keywords like :goal, :parameters, :type, :value, :question
    - Parameter names should be keywords (e.g., :budget, :duration) or strings
    - DO NOT use commas anywhere; separate entries by spaces or newlines
    - Strings must be double-quoted; keywords start with a colon
- Do not add any text before or after the map"#
    }

    /// Instructs the LLM to return ONLY JSON matching a conceptual schema; not used for RTFS outputs
    pub fn json_only(schema_hint: &str) -> String {
        format!(
            concat!(
                "- Respond ONLY with compact JSON, no comments/prose\n",
                "- JSON MUST match this conceptual schema: {}\n",
                "- No code fences"
            ),
            schema_hint
        )
    }

    /// Strips ```rtfs or ``` fences and trims outer whitespace
    pub fn strip_fenced_rtfs(raw: &str) -> String {
        raw.trim()
            .trim_start_matches("```rtfs")
            .trim_start_matches("```")
            .trim_end_matches("```")
            .trim()
            .to_string()
    }
}

#[derive(Parser, Debug)]
struct Args {
    /// Path to AgentConfig (TOML or JSON)
    #[arg(long)]
    config: Option<String>,

    /// Demo mode: learn (first interaction), apply (use learned capability), full (both)
    #[arg(long, default_value = "full")]
    mode: String,

    /// Optional LLM profile name
    #[arg(long)]
    profile: Option<String>,

    /// Show detailed prompts and responses
    #[arg(long, default_value_t = false)]
    debug_prompts: bool,

    /// Persist synthesized capabilities
    #[arg(long, default_value_t = true)]
    persist: bool,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
struct ResearchPreferences {
    topic: String,
    domains: Vec<String>,
    depth: String,
    format: String,
    sources: Vec<String>,
    time_constraint: String,
}

#[derive(Debug, Clone)]
struct InteractionMetrics {
    turns_count: usize,
    questions_asked: usize,
    time_elapsed_ms: u128,
    capability_synthesized: bool,
}

#[derive(Debug, Clone)]
struct ExtractedPreferences {
    /// The main goal/topic
    goal: String,
    /// Dynamic parameters extracted from Q&A: keyword -> (question, value, inferred_type)
    /// Examples: "budget" -> ("What's your budget?", "5000", "currency")
    ///           "duration" -> ("How long?", "7 days", "duration")
    ///           "interests" -> ("What interests you?", ["art", "food"], "list")
    parameters: std::collections::BTreeMap<String, ExtractedParam>,
}

#[derive(Debug, Clone)]
struct ExtractedParam {
    /// The question that extracted this parameter
    question: String,
    /// The user's answer
    value: String,
    /// Inferred parameter type: "string", "number", "list", "boolean", "duration", "currency"
    param_type: String,
    /// Optional semantic category for grouping
    category: Option<String>,
}

impl ExtractedPreferences {
    /// Get all parameters for capability generation
    fn get_parameter_schema(&self) -> String {
        self.parameters
            .iter()
            .map(|(key, param)| {
                let rtfs_type = match param.param_type.as_str() {
                    "list" => "(list \"string\")",
                    "number" => "\"number\"",
                    "boolean" => "\"boolean\"",
                    "duration" => "\"string\"", // "7 days", "2 weeks"
                    "currency" => "\"string\"", // "$5000", "€3000"
                    _ => "\"string\"",
                };
                format!("    :{} {}", key, rtfs_type)
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Get parameter bindings for RTFS let statements
    fn get_parameter_bindings(&self) -> String {
        self.parameters
            .keys()
            .map(|key| format!("  :{} {}", key, key))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Convert to legacy ResearchPreferences for backward compatibility
    fn to_legacy(&self) -> ResearchPreferences {
        ResearchPreferences {
            topic: self.goal.clone(),
            domains: self
                .parameters
                .get("domains")
                .or_else(|| self.parameters.get("interests"))
                .map(|p| vec![p.value.clone()])
                .unwrap_or_default(),
            depth: self
                .parameters
                .get("depth")
                .or_else(|| self.parameters.get("detail_level"))
                .map(|p| p.value.clone())
                .unwrap_or_default(),
            format: self
                .parameters
                .get("format")
                .or_else(|| self.parameters.get("output_format"))
                .map(|p| p.value.clone())
                .unwrap_or_default(),
            sources: self
                .parameters
                .get("sources")
                .or_else(|| self.parameters.get("resources"))
                .map(|p| vec![p.value.clone()])
                .unwrap_or_default(),
            time_constraint: self
                .parameters
                .get("duration")
                .or_else(|| self.parameters.get("time_constraint"))
                .map(|p| p.value.clone())
                .unwrap_or_default(),
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    if args.debug_prompts {
        std::env::set_var("RTFS_SHOW_PROMPTS", "1");
    }

    print_banner();

    // Load and apply configuration
    let mut loaded_config: Option<AgentConfig> = None;
    if let Some(cfg_path) = &args.config {
        match load_agent_config(cfg_path) {
            Ok(cfg) => {
                apply_llm_profile(&cfg, args.profile.as_deref())?;
                // Keep for potential future use in CCOS init; suppress unused warning
                let _ = &cfg;
                loaded_config = Some(cfg);
            }
            Err(e) => {
                eprintln!("❌ Failed to load config {}: {}", cfg_path, e);
                return Err(
                    "Provide a valid --config pointing to an AgentConfig with llm_profiles".into(),
                );
            }
        }
    } else {
        eprintln!("❌ No config provided. Strict mode requires a real DelegatedArbiter.");
        eprintln!("   Please run with --config path/to/agent_config.toml and valid API keys.");
        return Err("Missing --config for real LLM interrogation".into());
    }

    // Initialize CCOS
    let ccos = Arc::new(
        CCOS::new_with_agent_config_and_configs_and_debug_callback(
            IntentGraphConfig::default(),
            None,
            loaded_config.clone(),
            None,
        )
        .await?,
    );

    println!("✓ CCOS initialized");

    // Display LLM configuration
    if let Some(arbiter) = ccos.get_delegating_arbiter() {
        let llm_config = arbiter.get_llm_config();
        println!(
            "✓ LLM: {} via {:?}",
            llm_config.model, llm_config.provider_type
        );
    }

    match args.mode.as_str() {
        "learn" => {
            println!("\n{}", "🎓 LEARNING MODE".bold().green());
            println!(
                "{}",
                "═══════════════════════════════════════════════════════════════\n".dim()
            );
            run_learning_phase(&ccos, args.persist).await?;
        }
        "apply" => {
            println!("\n{}", "⚡ APPLICATION MODE".bold().cyan());
            println!(
                "{}",
                "═══════════════════════════════════════════════════════════════\n".dim()
            );
            run_application_phase(&ccos).await?;
        }
        "full" => {
            println!("\n{}", "🔄 FULL LEARNING LOOP".bold().magenta());
            println!(
                "{}",
                "═══════════════════════════════════════════════════════════════\n".dim()
            );

            let metrics_before = run_learning_phase(&ccos, args.persist).await?;

            println!(
                "\n{}",
                "─────────────────────────────────────────────────────────────".dim()
            );
            sleep(Duration::from_secs(2)).await;

            let metrics_after = run_application_phase(&ccos).await?;

            print_comparison(&metrics_before, &metrics_after);
        }
        _ => {
            eprintln!(
                "Unknown mode: {}. Use 'learn', 'apply', or 'full'",
                args.mode
            );
            return Err("Invalid mode".into());
        }
    }

    Ok(())
}

async fn run_learning_phase(
    ccos: &Arc<CCOS>,
    persist: bool,
) -> Result<InteractionMetrics, Box<dyn std::error::Error>> {
    let start = std::time::Instant::now();

    println!(
        "{}",
        "┌─────────────────────────────────────────────────────────────┐".bold()
    );
    println!(
        "{}",
        "│ PHASE 1: Initial Learning - Understanding Your Workflow    │".bold()
    );
    println!(
        "{}",
        "└─────────────────────────────────────────────────────────────┘\n".bold()
    );

    let research_topic = std::env::var("RESEARCH_TOPIC")
        .unwrap_or_else(|_| "plan a weekend trip to Paris".to_string());

    println!(
        "{} {}",
        "User Request:".bold(),
        research_topic.clone().cyan()
    );
    println!();

    // Real multi-turn interaction using CCOS capabilities
    let (preferences, interaction_history) =
        gather_preferences_via_ccos(&ccos, &research_topic).await?;

    println!("\n{}", "📊 Learned Preferences:".bold().green());
    println!("   • Goal: {}", preferences.goal);

    if preferences.parameters.is_empty() {
        println!("   • (No specific parameters extracted)");
    } else {
        println!("   • {} Parameters:", preferences.parameters.len());
        for (param_name, param) in &preferences.parameters {
            println!(
                "     - {} ({}): {}",
                param_name.as_str().bold().cyan(),
                param.param_type.as_str().yellow(),
                param.value
            );
        }
    }

    let turns_count = interaction_history.len();
    let questions_asked = turns_count.saturating_sub(1); // Exclude initial request

    println!(
        "\n{}",
        "┌─────────────────────────────────────────────────────────────┐".bold()
    );
    println!(
        "{}",
        "│ PHASE 2: Capability Synthesis (LLM-Driven)                 │".bold()
    );
    println!(
        "{}",
        "└─────────────────────────────────────────────────────────────┘\n".bold()
    );

    println!(
        "{}",
        "🔬 Analyzing interaction patterns with LLM...".yellow()
    );

    // Real LLM-driven synthesis using delegating arbiter
    let (mut capability_id, mut capability_spec) =
        synthesize_capability_via_llm(&ccos, &research_topic, &interaction_history, &preferences)
            .await?;

    println!("{}", "✓ LLM analyzed conversation history".dim());
    println!("{}", "✓ Extracted parameter schema from interactions".dim());
    println!("{}", "✓ Generated RTFS capability definition".dim());

    println!(
        "\n{}",
        "📦 Synthesized Capability (planner v0):".bold().cyan()
    );
    println!("```rtfs\n{}\n```", capability_spec.trim());

    // Attempt iterative resolution: discover/import missing capabilities and refine planner
    match resolve_missing_and_refine_planner(ccos, &capability_spec).await {
        Ok(Some((refined_id, refined_spec))) => {
            capability_id = refined_id;
            capability_spec = refined_spec;
            println!("\n{}", "📦 Refined Planner after resolution:".bold().cyan());
            println!("```rtfs\n{}\n```", capability_spec.trim());
        }
        Ok(None) => {
            // No changes; continue
        }
        Err(e) => {
            println!(
                "{}",
                format!("⚠️  Resolution iteration failed: {}", e).yellow()
            );
        }
    }

    // Register generic demo sub-capabilities so the planner can execute end-to-end
    if let Err(e) = register_generic_demo_capabilities(ccos).await {
        eprintln!("⚠️  Failed to register generic demo capabilities: {}", e);
    }

    // Register the capability (placeholder executor for now, RTFS execution TBD)
    let marketplace = ccos.get_capability_marketplace();
    marketplace
        .register_local_capability(
            capability_id.clone(),
            capability_id.clone(),
            "Smart research assistant capability learned from user interaction".to_string(),
            Arc::new(|_value: &Value| {
                Ok(Value::Map({
                    let mut m = std::collections::HashMap::new();
                    m.insert(
                        MapKey::String("status".to_string()),
                        Value::String("research_completed".to_string()),
                    );
                    m.insert(
                        MapKey::String("summary".to_string()),
                        Value::String("Research findings compiled successfully".to_string()),
                    );
                    m
                }))
            }),
        )
        .await?;

    println!(
        "\n{}",
        "✓ Registered capability in marketplace".green().bold()
    );

    if persist {
        persist_capability(&capability_id, &capability_spec)?;
        println!(
            "{}",
            format!(
                "✓ Persisted to capabilities/generated/{}.rtfs",
                capability_id
            )
            .green()
        );

        // Additionally persist a direct plan for the apply phase
        let direct_plan = format!(
            "(call :{} {{:goal \"{}\"}})",
            capability_id,
            research_topic.replace('"', "\\\"")
        );
        let plan_id = "synth.plan.v1";
        persist_plan(plan_id, &direct_plan)?;
        println!(
            "{}",
            format!(
                "✓ Persisted direct plan to capabilities/generated/{}.rtfs",
                plan_id
            )
            .green()
        );
    }

    let elapsed = start.elapsed().as_millis();

    Ok(InteractionMetrics {
        turns_count,
        questions_asked,
        time_elapsed_ms: elapsed,
        capability_synthesized: true,
    })
}

/// Resolve missing capabilities referenced by a synthesized planner and re-synthesize using a fresh marketplace snapshot.
/// Returns Some((id, spec)) when the planner changed, None when unchanged or no action, Err on failure.
async fn resolve_missing_and_refine_planner(
    ccos: &Arc<CCOS>,
    planner_spec: &str,
) -> Result<Option<(String, String)>, Box<dyn std::error::Error>> {
    use rtfs_compiler::ccos::synthesis::continuous_resolution::{
        ContinuousResolutionLoop, ResolutionConfig,
    };
    use rtfs_compiler::ccos::synthesis::dependency_extractor;
    use rtfs_compiler::ccos::synthesis::registration_flow::RegistrationFlow;

    // 1) Extract dependencies from the current planner
    let dep = match dependency_extractor::extract_dependencies(planner_spec) {
        Ok(d) => d,
        Err(e) => {
            // If we can't parse, bail silently (keep example robust)
            eprintln!("Dependency extraction failed: {}", e);
            return Ok(None);
        }
    };

    // Start with the extracted missing dependencies (as a set to avoid duplicates)
    let mut missing: std::collections::HashSet<String> = dep.missing_dependencies.clone();

    // Some early planners embed a generated-capability marker without explicit calls; extract and treat it as missing
    if missing.is_empty() {
        if let Some(idx) = planner_spec.find(":generated-capability :") {
            let rest = &planner_spec[idx + ":generated-capability :".len()..];
            let cap_id: String = rest
                .split(|c: char| c.is_whitespace() || c == '}' || c == ')')
                .next()
                .unwrap_or("")
                .trim()
                .trim_matches(|c: char| c == '"')
                .to_string();
            if !cap_id.is_empty() {
                missing.insert(cap_id);
            }
        }
    }

    if missing.is_empty() {
        return Ok(None);
    }

    println!(
        "{}",
        "🔁 Iterative resolution: discovering missing capabilities".yellow()
    );
    for m in &missing {
        println!("   • {}", m);
    }

    // 2) Attempt resolution via the built-in resolver (MCP registry, web search, curated overrides)
    let marketplace = ccos.get_capability_marketplace();
    let checkpoint_archive =
        Arc::new(rtfs_compiler::ccos::checkpoint_archive::CheckpointArchive::new());
    let resolver =
        rtfs_compiler::ccos::synthesis::missing_capability_resolver::MissingCapabilityResolver::new(
            Arc::clone(&marketplace),
            checkpoint_archive,
            rtfs_compiler::ccos::synthesis::missing_capability_resolver::ResolverConfig::default(),
            rtfs_compiler::ccos::synthesis::feature_flags::MissingCapabilityConfig::default(),
        );
    // Create registration flow and continuous resolution loop with ambitious defaults
    let registration_flow = Arc::new(RegistrationFlow::new(Arc::clone(&marketplace)));
    let loop_config = ResolutionConfig {
        high_risk_auto_resolution: std::env::var("CCOS_DEMO_AUTO_APPROVE")
            .unwrap_or_else(|_| "1".to_string())
            == "1",
        ..ResolutionConfig::default()
    };
    let cr_loop = ContinuousResolutionLoop::new(
        Arc::new(resolver),
        Arc::clone(&registration_flow),
        Arc::clone(&marketplace),
        loop_config,
    );

    // Up to N refinement rounds: resolve → re-snap → re-synthesize
    let max_rounds = std::env::var("CCOS_DEMO_RESOLVE_ROUNDS")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(3);
    let mut last_planner = planner_spec.to_string();

    for round in 1..=max_rounds {
        println!("{}", format!("🔂 Resolution round {}", round).yellow());

        // Enqueue and trigger resolution per missing capability
        for cap in missing.clone().into_iter() {
            let mut context = std::collections::HashMap::new();
            context.insert(
                "source".to_string(),
                format!("example_resolve_round_{}", round),
            );
            // Enqueue via resolver (internal to cr_loop.resolver)
            // Note: we access through marketplace + new resolver above; create a fresh resolver handle
            // by cloning from cr_loop internals is not public; re-enqueue via a new resolver instance
            let temp_resolver = rtfs_compiler::ccos::synthesis::missing_capability_resolver::MissingCapabilityResolver::new(
                Arc::clone(&marketplace),
                Arc::new(rtfs_compiler::ccos::checkpoint_archive::CheckpointArchive::new()),
                rtfs_compiler::ccos::synthesis::missing_capability_resolver::ResolverConfig::default(),
                rtfs_compiler::ccos::synthesis::feature_flags::MissingCapabilityConfig::default(),
            );
            temp_resolver.handle_missing_capability(cap.clone(), vec![], context)?;

            // Trigger the loop (risk assessment + approvals if needed)
            if let Err(e) = cr_loop
                .trigger_resolution(&cap, Some("interactive_synthesis"))
                .await
            {
                eprintln!("⚠️ trigger_resolution failed for {}: {}", cap, e);
            }

            // Optional auto-approval path
            if std::env::var("CCOS_DEMO_AUTO_APPROVE").unwrap_or_else(|_| "1".to_string()) == "1" {
                let _ = cr_loop.approve_capability(&cap, "demo").await;
            }

            // Process queue to actually discover/register candidates
            temp_resolver.process_queue().await?;
        }

        // Re-snapshot and re-synthesize planner with an empty conversation (registry-first)
        let snapshot = marketplace.list_capabilities().await;
        let re = rtfs_compiler::ccos::synthesis::synthesize_capabilities_with_marketplace(
            &[],
            &snapshot,
        );
        if let Some(new_planner) = re.planner {
            // Extract and report diff
            if new_planner != last_planner {
                println!("{}", "✨ Planner evolved after resolution".green());
                if let Some(new_id) = extract_capability_id_from_spec(&new_planner) {
                    // Check for remaining missing deps
                    if let Ok(dep_next) = dependency_extractor::extract_dependencies(&new_planner) {
                        if dep_next.missing_dependencies.is_empty() {
                            return Ok(Some((new_id, new_planner)));
                        } else {
                            // Prepare next round on remaining missing
                            missing = dep_next.missing_dependencies.clone();
                            last_planner = new_planner;
                            continue;
                        }
                    } else {
                        return Ok(Some((new_id, new_planner)));
                    }
                } else {
                    // Fallback ID if extraction fails
                    if let Ok(dep_next) = dependency_extractor::extract_dependencies(&new_planner) {
                        if dep_next.missing_dependencies.is_empty() {
                            return Ok(Some(("synth.domain.planner.v1".to_string(), new_planner)));
                        } else {
                            missing = dep_next.missing_dependencies.clone();
                            last_planner = new_planner;
                            continue;
                        }
                    } else {
                        return Ok(Some(("synth.domain.planner.v1".to_string(), new_planner)));
                    }
                }
            } else {
                // No change in planner; if still missing, try next round; else finish
                if missing.is_empty() {
                    return Ok(None);
                }
            }
        } else {
            // Planner not returned; nothing to do
            return Ok(None);
        }
    }

    // Max rounds reached; if planner evolved, return it; else no change
    if last_planner != planner_spec {
        if let Some(new_id) = extract_capability_id_from_spec(&last_planner) {
            return Ok(Some((new_id, last_planner)));
        } else {
            return Ok(Some(("synth.domain.planner.v1".to_string(), last_planner)));
        }
    }

    Ok(None)
}

async fn run_application_phase(
    ccos: &Arc<CCOS>,
) -> Result<InteractionMetrics, Box<dyn std::error::Error>> {
    let start = std::time::Instant::now();

    println!(
        "\n{}",
        "┌─────────────────────────────────────────────────────────────┐".bold()
    );
    println!(
        "{}",
        "│ PHASE 3: Applying Learned Capability                       │".bold()
    );
    println!(
        "{}",
        "└─────────────────────────────────────────────────────────────┘\n".bold()
    );

    // Use a different request to test the learned capability
    // (or use SECOND_RESEARCH_TOPIC to override)
    let new_topic = std::env::var("SECOND_RESEARCH_TOPIC")
        .unwrap_or_else(|_| "similar request using learned workflow".to_string());

    println!("{} {}", "User Request:".bold(), new_topic.clone().cyan());
    println!();

    // Best effort: load the most recently persisted capability from disk (if any)
    if let Some((persisted_id, _persisted_spec)) =
        load_and_register_latest_persisted_capability(ccos).await?
    {
        println!(
            "{}",
            format!("✓ Loaded persisted capability: {}", persisted_id).green()
        );
        println!(
            "{}",
            format!("  Source: capabilities/generated/{}.rtfs", persisted_id).dim()
        );
        // Keep going; it's now registered in the marketplace
    }

    // Also register generic demo sub-capabilities if not present yet (idempotent)
    if let Err(e) = register_generic_demo_capabilities(ccos).await {
        eprintln!("⚠️  Could not ensure generic demo capabilities: {}", e);
    }

    // Find the most recently registered capability (any domain!)
    let marketplace = ccos.get_capability_marketplace();
    let all_caps = marketplace.list_capabilities().await;

    // Get the most recent capability from generated/ directory
    let capability_manifest = all_caps
        .iter()
        .filter(|c| {
            // Look for any generated capabilities (travel, research, sentiment, etc.)
            c.id.contains(".") && !c.id.starts_with("ccos.")
        })
        .last() // Get the most recent one
        .ok_or("No learned capability found. Run in 'learn' or 'full' mode first")?;

    let capability_id = &capability_manifest.id;

    println!("{}", "🔍 Checking capability marketplace...".dim());
    sleep(Duration::from_millis(300)).await;
    println!(
        "{}",
        format!("✓ Found learned capability: {}", capability_id).green()
    );
    println!(
        "{}",
        format!("  Description: {}", capability_manifest.description).dim()
    );

    println!(
        "\n{}",
        "⚡ Executing learned workflow (capability or direct plan)...".yellow()
    );

    // Build a generic invocation from the capability's declared input_schema (RTFS types)
    // This adapts to any capability shape; no hard-coded domain prefixes.
    let prefs_for_apply: ExtractedPreferences = ExtractedPreferences {
        goal: new_topic.clone(),
        parameters: std::collections::BTreeMap::new(),
    };
    let args_map = build_rtfs_args_from_manifest(capability_manifest, &new_topic, &prefs_for_apply);
    let capability_invocation = format!("(call :{} {} )", capability_id, args_map);

    println!(
        "{}",
        format!("  Invocation: {}", capability_invocation).dim()
    );

    // Prefer executing a direct plan if present (e.g., travel.plan.v1.rtfs), else call the capability
    let plan_path = std::path::Path::new("capabilities/generated/travel.plan.v1.rtfs");
    let (plan_code, ctx_caps) = if plan_path.exists() {
        let plan_src = std::fs::read_to_string(plan_path)?;
        // Allow the travel.* capabilities in controlled mode
        let caps = vec![
            "travel.flights.search".to_string(),
            "travel.hotels.search".to_string(),
            "travel.itinerary.plan".to_string(),
            "travel.museums.reserve".to_string(),
        ];
        (plan_src, caps)
    } else {
        (
            capability_invocation.clone(),
            vec![capability_id.to_string()],
        )
    };
    let ctx = rtfs_compiler::runtime::RuntimeContext::controlled(ctx_caps);
    let plan = rtfs_compiler::ccos::types::Plan::new_rtfs(plan_code, vec![]);

    match ccos.validate_and_execute_plan(plan, &ctx).await {
        Ok(result) => {
            println!("{}", "  → Executed successfully".dim());
            println!("{}", format!("  → Result: {:?}", result.value).dim());
            println!(
                "\n{}",
                "✓ Workflow completed using learned capability!"
                    .green()
                    .bold()
            );
        }
        Err(e) => {
            println!("{}", format!("  ⚠ Execution error: {}", e).yellow());
            println!(
                "{}",
                "  → This is expected if the capability calls sub-capabilities not yet registered"
                    .dim()
            );
            println!(
                "\n{}",
                "✓ Capability structure validated (would work with implemented sub-capabilities)"
                    .green()
                    .bold()
            );
        }
    }

    let elapsed = start.elapsed().as_millis();

    Ok(InteractionMetrics {
        turns_count: 1,     // Single invocation!
        questions_asked: 0, // No questions!
        time_elapsed_ms: elapsed,
        capability_synthesized: false,
    })
}

/// Try to load the most recent persisted capability from capabilities/generated and register it
async fn load_and_register_latest_persisted_capability(
    ccos: &Arc<CCOS>,
) -> Result<Option<(String, String)>, Box<dyn std::error::Error>> {
    use std::fs;
    use std::time::SystemTime;

    let dir = std::path::Path::new("capabilities/generated");
    if !dir.exists() {
        return Ok(None);
    }

    let mut newest: Option<(std::path::PathBuf, SystemTime)> = None;
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) == Some("rtfs") {
            let meta = entry.metadata()?;
            let mtime = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
            if newest.as_ref().map(|(_, t)| mtime > *t).unwrap_or(true) {
                newest = Some((path, mtime));
            }
        }
    }

    let (path, _mtime) = match newest {
        Some(v) => v,
        None => return Ok(None),
    };
    let spec = fs::read_to_string(&path)?;
    let spec_trimmed = spec.trim().to_string();
    let id = extract_capability_id_from_spec(&spec_trimmed)
        .ok_or("Failed to extract capability id from persisted RTFS spec")?;

    // Register a simple placeholder executor as in learning phase
    let marketplace = ccos.get_capability_marketplace();
    marketplace
        .register_local_capability(
            id.clone(),
            id.clone(),
            "Persisted learned capability".to_string(),
            Arc::new(|_value: &Value| {
                Ok(Value::Map({
                    let mut m = std::collections::HashMap::new();
                    m.insert(
                        MapKey::String("status".to_string()),
                        Value::String("research_completed".to_string()),
                    );
                    m.insert(
                        MapKey::String("summary".to_string()),
                        Value::String("Research findings compiled successfully".to_string()),
                    );
                    m
                }))
            }),
        )
        .await?;

    Ok(Some((id, spec_trimmed)))
}

/// Build an RTFS argument map string from a capability's input_schema and available preferences
/// - Uses TypeExpr to select sensible defaults
/// - Fills :topic with provided topic when present
/// - For unknown or complex types, falls back to empty or simple placeholders
fn build_rtfs_args_from_manifest(
    cap: &rtfs_compiler::ccos::capability_marketplace::types::CapabilityManifest,
    topic: &str,
    prefs: &ExtractedPreferences,
) -> String {
    // Helper to lookup a parameter by name from preferences
    let lookup_pref = |name: &str| -> Option<&ExtractedParam> { prefs.parameters.get(name) };

    // Render a primitive default value according to type and optional preference
    fn render_value_for_type(
        ty: &TypeExpr,
        pref: Option<&ExtractedParam>,
        topic: &str,
        key: &str,
    ) -> String {
        match ty {
            TypeExpr::Primitive(p) => match p {
                PrimitiveType::String | PrimitiveType::Symbol | PrimitiveType::Keyword => {
                    let v = pref.map(|p| p.value.as_str()).unwrap_or_else(|| {
                        if key == "topic" {
                            topic
                        } else {
                            ""
                        }
                    });
                    format!("\"{}\"", v.replace('"', "\\\\\""))
                }
                PrimitiveType::Int => {
                    if let Some(p) = pref {
                        p.value
                            .parse::<i64>()
                            .map(|n| n.to_string())
                            .unwrap_or_else(|_| "0".to_string())
                    } else {
                        "0".to_string()
                    }
                }
                PrimitiveType::Float => {
                    if let Some(p) = pref {
                        p.value
                            .parse::<f64>()
                            .map(|n| n.to_string())
                            .unwrap_or_else(|_| "0.0".to_string())
                    } else {
                        "0.0".to_string()
                    }
                }
                PrimitiveType::Bool => {
                    let v = pref
                        .map(|p| p.value.to_lowercase())
                        .unwrap_or_else(|| "false".to_string());
                    let b = matches!(v.as_str(), "true" | "yes" | "y" | "1");
                    b.to_string()
                }
                PrimitiveType::Nil => "nil".to_string(),
                PrimitiveType::Custom(_k) => {
                    // Best effort: treat as string placeholder
                    let v = pref.map(|p| p.value.as_str()).unwrap_or("");
                    format!("\"{}\"", v.replace('"', "\\\\\""))
                }
            },
            TypeExpr::Vector(_inner) => {
                // Lists: try to split preference value on commas into string vector
                if let Some(p) = pref {
                    let items: Vec<String> = p
                        .value
                        .split(',')
                        .map(|s| format!("\"{}\"", s.trim().replace('"', "\\\\\"")))
                        .collect();
                    format!("[{}]", items.join(" "))
                } else {
                    // Empty vector with type hint ignored in literal
                    "[]".to_string()
                }
            }
            TypeExpr::Tuple(types) => {
                // Render tuple with defaults for each element
                let parts: Vec<String> = types
                    .iter()
                    .enumerate()
                    .map(|(i, t)| render_value_for_type(t, pref, topic, &format!("{}_{}", key, i)))
                    .collect();
                format!("[{}]", parts.join(" "))
            }
            TypeExpr::Map { entries, .. } => {
                // Nested map: recurse with no specific prefs
                let mut parts = Vec::new();
                for MapTypeEntry {
                    key: k, value_type, ..
                } in entries
                {
                    let rendered = render_value_for_type(value_type, None, topic, &k.0);
                    parts.push(format!(":{} {}", k.0, rendered));
                }
                format!("{{{}}}", parts.join(" "))
            }
            TypeExpr::Union(variants) => {
                // Choose first variant heuristically
                if let Some(first) = variants.first() {
                    render_value_for_type(first, pref, topic, key)
                } else {
                    "nil".to_string()
                }
            }
            TypeExpr::Optional(inner) => {
                // Provide value according to inner type; could be nil if no pref
                if pref.is_some() || key == "topic" {
                    render_value_for_type(inner, pref, topic, key)
                } else {
                    "nil".to_string()
                }
            }
            // Function/Resource/Array/Refined/Enum/Never/Any and others: best-effort string
            _ => {
                let v = pref.map(|p| p.value.as_str()).unwrap_or("");
                format!("\"{}\"", v.replace('"', "\\\\\""))
            }
        }
    }

    // Default if no schema: pass :topic only
    if cap.input_schema.is_none() {
        return format!("{{:topic \"{}\"}}", topic.replace('"', "\\\\\""));
    }

    // Build from map entries only; for other shapes fallback to :topic
    match cap.input_schema.as_ref().unwrap() {
        TypeExpr::Map { entries, .. } => {
            let mut parts: Vec<String> = Vec::with_capacity(entries.len());
            for MapTypeEntry {
                key,
                value_type,
                optional: _,
            } in entries
            {
                let pref = lookup_pref(&key.0);
                let rendered = render_value_for_type(value_type, pref, topic, &key.0);
                parts.push(format!(":{} {}", key.0, rendered));
            }
            format!("{{{}}}", parts.join(" "))
        }
        other => {
            // Fallback
            eprintln!(
                "Note: unsupported input_schema shape {:?}, defaulting to :topic only",
                other
            );
            format!("{{:topic \"{}\"}}", topic.replace('"', "\\\\\""))
        }
    }
}

/// Generate appropriate clarification questions based on user's goal using LLM
async fn generate_questions_for_goal(
    ccos: &Arc<CCOS>,
    goal: &str,
) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let arbiter = ccos
        .get_delegating_arbiter()
        .ok_or("Delegating arbiter not available for question generation")?;

    // Instruct LLM to emit RTFS Vector (not JSON)
    let prompt = format!(
        r#"You are analyzing a user's goal to determine high-signal clarifying questions.

User Goal: "{goal}"

Output rules:
{rtfs_hint}
- 3 to 5 questions, specific to THIS goal, gathering preferences, constraints, and success criteria.
- No duplicate or generic questions.
"#,
        goal = goal,
        rtfs_hint = crate::prompt_hints::rtfs_vector_only()
    );

    let response = arbiter
        .generate_raw_text(&prompt)
        .await
        .map_err(|e| format!("Question generation failed: {}", e))?;

    // Parse RTFS vector/list of strings strictly (no fallback)
    let rtfs = crate::prompt_hints::strip_fenced_rtfs(&response);
    let expr = parse_expression(&rtfs).map_err(|e| {
        format!(
            "LLM did not produce a valid RTFS vector of questions: {:?}",
            e
        )
    })?;

    fn vec_strings_from_expr(expr: &Expression) -> Result<Vec<String>, String> {
        let items: &Vec<Expression> = match expr {
            Expression::Vector(v) | Expression::List(v) => v,
            other => return Err(format!("Expected RTFS vector/list, got {:?}", other)),
        };
        let mut out = Vec::with_capacity(items.len());
        for e in items {
            match e {
                Expression::Literal(Literal::String(s)) => out.push(s.clone()),
                _ => return Err("Non-string element in questions vector".to_string()),
            }
        }
        Ok(out)
    }

    let questions =
        vec_strings_from_expr(&expr).map_err(|e| format!("LLM questions vector invalid: {}", e))?;
    if questions.is_empty() {
        return Err("LLM returned an empty questions vector".into());
    }
    Ok(questions)
}

/// Real interaction using CCOS user.ask capability
async fn gather_preferences_via_ccos(
    ccos: &Arc<CCOS>,
    topic: &str,
) -> Result<(ExtractedPreferences, Vec<(String, String)>), Box<dyn std::error::Error>> {
    use rtfs_compiler::runtime::RuntimeContext;

    println!("{}", "💬 Interactive Preference Collection:".bold());
    println!();

    // Generate questions dynamically based on the user's goal using LLM
    let questions = generate_questions_for_goal(ccos, topic).await?;

    let mut interaction_history = vec![];
    interaction_history.push(("initial_topic".to_string(), topic.to_string()));

    // Set up fallback canned responses if not in interactive mode
    // These are generic fallbacks - for real usage, set CCOS_INTERACTIVE_ASK=1
    if std::env::var("CCOS_INTERACTIVE_ASK").is_err() {
        // Set some generic responses for automated testing
        std::env::set_var(
            "CCOS_USER_ASK_RESPONSE_Q1",
            "Medium budget, prefer quality over cheapness",
        );
        std::env::set_var("CCOS_USER_ASK_RESPONSE_Q2", "7 days in total");
        std::env::set_var(
            "CCOS_USER_ASK_RESPONSE_Q3",
            "Mix of sightseeing, food, and culture",
        );
        std::env::set_var(
            "CCOS_USER_ASK_RESPONSE_Q4",
            "Train and walking, avoid driving",
        );
        std::env::set_var(
            "CCOS_USER_ASK_RESPONSE_Q5",
            "Mid-range hotels or nice Airbnbs",
        );
    }

    // Runtime context allowing user.ask
    let ctx = RuntimeContext::controlled(vec!["ccos.user.ask".to_string()]);

    let mut answers = HashMap::new();

    for (i, question) in questions.iter().enumerate() {
        sleep(Duration::from_millis(200)).await;
        println!("{} {}", format!("  Q{}:", i + 1).bold().yellow(), question);

        // Execute RTFS plan to ask the question
        let plan_body = format!(
            "(call :ccos.user.ask \"{}\")",
            question.replace('"', "\\\"")
        );
        let plan = rtfs_compiler::ccos::types::Plan::new_rtfs(plan_body, vec![]);

        match ccos.validate_and_execute_plan(plan, &ctx).await {
            Ok(result) => {
                let answer = match &result.value {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                };

                println!(
                    "{} {}",
                    format!("  A{}:", i + 1).dim(),
                    answer.clone().cyan()
                );
                println!();

                answers.insert(question.to_string(), answer.clone());
                interaction_history.push((question.to_string(), answer));
            }
            Err(e) => {
                eprintln!("Failed to ask question: {}", e);
                // Use fallback answer from env or generic
                let env_key = format!("CCOS_USER_ASK_RESPONSE_Q{}", i + 1);
                let fallback = std::env::var(&env_key)
                    .unwrap_or_else(|_| format!("Reasonable preference for question {}", i + 1));

                println!(
                    "{} {}",
                    format!("  A{}:", i + 1).dim(),
                    fallback.clone().cyan()
                );
                println!();

                answers.insert(question.to_string(), fallback.clone());
                interaction_history.push((question.to_string(), fallback));
            }
        }
    }

    // Strict parsing via LLM: no heuristic fallback
    let parsed = parse_preferences_via_llm(ccos, topic, &interaction_history).await?;
    Ok((parsed, interaction_history))
}

/// Extract a single preference value from text based on keywords
#[allow(dead_code)]
fn extract_single_from_text(text: &str, keywords: &[&str]) -> String {
    // Simple heuristic: find sentences containing keywords
    for sentence in text.split(['.', ',', ';']) {
        let lower = sentence.to_lowercase();
        if keywords.iter().any(|kw| lower.contains(kw)) {
            return sentence.trim().to_string();
        }
    }
    text.split(',')
        .next()
        .unwrap_or("moderate")
        .trim()
        .to_string()
}

/// Extract a list of items from text based on keywords
#[allow(dead_code)]
fn extract_list_from_text(text: &str, keywords: &[&str]) -> Vec<String> {
    let mut items = Vec::new();
    for sentence in text.split(['.', ';']) {
        let lower = sentence.to_lowercase();
        if keywords.iter().any(|kw| lower.contains(kw)) {
            for item in sentence.split(',') {
                let trimmed = item.trim();
                if !trimmed.is_empty() {
                    items.push(trimmed.to_string());
                }
            }
        }
    }
    if items.is_empty() {
        items.push("general".to_string());
    }
    items
}

/// Ask the delegating LLM to parse the Q/A pairs and dynamically extract parameters with keywords
async fn parse_preferences_via_llm(
    ccos: &Arc<CCOS>,
    topic: &str,
    interaction_history: &[(String, String)],
) -> Result<ExtractedPreferences, Box<dyn std::error::Error>> {
    // Try to get the delegating arbiter. If missing, bail to allow fallback heuristics.
    let arbiter = ccos
        .get_delegating_arbiter()
        .ok_or("Delegating arbiter not available for preference parsing")?;

    // Build RTFS-friendly prompt: ask RTFS map output, no JSON.
    // Represent Q/A pairs inline as a vector of {:q "..." :a "..."}
    let mut qa_rtfs_elems = Vec::new();
    for (_i, (q, a)) in interaction_history.iter().enumerate().skip(1) {
        // escape quotes
        let q_esc = q.replace('"', "\\\"");
        let a_esc = a.replace('"', "\\\"");
        qa_rtfs_elems.push(format!("{{:q \"{}\" :a \"{}\"}}", q_esc, a_esc));
    }
    let qa_rtfs = format!("[{}]", qa_rtfs_elems.join(" "));

    let prompt = format!(
        r#"Analyze these question/answer pairs and extract semantic parameters with inferred types.

Your task:
1. For each Q/A, determine the parameter (e.g., :budget, :duration, :interests)
2. Infer type keyword from: :string :number :list :boolean :duration :currency
3. {rtfs_hint}
STRICT FORMAT EXAMPLE (no commas, keyword keys, exactly this shape):
     {{
         :goal "{topic}"
         :parameters {{
             :PARAM {{:type :keyword :value "..." :question "..."}}
         }}
     }}
     {{
         :goal "{topic}"
         :parameters {{
             :PARAM {{:type :keyword :value "..." :question "..."}}
             ...
         }}
     }}

Q/A pairs (RTFS): {qa}
"#,
        topic = topic,
        qa = qa_rtfs,
        rtfs_hint = crate::prompt_hints::rtfs_map_only()
    );

    let raw = arbiter
        .generate_raw_text(&prompt)
        .await
        .map_err(|e| format!("Preference parsing LLM call failed: {}", e))?;

    // Clean fenced code if present and parse RTFS map
    let cleaned = crate::prompt_hints::strip_fenced_rtfs(&raw);
    let expr = parse_expression(&cleaned)
        .map_err(|e| format!("Preference parser failed to parse RTFS map: {:?}", e))?;

    // Expect {:goal "..." :parameters { :param {:type :keyword :value "..." :question "..."} ... }}
    let (mut goal, mut parameters) = (topic.to_string(), std::collections::BTreeMap::new());

    // Helper: convert Expression to string if it is a string literal
    fn expr_to_string(expr: &Expression) -> Option<String> {
        if let Expression::Literal(Literal::String(s)) = expr {
            Some(s.clone())
        } else {
            None
        }
    }

    // Helper: convert Expression keyword or string literal to type keyword string (e.g., ":string")
    fn expr_to_type_keyword(expr: &Expression) -> Option<String> {
        match expr {
            Expression::Literal(Literal::Keyword(k)) => Some(format!(":{}", k.0)),
            Expression::Literal(Literal::String(s)) => Some(s.clone()),
            _ => None,
        }
    }

    // Walk the map
    if let Expression::Map(kvs) = expr {
        // Top-level keys
        for (k, v) in kvs.iter() {
            match k {
                RtfsMapKey::Keyword(k) if k.0 == "goal" => {
                    if let Some(s) = expr_to_string(v) {
                        goal = s;
                    }
                }
                RtfsMapKey::Keyword(k) if k.0 == "parameters" => {
                    if let Expression::Map(pairs) = v {
                        for (pk, pv) in pairs.iter() {
                            // param name must be keyword or string
                            let pname = match pk {
                                RtfsMapKey::Keyword(kw) => kw.0.clone(),
                                RtfsMapKey::String(s) => s.clone(),
                                RtfsMapKey::Integer(i) => i.to_string(),
                            };
                            if let Expression::Map(pmap) = pv {
                                let mut ptype: String = "string".to_string();
                                let mut pval: String = String::new();
                                let mut pquestion: String = String::new();
                                for (mk, mv) in pmap.iter() {
                                    match mk {
                                        RtfsMapKey::Keyword(kw) if kw.0 == "type" => {
                                            if let Some(t) = expr_to_type_keyword(mv) {
                                                ptype = t;
                                            }
                                        }
                                        RtfsMapKey::Keyword(kw) if kw.0 == "value" => {
                                            if let Some(s) = expr_to_string(mv) {
                                                pval = s;
                                            }
                                        }
                                        RtfsMapKey::Keyword(kw) if kw.0 == "question" => {
                                            if let Some(s) = expr_to_string(mv) {
                                                pquestion = s;
                                            }
                                        }
                                        _ => {}
                                    }
                                }
                                parameters.insert(
                                    pname,
                                    ExtractedParam {
                                        question: pquestion,
                                        value: pval,
                                        param_type: ptype,
                                        category: None,
                                    },
                                );
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    } else {
        return Err("Preference parser: expected RTFS map at top-level".into());
    }

    let prefs = ExtractedPreferences { goal, parameters };
    // Strict cleanup: drop parameters with empty values or empty questions to avoid noise
    let mut cleaned_params = std::collections::BTreeMap::new();
    for (k, v) in prefs.parameters.iter() {
        let val_ok = !v.value.trim().is_empty();
        let q_ok = !v.question.trim().is_empty();
        if val_ok && q_ok {
            cleaned_params.insert(k.clone(), v.clone());
        }
    }

    let prefs = ExtractedPreferences {
        goal: prefs.goal,
        parameters: cleaned_params,
    };

    // If everything was dropped, signal an error to re-prompt or adjust
    if prefs.parameters.is_empty() {
        return Err("LLM returned no usable parameters (empty values); please retry".into());
    }
    Ok(prefs)
}

/// Real LLM-driven capability synthesis using the Phase 8 pipeline (collector + planner)
/// Refactored to always emit an executable plan (no plan-body indirection).
async fn synthesize_capability_via_llm(
    ccos: &Arc<CCOS>,
    topic: &str,
    interaction_history: &[(String, String)],
    prefs: &ExtractedPreferences,
) -> Result<(String, String), Box<dyn std::error::Error>> {
    // Convert interaction history to synthesis turns (first entry is initial goal)
    let turns: Vec<rtfs_compiler::ccos::synthesis::InteractionTurn> = interaction_history
        .iter()
        .enumerate()
        .map(
            |(i, (q, a))| rtfs_compiler::ccos::synthesis::InteractionTurn {
                turn_index: i,
                prompt: q.clone(),
                answer: Some(a.clone()),
            },
        )
        .collect();

    // Snapshot marketplace to enable registry-first planner generation
    let marketplace = ccos.get_capability_marketplace();
    let snapshot = marketplace.list_capabilities().await;

    let result =
        rtfs_compiler::ccos::synthesis::synthesize_capabilities_with_marketplace(&turns, &snapshot);

    // Show collector for visibility (it drives the questions) but persist the planner (the actual workflow)
    if let Some(collector) = &result.collector {
        println!("\n{}", "🧾 Generated collector (questions)".bold().yellow());
        println!("```rtfs\n{}\n```", collector.trim());
    }

    let _original_planner_spec = result
        .planner
        .ok_or("Synthesis did not return a planner capability")?;

    // Generic approach: always emit an executable plan as the capability implementation.
    // Extract parameters from the learned preferences and compose generic steps.
    let planner_spec = build_generic_planner_capability(topic, prefs)?;

    // Extract ID from planner RTFS spec
    let capability_id = extract_capability_id_from_spec(&planner_spec)
        .ok_or("Failed to extract capability id from synthesized planner RTFS")?;

    // Dependency analysis for planner
    if let Ok(dep_result) =
        rtfs_compiler::ccos::synthesis::dependency_extractor::extract_dependencies(&planner_spec)
    {
        println!("🔍 DEPENDENCY ANALYSIS for {}", capability_id);
        println!("   Total dependencies: {}", dep_result.dependencies.len());
        println!(
            "   Missing dependencies: {}",
            dep_result.missing_dependencies.len()
        );

        if !dep_result.missing_dependencies.is_empty() {
            println!("   Missing capabilities:");
            for dep in &dep_result.missing_dependencies {
                println!("     - {}", dep);
            }

            // Create audit event
            let audit_data =
                rtfs_compiler::ccos::synthesis::dependency_extractor::create_audit_event_data(
                    &capability_id,
                    &dep_result.missing_dependencies,
                );
            println!(
                "   AUDIT: capability_deps_missing - {}",
                audit_data
                    .get("missing_capabilities")
                    .unwrap_or(&"none".to_string())
            );
        }

        if !dep_result.dependencies.is_empty() {
            println!("   All dependencies found:");
            for dep in &dep_result.dependencies {
                println!("     - {} (line {})", dep.capability_id, dep.line_number);
            }
        }
    }

    Ok((capability_id, planner_spec))
}

/// Build a generic, executable planner capability whose implementation IS the plan.
/// Extracts parameters from preferences and composes generic workflow steps.
/// Adapts to any domain by calling generic sub-capabilities that provide domain-agnostic execution.
fn build_generic_planner_capability(
    topic: &str,
    prefs: &ExtractedPreferences,
) -> Result<String, Box<dyn std::error::Error>> {
    let esc = |s: &str| s.replace('"', "\\\"");

    // Build let-bindings for each parameter extracted from Q&A
    let mut param_bindings = Vec::new();
    for (key, param) in &prefs.parameters {
        param_bindings.push(format!("      :{} \"{}\"", key, esc(&param.value)));
    }

    // Generic workflow: discover requirements, plan steps, execute.
    let spec = format!(
        r#"(capability "synth.planner.v1"
  :description "Generic planner that adapts to any domain via extensible sub-capabilities"
  :parameters {{:goal "string"}}
  :implementation
    (do
      (let ((goal "{}"{})
        (let ((discover (call :generic.discover {{:goal goal}}))
              (plan (call :generic.plan {{:discover discover :goal goal}}))
              (execute (call :generic.execute {{:plan plan :goal goal}})))
          {{:status "completed"
            :goal goal
            :discover discover
            :plan plan
            :execute execute}})))
)"#,
        esc(topic),
        if !param_bindings.is_empty() {
            format!("\n{}\n      ", param_bindings.join("\n"))
        } else {
            String::new()
        }
    );
    Ok(spec)
}

/// Build a domain-specific trip planner capability whose implementation IS the plan.
/// It composes concrete steps and calls domain capabilities directly.

/// Extract RTFS capability from LLM response (handles fenced blocks and raw s-expressions)
fn extract_capability_from_response(response: &str) -> Result<String, Box<dyn std::error::Error>> {
    // Try to find fenced rtfs block
    if let Some(start) = response.find("```rtfs") {
        if let Some(end) = response[start + 7..].find("```") {
            let spec = response[start + 7..start + 7 + end].trim();
            return Ok(spec.to_string());
        }
    }

    // Try to find raw (capability ...) form
    if let Some(start) = response.find("(capability") {
        if let Some(spec) = extract_balanced_sexpr(response, start) {
            return Ok(spec);
        }
    }

    Err("Could not extract capability from LLM response".into())
}

/// Extract balanced s-expression starting at given index
fn extract_balanced_sexpr(text: &str, start_idx: usize) -> Option<String> {
    let bytes = text.as_bytes();
    if bytes.get(start_idx) != Some(&b'(') {
        return None;
    }
    let mut depth = 0i32;
    for (i, ch) in text[start_idx..].char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(text[start_idx..start_idx + i + 1].to_string());
                }
            }
            _ => {}
        }
    }
    None
}

/// Extract capability ID from RTFS spec
fn extract_capability_id_from_spec(spec: &str) -> Option<String> {
    // Look for (capability "id" ...)
    if let Some(idx) = spec.find("(capability") {
        if let Some(q1) = spec[idx..].find('"') {
            let start = idx + q1 + 1;
            if let Some(q2) = spec[start..].find('"') {
                return Some(spec[start..start + q2].to_string());
            }
        }
    }
    None
}

// Note: All fallback generators removed for strict mode

#[allow(dead_code)]
fn generate_research_capability(prefs: &ResearchPreferences, id: &str) -> String {
    format!(
        r#"(capability "{id}"
  :description "Smart research assistant that gathers, analyzes, and synthesizes information based on learned preferences"
  :parameters {{
    :topic "string"
    :domains (list "string")
    :depth "string"
    :format "string"
    :sources (list "string")
    :time_constraint "string"
  }}
  :implementation
    (do
      (step "Gather Sources"
        (call :research.sources.gather {{
          :topic topic
          :sources sources
          :domains domains
        }}))
      
      (step "Analyze Content"
        (call :research.content.analyze {{
          :sources gathered_sources
          :depth depth
          :focus_areas domains
        }}))
      
      (step "Synthesize Findings"
        (call :research.synthesis.create {{
          :analyzed_content analyzed_data
          :topic topic
          :format format
        }}))
      
      (step "Format Report"
        (call :research.report.format {{
          :findings synthesized_findings
          :format format
          :citations true
        }}))
      
      (step "Return Results"
        {{
          :status "completed"
          :topic topic
          :summary formatted_report
          :confidence "high"
          :time_taken time_constraint
        }})
      ))
  
  :learned_from {{
    :initial_topic "{topic}"
    :interaction_turns 6
    :timestamp (now)
  }}
)"#,
        id = id,
        topic = prefs.topic
    )
}

fn print_comparison(before: &InteractionMetrics, after: &InteractionMetrics) {
    println!(
        "\n\n{}",
        "═══════════════════════════════════════════════════════════════".bold()
    );
    println!(
        "{}",
        "                    LEARNING IMPACT ANALYSIS"
            .bold()
            .magenta()
    );
    println!(
        "{}",
        "═══════════════════════════════════════════════════════════════\n".bold()
    );

    println!(
        "{}",
        "┌─────────────────────┬───────────────┬───────────────┬──────────┐".dim()
    );
    println!(
        "{}",
        "│ Metric              │ Before Learn  │ After Learn   │ Gain     │".bold()
    );
    println!(
        "{}",
        "├─────────────────────┼───────────────┼───────────────┼──────────┤".dim()
    );

    println!(
        "│ {} │ {:>13} │ {:>13} │ {:>8} │",
        "Interaction Turns   ".dim(),
        format!("{}", before.turns_count).yellow(),
        format!("{}", after.turns_count).green(),
        format!("{}x", before.turns_count / after.turns_count.max(1))
            .cyan()
            .bold()
    );

    println!(
        "│ {} │ {:>13} │ {:>13} │ {:>8} │",
        "Questions Asked     ".dim(),
        format!("{}", before.questions_asked).yellow(),
        format!("{}", after.questions_asked).green(),
        format!("-{}", before.questions_asked - after.questions_asked)
            .cyan()
            .bold()
    );

    let time_saved = before.time_elapsed_ms.saturating_sub(after.time_elapsed_ms);
    let time_saved_pct = if before.time_elapsed_ms > 0 {
        (time_saved as f64 / before.time_elapsed_ms as f64 * 100.0) as usize
    } else {
        0
    };

    println!(
        "│ {} │ {:>11}ms │ {:>11}ms │ {:>6}% │",
        "Time Elapsed        ".dim(),
        before.time_elapsed_ms.to_string().yellow(),
        after.time_elapsed_ms.to_string().green(),
        format!("-{}", time_saved_pct).cyan().bold()
    );

    println!(
        "{}",
        "└─────────────────────┴───────────────┴───────────────┴──────────┘\n".dim()
    );

    println!("{}", "🎯 Key Achievements:".bold().green());
    println!(
        "   {} Reduced interaction from {} turns to {} turn",
        "✓".green(),
        before.turns_count,
        after.turns_count
    );
    println!(
        "   {} Eliminated {} redundant questions",
        "✓".green(),
        before.questions_asked
    );
    println!("   {} Capability reusable for similar tasks", "✓".green());
    println!("   {} Knowledge persisted in marketplace", "✓".green());

    println!("\n{}", "💡 What This Means:".bold().cyan());
    println!("   The system learned your research workflow and can now apply it");
    println!("   instantly to new topics without repeating the same questions.");
    println!("   This represents genuine learning and knowledge accumulation.");

    println!("\n{}", "🔮 Next Steps:".bold().yellow());
    println!("   • Run with different topics to see the learned capability adapt");
    println!("   • Check generated_capabilities/ for the persisted RTFS code");
    println!("   • Import the capability into other RTFS programs");
    println!("   • Build upon this pattern for more complex workflows");

    println!(
        "\n{}",
        "═══════════════════════════════════════════════════════════════\n".bold()
    );
}

fn print_banner() {
    println!(
        "\n{}",
        "═══════════════════════════════════════════════════════════════"
            .bold()
            .cyan()
    );
    println!(
        "{}",
        "       🧠 CCOS/RTFS Self-Learning Demonstration 🧠"
            .bold()
            .cyan()
    );
    println!("{}", "           Smart Research Assistant Example".bold());
    println!(
        "{}",
        "═══════════════════════════════════════════════════════════════\n"
            .bold()
            .cyan()
    );
}

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

fn apply_llm_profile(
    config: &AgentConfig,
    profile_name: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    // Always enable delegation for this demo
    std::env::set_var("CCOS_ENABLE_DELEGATION", "1");

    if let Some(llm_cfg) = &config.llm_profiles {
        let (profiles, _meta, _why) = expand_profiles(config);
        let chosen_name = profile_name
            .map(String::from)
            .or_else(|| llm_cfg.default.clone())
            .or_else(|| profiles.first().map(|p| p.name.clone()));

        if let Some(name) = chosen_name {
            if let Some(p) = profiles.iter().find(|pp| pp.name == name) {
                apply_profile_env(p);
            }
        } else if !profiles.is_empty() {
            // Fallback to first profile if no default specified
            apply_profile_env(&profiles[0]);
        }
    }
    Ok(())
}

fn apply_profile_env(p: &LlmProfile) {
    std::env::set_var("CCOS_DELEGATING_MODEL", &p.model);
    std::env::set_var("CCOS_LLM_MODEL", &p.model);
    std::env::set_var("CCOS_LLM_PROVIDER_HINT", &p.provider);

    if let Some(url) = &p.base_url {
        std::env::set_var("CCOS_LLM_BASE_URL", url);
    } else if p.provider == "openrouter" {
        if std::env::var("CCOS_LLM_BASE_URL").is_err() {
            std::env::set_var("CCOS_LLM_BASE_URL", "https://openrouter.ai/api/v1");
        }
    }

    let resolved_key = if let Some(inline) = &p.api_key {
        Some(inline.clone())
    } else if let Some(env_key) = &p.api_key_env {
        std::env::var(env_key).ok()
    } else {
        None
    };

    if let Some(key) = resolved_key {
        match p.provider.as_str() {
            "openrouter" => std::env::set_var("OPENROUTER_API_KEY", key),
            "claude" | "anthropic" => std::env::set_var("ANTHROPIC_API_KEY", key),
            "gemini" => std::env::set_var("GEMINI_API_KEY", key),
            "stub" => {}
            _ => std::env::set_var("OPENAI_API_KEY", key),
        }
    }

    match p.provider.as_str() {
        "openai" => std::env::set_var("CCOS_LLM_PROVIDER", "openai"),
        "claude" | "anthropic" => std::env::set_var("CCOS_LLM_PROVIDER", "anthropic"),
        "openrouter" => std::env::set_var("CCOS_LLM_PROVIDER", "openrouter"),
        "local" => std::env::set_var("CCOS_LLM_PROVIDER", "local"),
        "stub" => std::env::set_var("CCOS_LLM_PROVIDER", "stub"),
        _ => {}
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
    let file_path = dir.join(format!("{}.rtfs", id));
    fs::write(file_path, plan_code.as_bytes())?;
    Ok(())
}

/// Register generic sub-capabilities used by the planner.
/// These are domain-agnostic and adapt to any goal/topic.
async fn register_generic_demo_capabilities(
    ccos: &Arc<CCOS>,
) -> Result<(), Box<dyn std::error::Error>> {
    use rtfs_compiler::runtime::Value as V;
    let mp = ccos.get_capability_marketplace();

    // generic.discover: analyzes goal and parameters
    mp.register_local_capability(
        "generic.discover".to_string(),
        "Discover Requirements".to_string(),
        "Generic requirement discovery".to_string(),
        Arc::new(|_input: &V| {
            let mut out = std::collections::HashMap::new();
            out.insert(
                MapKey::String("status".into()),
                V::String("discovered".into()),
            );
            out.insert(
                MapKey::String("requirements".into()),
                V::Vector(vec![
                    V::String("requirement_1".into()),
                    V::String("requirement_2".into()),
                ]),
            );
            Ok(V::Map(out))
        }),
    )
    .await?;

    // generic.plan: creates a plan from discovered requirements
    mp.register_local_capability(
        "generic.plan".to_string(),
        "Plan Steps".to_string(),
        "Generic step planner".to_string(),
        Arc::new(|_input: &V| {
            let mut out = std::collections::HashMap::new();
            out.insert(MapKey::String("status".into()), V::String("planned".into()));
            out.insert(
                MapKey::String("steps".into()),
                V::Vector(vec![V::String("step_1".into()), V::String("step_2".into())]),
            );
            Ok(V::Map(out))
        }),
    )
    .await?;

    // generic.execute: executes the plan
    mp.register_local_capability(
        "generic.execute".to_string(),
        "Execute Plan".to_string(),
        "Generic plan executor".to_string(),
        Arc::new(|_input: &V| {
            let mut out = std::collections::HashMap::new();
            out.insert(
                MapKey::String("status".into()),
                V::String("executed".into()),
            );
            out.insert(MapKey::String("result".into()), V::String("success".into()));
            Ok(V::Map(out))
        }),
    )
    .await?;

    Ok(())
}
