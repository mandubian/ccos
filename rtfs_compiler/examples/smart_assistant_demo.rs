// Governed smart assistant demo rebuilt for arbitrary natural-language goals.
//
// The previous implementation is retained below (commented out) while the new
// adaptive assistant flows are authored.

use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::sync::Arc;

use chrono::Utc;
use clap::Parser;
use crossterm::style::Stylize;
use rtfs_compiler::ast::{Expression, Keyword, Literal, MapKey};
use rtfs_compiler::ccos::arbiter::delegating_arbiter::DelegatingArbiter;
use rtfs_compiler::ccos::intent_graph::config::IntentGraphConfig;
use rtfs_compiler::ccos::types::{Intent, Plan};
use rtfs_compiler::ccos::CCOS;
use rtfs_compiler::config::profile_selection::expand_profiles;
use rtfs_compiler::config::types::{AgentConfig, LlmProfile};
use rtfs_compiler::parser::parse_expression;
use rtfs_compiler::runtime::error::{RuntimeError, RuntimeResult};
use rtfs_compiler::runtime::values::Value;
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

    let answers =
        conduct_interview(&ccos, &questions, &mut seeded_answers, args.debug_prompts).await?;

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
        Ok(steps) if !steps.is_empty() => steps,
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

    let needs_value = build_needs_capabilities(&plan_steps);
    
    // Resolve missing capabilities and build orchestrating agent
    let resolved_steps = resolve_and_stub_capabilities(&ccos, &plan_steps, &matches).await?;
    let orchestrator_rtfs = generate_orchestrator_capability(&goal, &resolved_steps)?;
    
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

    print_plan_draft(&plan_steps, &matches, &plan);
    println!(
        "\n{}",
        "✅ Orchestrator generated and ready for execution".bold().green()
    );

    Ok(())
}

type DemoResult<T> = Result<T, Box<dyn Error>>;

fn runtime_error(err: RuntimeError) -> Box<dyn Error> {
    Box::new(err)
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
    let normalized = strip_commas_outside_strings(&sanitized);
    match parse_expression(&normalized) {
        Ok(expr) => Ok(expression_to_value(&expr)),
        Err(rtfs_err) => match serde_json::from_str::<serde_json::Value>(&normalized) {
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

async fn conduct_interview(
    _ccos: &Arc<CCOS>,
    questions: &[ClarifyingQuestion],
    seeded_answers: &mut HashMap<String, AnswerRecord>,
    _debug: bool,
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

    let response = delegating
        .generate_raw_text(&prompt)
        .await
        .map_err(runtime_error)?;
    if debug {
        println!(
            "{}\n{}\n{}",
            "┌─ Proposed steps response ──────────────────".dim(),
            response,
            "└────────────────────────────────────────────".dim()
        );
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
    let normalized = strip_commas_outside_strings(&sanitized);
    match parse_expression(&normalized) {
        Ok(expr) => Ok(expression_to_value(&expr)),
        Err(rtfs_err) => match serde_json::from_str::<serde_json::Value>(&normalized) {
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
}

/// Resolve missing capabilities by searching marketplace or creating stubs.
async fn resolve_and_stub_capabilities(
    ccos: &Arc<CCOS>,
    steps: &[ProposedStep],
    matches: &[CapabilityMatch],
) -> DemoResult<Vec<ResolvedStep>> {
    let mut resolved = Vec::with_capacity(steps.len());

    for step in steps {
        if let Some(match_record) = matches.iter().find(|m| m.step_id == step.id) {
            if let Some(cap_id) = &match_record.matched_capability {
                resolved.push(ResolvedStep {
                    original: step.clone(),
                    capability_id: cap_id.clone(),
                    resolution_strategy: ResolutionStrategy::Found,
                });
                continue;
            }
        }

        // Not found; create a stub capability for later resolution
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

/// Generate an RTFS orchestrator capability that chains all resolved steps.
fn generate_orchestrator_capability(
    goal: &str,
    resolved_steps: &[ResolvedStep],
) -> DemoResult<String> {
    let mut rtfs_code = String::new();
    rtfs_code.push_str("(do\n");
    rtfs_code.push_str(&format!(
        "  ;; Orchestrator generated for goal: {}\n",
        goal.replace("\"", "\\\"")
    ));
    rtfs_code.push_str("  ;; This capability chains multiple steps and manages data flow.\n");

    if resolved_steps.is_empty() {
        rtfs_code.push_str("  nil ;; No steps to execute\n");
    } else {
        rtfs_code.push_str("  (let [\n");
        
        // Build sequential let bindings for each step
        for (idx, resolved) in resolved_steps.iter().enumerate() {
            let step_var = format!("step_{}", idx);
            rtfs_code.push_str(&format!(
                "    {} (({} {}))\n",
                step_var,
                resolved.capability_id,
                build_step_call_args(&resolved.original, resolved_steps, idx)?
            ));
        }
        
        rtfs_code.push_str("    ]\n");
        rtfs_code.push_str(&format!("    ;; Aggregate and return all step results\n"));
        rtfs_code.push_str(&build_final_output(resolved_steps)?);
        rtfs_code.push_str("\n  )\n");
    }
    
    rtfs_code.push_str(")\n");
    Ok(rtfs_code)
}

fn build_step_call_args(
    step: &ProposedStep,
    _resolved_steps: &[ResolvedStep],
    _idx: usize,
) -> DemoResult<String> {
    // For now, build simple call with just the required inputs as keyword args
    let args: Vec<String> = step
        .required_inputs
        .iter()
        .map(|input| format!(":${}", input))
        .collect();

    Ok(args.join(" "))
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

        matches.push(CapabilityMatch {
            step_id: step.id.clone(),
            matched_capability: None,
            status: MatchStatus::Missing,
            note: Some("No matching capability registered".to_string()),
        });
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
    if let rtfs_compiler::ccos::types::PlanBody::Rtfs(code) = &plan.body {
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
