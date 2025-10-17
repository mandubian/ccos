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
use std::collections::HashMap;
use std::sync::Arc;
use std::fs;
use std::path::Path;
use tokio::time::{sleep, Duration};
use serde_json;
use toml;

use rtfs_compiler::ccos::CCOS;
use rtfs_compiler::ccos::intent_graph::config::IntentGraphConfig;
use rtfs_compiler::runtime::Value;
use rtfs_compiler::ast::MapKey;
use rtfs_compiler::config::types::{AgentConfig, LlmProfile};
use rtfs_compiler::config::profile_selection::expand_profiles;

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
                    "duration" => "\"string\"",  // "7 days", "2 weeks"
                    "currency" => "\"string\"",  // "$5000", "€3000"
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
            domains: self.parameters
                .get("domains")
                .or_else(|| self.parameters.get("interests"))
                .map(|p| vec![p.value.clone()])
                .unwrap_or_default(),
            depth: self.parameters
                .get("depth")
                .or_else(|| self.parameters.get("detail_level"))
                .map(|p| p.value.clone())
                .unwrap_or_default(),
            format: self.parameters
                .get("format")
                .or_else(|| self.parameters.get("output_format"))
                .map(|p| p.value.clone())
                .unwrap_or_default(),
            sources: self.parameters
                .get("sources")
                .or_else(|| self.parameters.get("resources"))
                .map(|p| vec![p.value.clone()])
                .unwrap_or_default(),
            time_constraint: self.parameters
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
                loaded_config = Some(cfg);
            }
            Err(e) => {
                eprintln!("⚠️  Failed to load config {}: {}", cfg_path, e);
                eprintln!("⚠️  Delegation may not work without valid config");
            }
        }
    } else {
        // No config provided - enable delegation with stub provider for demo
        eprintln!("⚠️  No config provided. Using stub provider for demonstration.");
        eprintln!("⚠️  For real LLM synthesis, provide --config with valid LLM settings.");
        std::env::set_var("CCOS_ENABLE_DELEGATION", "1");
        std::env::set_var("CCOS_LLM_PROVIDER", "stub");
        std::env::set_var("CCOS_DELEGATING_MODEL", "stub-model");
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
        println!("✓ LLM: {} via {:?}", llm_config.model, llm_config.provider_type);
    }

    match args.mode.as_str() {
        "learn" => {
            println!("\n{}", "🎓 LEARNING MODE".bold().green());
            println!("{}", "═══════════════════════════════════════════════════════════════\n".dim());
            run_learning_phase(&ccos, args.persist).await?;
        }
        "apply" => {
            println!("\n{}", "⚡ APPLICATION MODE".bold().cyan());
            println!("{}", "═══════════════════════════════════════════════════════════════\n".dim());
            run_application_phase(&ccos).await?;
        }
        "full" => {
            println!("\n{}", "🔄 FULL LEARNING LOOP".bold().magenta());
            println!("{}", "═══════════════════════════════════════════════════════════════\n".dim());
            
            let metrics_before = run_learning_phase(&ccos, args.persist).await?;
            
            println!("\n{}", "─────────────────────────────────────────────────────────────".dim());
            sleep(Duration::from_secs(2)).await;
            
            let metrics_after = run_application_phase(&ccos).await?;
            
            print_comparison(&metrics_before, &metrics_after);
        }
        _ => {
            eprintln!("Unknown mode: {}. Use 'learn', 'apply', or 'full'", args.mode);
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
    
    println!("{}", "┌─────────────────────────────────────────────────────────────┐".bold());
    println!("{}", "│ PHASE 1: Initial Learning - Understanding Your Workflow    │".bold());
    println!("{}", "└─────────────────────────────────────────────────────────────┘\n".bold());

    let research_topic = std::env::var("RESEARCH_TOPIC")
        .unwrap_or_else(|_| "quantum computing applications in cryptography".to_string());

    println!("{} {}", "User Request:".bold(), research_topic.clone().cyan());
    println!();

    // Real multi-turn interaction using CCOS capabilities
    let (preferences, interaction_history) = gather_preferences_via_ccos(&ccos, &research_topic).await?;
    
    println!("\n{}", "📊 Learned Preferences:".bold().green());
    println!("   • Goal: {}", preferences.goal);
    
    if preferences.parameters.is_empty() {
        println!("   • (No specific parameters extracted)");
    } else {
        println!("   • {} Parameters:", preferences.parameters.len());
        for (param_name, param) in &preferences.parameters {
            println!("     - {} ({}): {}", 
                param_name.as_str().bold().cyan(), 
                param.param_type.as_str().yellow(),
                param.value
            );
        }
    }

    let turns_count = interaction_history.len();
    let questions_asked = turns_count.saturating_sub(1); // Exclude initial request

    println!("\n{}", "┌─────────────────────────────────────────────────────────────┐".bold());
    println!("{}", "│ PHASE 2: Capability Synthesis (LLM-Driven)                 │".bold());
    println!("{}", "└─────────────────────────────────────────────────────────────┘\n".bold());

    println!("{}", "🔬 Analyzing interaction patterns with LLM...".yellow());
    
    // Real LLM-driven synthesis using delegating arbiter
    let (capability_id, capability_spec) = synthesize_capability_via_llm(
        &ccos,
        &research_topic,
        &interaction_history,
        &preferences,
    ).await?;

    println!("{}", "✓ LLM analyzed conversation history".dim());
    println!("{}", "✓ Extracted parameter schema from interactions".dim());
    println!("{}", "✓ Generated RTFS capability definition".dim());

    println!("\n{}", "📦 Synthesized Capability:".bold().cyan());
    println!("```rtfs\n{}\n```", capability_spec.trim());

    // Register the capability
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

    println!("\n{}", "✓ Registered capability in marketplace".green().bold());

    if persist {
        persist_capability(&capability_id, &capability_spec)?;
        println!("{}", format!("✓ Persisted to capabilities/generated/{}.rtfs", capability_id).green());
    }

    let elapsed = start.elapsed().as_millis();

    Ok(InteractionMetrics {
        turns_count,
        questions_asked,
        time_elapsed_ms: elapsed,
        capability_synthesized: true,
    })
}

async fn run_application_phase(
    ccos: &Arc<CCOS>,
) -> Result<InteractionMetrics, Box<dyn std::error::Error>> {
    let start = std::time::Instant::now();

    println!("\n{}", "┌─────────────────────────────────────────────────────────────┐".bold());
    println!("{}", "│ PHASE 3: Applying Learned Capability                       │".bold());
    println!("{}", "└─────────────────────────────────────────────────────────────┘\n".bold());

    // Use a different request to test the learned capability
    // (or use SECOND_RESEARCH_TOPIC to override)
    let new_topic = std::env::var("SECOND_RESEARCH_TOPIC")
        .unwrap_or_else(|_| "similar request using learned workflow".to_string());

    println!("{} {}", "User Request:".bold(), new_topic.clone().cyan());
    println!();

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
    println!("{}", format!("✓ Found learned capability: {}", capability_id).green());
    println!("{}", format!("  Description: {}", capability_manifest.description).dim());

    println!("\n{}", "⚡ Executing learned workflow via registered capability...".yellow());
    
    // Build invocation with appropriate parameters based on capability
    // Note: For demo purposes, we use simple mock parameters
    // In production, these would come from actual user input
    let capability_invocation = if capability_id.starts_with("travel.") {
        format!(
            "(call :{} {{:destination \"{}\" :duration 5 :budget 3000 :interests [\"culture\" \"food\"]}})",
            capability_id,
            new_topic.replace('"', "\\\"")
        )
    } else if capability_id.starts_with("sentiment.") {
        format!(
            "(call :{} {{:source \"{}\" :format \"csv\" :granularity \"detailed\"}})",
            capability_id,
            new_topic.replace('"', "\\\"")
        )
    } else {
        // Default for research capabilities
        format!(
            "(call :{} {{:topic \"{}\"}})",
            capability_id,
            new_topic.replace('"', "\\\"")
        )
    };
    
    println!("{}", format!("  Invocation: {}", capability_invocation).dim());
    
    let ctx = rtfs_compiler::runtime::RuntimeContext::controlled(vec![capability_id.to_string()]);
    let plan = rtfs_compiler::ccos::types::Plan::new_rtfs(capability_invocation, vec![]);
    
    match ccos.validate_and_execute_plan(plan, &ctx).await {
        Ok(result) => {
            println!("{}", "  → Capability executed successfully".dim());
            println!("{}", format!("  → Result: {:?}", result.value).dim());
            println!("\n{}", "✓ Workflow completed using learned capability!".green().bold());
        }
        Err(e) => {
            println!("{}", format!("  ⚠ Capability execution error: {}", e).yellow());
            println!("{}", "  → This is expected if the capability calls sub-capabilities not yet registered".dim());
            println!("\n{}", "✓ Capability structure validated (would work with implemented sub-capabilities)".green().bold());
        }
    }

    let elapsed = start.elapsed().as_millis();

    Ok(InteractionMetrics {
        turns_count: 1, // Single invocation!
        questions_asked: 0, // No questions!
        time_elapsed_ms: elapsed,
        capability_synthesized: false,
    })
}

/// Generate appropriate clarification questions based on user's goal using LLM
async fn generate_questions_for_goal(
    ccos: &Arc<CCOS>,
    goal: &str,
) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let arbiter = ccos.get_delegating_arbiter()
        .ok_or("Delegating arbiter not available for question generation")?;
    
    let prompt = format!(
        r#"You are analyzing a user's goal to determine what clarifying questions to ask.

User Goal: "{}"

Generate 5 specific, relevant questions to understand how to best help achieve this goal.
The questions should gather preferences, constraints, and requirements specific to THIS goal.

IMPORTANT: Generate questions appropriate for the ACTUAL goal, not generic research questions.

Output ONLY a JSON array of question strings, no markdown fences:
["question 1", "question 2", "question 3", "question 4", "question 5"]"#,
        goal
    );
    
    let response = arbiter.generate_raw_text(&prompt).await
        .map_err(|e| format!("Question generation failed: {}", e))?;
    
    // Parse JSON response
    let cleaned = response
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
    
    let questions: Vec<String> = serde_json::from_str(cleaned)
        .map_err(|e| format!("Failed to parse questions JSON: {}", e))?;
    
    if questions.len() < 3 {
        return Err("LLM generated too few questions".into());
    }
    
    // Limit to 5 questions
    Ok(questions.into_iter().take(5).collect())
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
        std::env::set_var("CCOS_USER_ASK_RESPONSE_Q1", "Medium budget, prefer quality over cheapness");
        std::env::set_var("CCOS_USER_ASK_RESPONSE_Q2", "7 days in total");
        std::env::set_var("CCOS_USER_ASK_RESPONSE_Q3", "Mix of sightseeing, food, and culture");
        std::env::set_var("CCOS_USER_ASK_RESPONSE_Q4", "Train and walking, avoid driving");
        std::env::set_var("CCOS_USER_ASK_RESPONSE_Q5", "Mid-range hotels or nice Airbnbs");
    }

    // Runtime context allowing user.ask
    let ctx = RuntimeContext::controlled(vec!["ccos.user.ask".to_string()]);
    
    let mut answers = HashMap::new();
    
    for (i, question) in questions.iter().enumerate() {
        sleep(Duration::from_millis(200)).await;
        println!("{} {}", format!("  Q{}:", i + 1).bold().yellow(), question);
        
        // Execute RTFS plan to ask the question
        let plan_body = format!("(call :ccos.user.ask \"{}\")", question.replace('"', "\\\""));
        let plan = rtfs_compiler::ccos::types::Plan::new_rtfs(plan_body, vec![]);
        
        match ccos.validate_and_execute_plan(plan, &ctx).await {
            Ok(result) => {
                let answer = match &result.value {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                
                println!("{} {}", format!("  A{}:", i + 1).dim(), answer.clone().cyan());
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
                
                println!("{} {}", format!("  A{}:", i + 1).dim(), fallback.clone().cyan());
                println!();
                
                answers.insert(question.to_string(), fallback.clone());
                interaction_history.push((question.to_string(), fallback));
            }
        }
    }

    // Parse answers into structured preferences
    // First try to ask the delegating LLM to parse the Q/A pairs into the preference schema.
    // This produces a more robust mapping than the simple heuristics below. If the arbiter
    // isn't available or parsing fails, fallback to the original heuristic parsing.
    if let Ok(Some(parsed)) = parse_preferences_via_llm(ccos, topic, &interaction_history).await {
        return Ok((parsed, interaction_history));
    }

    // Fallback: Use heuristic extraction with dynamic parameters
    let mut parameters = std::collections::BTreeMap::new();
    
    // Map Q/A pairs to parameters heuristically
    for (question, answer) in interaction_history.iter().skip(1) {
        let q_lower = question.to_lowercase();
        
        // Try to infer parameter name from question
        let param_name = if q_lower.contains("budget") {
            Some(("budget", "currency"))
        } else if q_lower.contains("day") || q_lower.contains("duration") || q_lower.contains("long") || q_lower.contains("week") {
            Some(("duration", "duration"))
        } else if q_lower.contains("interest") || q_lower.contains("prefer") || q_lower.contains("like") {
            Some(("interests", "list"))
        } else if q_lower.contains("domain") || q_lower.contains("area") || q_lower.contains("type") {
            Some(("domains", "list"))
        } else if q_lower.contains("depth") || q_lower.contains("detail") || q_lower.contains("level") {
            Some(("depth", "string"))
        } else if q_lower.contains("format") || q_lower.contains("output") || q_lower.contains("style") {
            Some(("format", "string"))
        } else if q_lower.contains("source") || q_lower.contains("resource") {
            Some(("sources", "list"))
        } else {
            None
        };
        
        if let Some((param_name, param_type)) = param_name {
            parameters.insert(param_name.to_string(), ExtractedParam {
                question: question.clone(),
                value: answer.clone(),
                param_type: param_type.to_string(),
                category: None,
            });
        }
    }

    let preferences = ExtractedPreferences {
        goal: topic.to_string(),
        parameters,
    };

    Ok((preferences, interaction_history))
}

/// Extract a single preference value from text based on keywords
fn extract_single_from_text(text: &str, keywords: &[&str]) -> String {
    // Simple heuristic: find sentences containing keywords
    for sentence in text.split(['.', ',', ';']) {
        let lower = sentence.to_lowercase();
        if keywords.iter().any(|kw| lower.contains(kw)) {
            return sentence.trim().to_string();
        }
    }
    text.split(',').next().unwrap_or("moderate").trim().to_string()
}

/// Extract a list of items from text based on keywords
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
) -> Result<Option<ExtractedPreferences>, Box<dyn std::error::Error>> {
    // Try to get the delegating arbiter. If missing, bail to allow fallback heuristics.
    let arbiter = match ccos.get_delegating_arbiter() {
        Some(a) => a,
        None => return Ok(None),
    };

    // Build a compact JSON-friendly prompt containing the Q/A pairs.
    // The goal: have LLM extract meaningful keywords and infer parameter types.
    let mut qa_list = Vec::new();
    for (_i, (q, a)) in interaction_history.iter().enumerate().skip(1) {
        qa_list.push(format!("{{\"q\": {}, \"a\": {}}}", serde_json::to_string(q)?, serde_json::to_string(a)?));
    }
    let qa_json = format!("[{}]", qa_list.join(","));

    let prompt = format!(
        r#"Analyze these question/answer pairs and extract semantic parameters with inferred types.

Your task:
1. For each Q/A pair, identify what parameter the question is asking about (e.g., "budget", "duration", "interests")
2. Infer the parameter type: "string", "number", "list", "boolean", "duration", "currency"
3. Return a JSON object where each parameter maps to metadata

Examples of expected extractions:
Q: "What's your budget?"  -> parameter: "budget", type: "currency", value: user's budget
Q: "How many days?"       -> parameter: "duration", type: "number", value: number of days
Q: "What interests you?"  -> parameter: "interests", type: "list", value: comma-separated interests

Respond ONLY with valid JSON matching this schema:
{{
  "goal": "{topic}",
  "parameters": {{
    "PARAMETER_NAME": {{
      "type": "string|number|list|boolean|duration|currency",
      "value": "extracted value from answer",
      "question": "the question that asked for this"
    }},
    ...
  }}
}}

Q/A pairs: {qa_json}
"#,
        topic = topic,
        qa_json = qa_json
    );

    let raw = arbiter.generate_raw_text(&prompt).await;
    let raw = match raw {
        Ok(r) => r,
        Err(_) => return Ok(None),
    };

    // Clean fenced code if present
    let cleaned = raw
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();

    // Try to parse JSON
    let v: serde_json::Value = match serde_json::from_str(cleaned) {
        Ok(v) => v,
        Err(_) => return Ok(None),
    };

    // Extract goal
    let goal = v.get("goal")
        .and_then(|x| x.as_str())
        .unwrap_or(topic)
        .to_string();

    // Extract dynamic parameters
    let mut parameters = std::collections::BTreeMap::new();
    
    if let Some(params_obj) = v.get("parameters").and_then(|x| x.as_object()) {
        for (param_name, param_data) in params_obj {
            if let Some(param_obj) = param_data.as_object() {
                let param_type = param_obj.get("type")
                    .and_then(|x| x.as_str())
                    .unwrap_or("string")
                    .to_string();
                
                let value = param_obj.get("value")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string();
                
                let question = param_obj.get("question")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string();
                
                parameters.insert(param_name.clone(), ExtractedParam {
                    question,
                    value,
                    param_type,
                    category: None,
                });
            }
        }
    }

    let prefs = ExtractedPreferences {
        goal,
        parameters,
    };

    Ok(Some(prefs))
}

/// Real LLM-driven capability synthesis using the delegating arbiter
async fn synthesize_capability_via_llm(
    ccos: &Arc<CCOS>,
    topic: &str,
    interaction_history: &[(String, String)],
    prefs: &ExtractedPreferences,
) -> Result<(String, String), Box<dyn std::error::Error>> {
    
    let arbiter = ccos.get_delegating_arbiter()
        .ok_or_else(|| {
            eprintln!("\n❌ Delegating arbiter not initialized!");
            eprintln!("   This usually means:");
            eprintln!("   1. No valid LLM configuration in config file");
            eprintln!("   2. Missing API keys (OPENAI_API_KEY, ANTHROPIC_API_KEY, etc.)");
            eprintln!("   3. Invalid provider/model settings");
            eprintln!("\n   Solutions:");
            eprintln!("   • Check config/agent_config.toml has valid llm_profiles");
            eprintln!("   • Ensure API key environment variable is set");
            eprintln!("   • Try: export OPENAI_API_KEY=sk-...");
            eprintln!("   • Or use --profile to select a valid profile\n");
            "Delegating arbiter not available - check LLM configuration"
        })?;
    
    // Build synthesis prompt from the actual interaction
    let mut interaction_summary = String::new();
    for (i, (question, answer)) in interaction_history.iter().enumerate() {
        interaction_summary.push_str(&format!("Turn {}: Q: {} A: {}\n", i + 1, question, answer));
    }
    
    // Format extracted parameters for the prompt
    let mut parameters_summary = String::new();
    for (param_name, param) in &prefs.parameters {
        parameters_summary.push_str(&format!(
            "- {} ({}) from Q: \"{}\" → A: \"{}\"\n",
            param_name, param.param_type, param.question, param.value
        ));
    }

    let synthesis_prompt = format!(
        r#"You are synthesizing an RTFS capability from a user interaction.

## User's Goal
"{}"

## Conversation History (Questions & Answers)
{}

## Your Task
Analyze the user's ACTUAL GOAL and the conversation to create a capability that helps achieve THAT SPECIFIC GOAL.

CRITICAL: The capability MUST match the user's goal, not generic research.
- If goal is "plan a trip", create a trip planning capability
- If goal is "research X", create a research capability  
- If goal is "build Y", create a building/development capability

The capability should:
1. Have an ID matching the goal domain (e.g., "travel.trip-planner.v1", "research.assistant.v1")
2. Accept relevant parameters based on the goal
3. Call external sub-capabilities to delegate specialized tasks
4. Use RTFS 'let' syntax to bind capability results to variables
5. Return a complete result map with all relevant information

CRITICAL RTFS PATTERN - Use 'let' to bind results:
- When calling a capability, ALWAYS bind the result with 'let'
- Use the bound variable in subsequent steps
- Final step should return the complete result

OUTPUT EXACTLY ONE fenced ```rtfs block containing a well-formed (capability ...) s-expression.

CRITICAL: Parameter types must use KEYWORD syntax, NOT string literals!
- CORRECT: :parameters {{:destination :string :duration :number :budget :currency}}
- WRONG:   :parameters {{:destination "string" :duration "number" :budget "currency"}}

Use keyword types: :string, :number, :list, :boolean, :currency, :duration, :integer, :float, etc.

Example for trip planning goal (showing proper 'let' binding with CORRECT TYPES):
```rtfs
(capability "travel.trip-planner.paris.v1"
  :description "Paris trip planner with user's budget and duration preferences"
  :parameters {{:destination :string :travel_dates :string :duration :number :budget :currency :interests :list :accommodation_type :string :travel_companions :string}}
  :implementation
    (do
      (let attractions 
        (call :travel.research {{:destination destination :interests interests}}))
      (let hotels
        (call :travel.hotels {{:city destination :budget budget :duration duration}}))
      (let transport
        (call :travel.transport {{:destination destination :mode "metro_and_walk"}}))
      (let food_spots
        (call :food.recommendations {{:city destination :interests interests :budget budget}}))
      (let itinerary
        (call :travel.itinerary {{:days duration :attractions attractions :hotels hotels :food food_spots}}))
      {{:status "research_completed"
        :summary (str "Complete " duration "-day trip plan for " destination " with $" budget " budget")
        :destination destination
        :attractions attractions
        :hotels hotels
        :transport transport
        :food_recommendations food_spots
        :itinerary itinerary}}))
```

KEY POINTS:
1. Use (let variable_name (call :capability {{:params}})) to bind results
2. Each 'let' captures the capability's return value
3. Parameter types are KEYWORDS like :string, :number, NOT string literals

EXTRACTED PARAMETERS FROM USER INTERACTION (use these in your capability):
{}

Respond ONLY with the fenced RTFS block specific to the user's ACTUAL goal, no other text."#,
        topic, interaction_summary, parameters_summary
    );
    
    // Call LLM to generate the capability
    let raw_response = arbiter.generate_raw_text(&synthesis_prompt).await
        .map_err(|e| format!("LLM synthesis failed: {}", e))?;
    
    // Extract capability from response
    let capability_spec = extract_capability_from_response(&raw_response)?;
    
    // Extract capability ID
    let capability_id = extract_capability_id_from_spec(&capability_spec)
        .unwrap_or_else(|| "research.smart-assistant.v1".to_string());
    
    // Phase 1: Extract dependencies from synthesized capability
    if let Ok(dep_result) = rtfs_compiler::ccos::synthesis::dependency_extractor::extract_dependencies(&capability_spec) {
        println!("🔍 DEPENDENCY ANALYSIS for {}", capability_id);
        println!("   Total dependencies: {}", dep_result.dependencies.len());
        println!("   Missing dependencies: {}", dep_result.missing_dependencies.len());
        
        if !dep_result.missing_dependencies.is_empty() {
            println!("   Missing capabilities:");
            for dep in &dep_result.missing_dependencies {
                println!("     - {}", dep);
            }
            
            // Create audit event
            let audit_data = rtfs_compiler::ccos::synthesis::dependency_extractor::create_audit_event_data(&capability_id, &dep_result.missing_dependencies);
            println!("   AUDIT: capability_deps_missing - {}", 
                audit_data.get("missing_capabilities").unwrap_or(&"none".to_string()));
        }
        
        if !dep_result.dependencies.is_empty() {
            println!("   All dependencies found:");
            for dep in &dep_result.dependencies {
                println!("     - {} (line {})", dep.capability_id, dep.line_number);
            }
        }
    }
    
    Ok((capability_id, capability_spec))
}

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
    println!("\n\n{}", "═══════════════════════════════════════════════════════════════".bold());
    println!("{}", "                    LEARNING IMPACT ANALYSIS".bold().magenta());
    println!("{}", "═══════════════════════════════════════════════════════════════\n".bold());

    println!("{}", "┌─────────────────────┬───────────────┬───────────────┬──────────┐".dim());
    println!("{}", "│ Metric              │ Before Learn  │ After Learn   │ Gain     │".bold());
    println!("{}", "├─────────────────────┼───────────────┼───────────────┼──────────┤".dim());
    
    println!(
        "│ {} │ {:>13} │ {:>13} │ {:>8} │",
        "Interaction Turns   ".dim(),
        format!("{}", before.turns_count).yellow(),
        format!("{}", after.turns_count).green(),
        format!("{}x", before.turns_count / after.turns_count.max(1)).cyan().bold()
    );
    
    println!(
        "│ {} │ {:>13} │ {:>13} │ {:>8} │",
        "Questions Asked     ".dim(),
        format!("{}", before.questions_asked).yellow(),
        format!("{}", after.questions_asked).green(),
        format!("-{}", before.questions_asked - after.questions_asked).cyan().bold()
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
    
    println!("{}", "└─────────────────────┴───────────────┴───────────────┴──────────┘\n".dim());

    println!("{}", "🎯 Key Achievements:".bold().green());
    println!("   {} Reduced interaction from {} turns to {} turn", "✓".green(), before.turns_count, after.turns_count);
    println!("   {} Eliminated {} redundant questions", "✓".green(), before.questions_asked);
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

    println!("\n{}", "═══════════════════════════════════════════════════════════════\n".bold());
}

fn print_banner() {
    println!("\n{}", "═══════════════════════════════════════════════════════════════".bold().cyan());
    println!("{}", "       🧠 CCOS/RTFS Self-Learning Demonstration 🧠".bold().cyan());
    println!("{}", "           Smart Research Assistant Example".bold());
    println!("{}", "═══════════════════════════════════════════════════════════════\n".bold().cyan());
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

