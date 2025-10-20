// GitHub Issue Triage Demo: two-turn conversation to collect repository and triage criteria,
// then synthesize an RTFS capability via delegating arbiter to auto-triage matching issues.
use clap::Parser;
use std::fs;
use std::path::Path;
use std::sync::Arc;

use serde_json;
use toml;

use rtfs_compiler::ast::MapKey;
use rtfs_compiler::ccos::intent_graph::config::IntentGraphConfig;
use rtfs_compiler::ccos::synthesis::{self, synthesize_capabilities, InteractionTurn};
use rtfs_compiler::ccos::types::{Plan, StorableIntent};
use rtfs_compiler::ccos::CCOS;
use rtfs_compiler::config::profile_selection::expand_profiles;
use rtfs_compiler::config::types::{AgentConfig, LlmProfile};
use rtfs_compiler::parser;
use rtfs_compiler::runtime::{RuntimeContext, RuntimeResult, Value};

#[derive(Parser, Debug)]
/// Minimal demo with optional config loading (`--config <path>`)
struct Args {
    /// Path to AgentConfig (TOML or JSON)
    #[arg(long)]
    config: Option<String>,

    /// Optional LLM profile name to select from config
    #[arg(long)]
    profile: Option<String>,
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

fn apply_profile_env(p: &LlmProfile) {
    // Provider + model mapping
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

    // Route API keys to provider-specific envs
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
            "stub" => { /* no key needed */ }
            _ => std::env::set_var("OPENAI_API_KEY", key),
        }
    }

    // Normalize provider id for runtime
    match p.provider.as_str() {
        "openai" => std::env::set_var("CCOS_LLM_PROVIDER", "openai"),
        "claude" | "anthropic" => std::env::set_var("CCOS_LLM_PROVIDER", "anthropic"),
        "openrouter" => std::env::set_var("CCOS_LLM_PROVIDER", "openrouter"),
        "local" => std::env::set_var("CCOS_LLM_PROVIDER", "local"),
        "stub" => std::env::set_var("CCOS_LLM_PROVIDER", "stub"),
        _ => {}
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🤖 GitHub Issue Triage Demo - CCOS Two-Turn Synthesis");
    println!(
        "Collects repository and triage criteria, then synthesizes an auto-triage capability.\n"
    );
    println!(
        "💡 Tip: Set CCOS_INTERACTIVE_ASK=1 for live prompts or run in batch mode with defaults.\n"
    );

    let args = Args::parse();

    let mut agent_config: Option<AgentConfig> = None;
    if let Some(cfg_path) = &args.config {
        match load_agent_config(cfg_path) {
            Ok(cfg) => {
                // Resolve profile: CLI > config default; support model_set expansion
                if let Some(llm_cfg) = &cfg.llm_profiles {
                    let (profiles, _meta, _why) = expand_profiles(&cfg);
                    let chosen_name = args
                        .profile
                        .as_ref()
                        .cloned()
                        .or_else(|| llm_cfg.default.clone());
                    if let Some(name) = chosen_name {
                        if let Some(p) = profiles.iter().find(|pp| pp.name == name) {
                            println!(
                                "Using LLM profile: {} (provider={}, model={})",
                                p.name, p.provider, p.model
                            );
                            apply_profile_env(p);
                        }
                    }
                }
                agent_config = Some(cfg);
            }
            Err(e) => eprintln!("failed to load agent config {}: {}", cfg_path, e),
        }
    }

    let ccos = if let Some(cfg) = agent_config.clone() {
        Arc::new(
            CCOS::new_with_agent_config_and_configs_and_debug_callback(
                IntentGraphConfig::default(),
                None,
                Some(cfg),
                None,
            )
            .await?,
        )
    } else {
        Arc::new(CCOS::new_with_debug_callback(None).await?)
    };

    println!(
        "delegating arbiter available = {}",
        ccos.get_delegating_arbiter().is_some()
    );

    // Execute a simple two-turn plan to collect conversation
    let runtime_context = RuntimeContext::controlled(vec!["ccos.user.ask".to_string()]);

    let plan_body = build_two_turn_plan(FIRST_PROMPT, SECOND_PROMPT);
    println!("\n--- Hardcoded Plan ---\n{}\n", plan_body);

    let intent_id = "intent.two_turn.demo".to_string();
    let mut storable_intent =
        StorableIntent::new("Collect calendar details via two prompts".to_string());
    storable_intent.intent_id = intent_id.clone();
    storable_intent.name = Some("two-turn-demo".to_string());

    if let Ok(mut graph) = ccos.get_intent_graph().lock() {
        graph.store_intent(storable_intent)?;
    } else {
        return Err("Failed to lock intent graph".into());
    }

    let plan = Plan::new_rtfs(plan_body.clone(), vec![intent_id]);

    let execution = ccos
        .validate_and_execute_plan(plan, &runtime_context)
        .await?;

    println!("Execution value: {}", execution.value);

    let conversation = extract_conversation(&execution.value);
    if conversation.is_empty() {
        println!("No conversation captured; confirm CCOS_INTERACTIVE_ASK=1 when running.");
        return Ok(());
    }

    println!("\nConversation transcript:");
    for (idx, turn) in conversation.iter().enumerate() {
        println!("- Turn {} prompt: {}", idx + 1, turn.prompt);
        println!("  Answer: {}", turn.answer);
    }

    // Derive a concise goal from the answers
    let goal_text = derive_goal_from_answers(&conversation);
    if !goal_text.is_empty() {
        println!("\nGoal: {}", goal_text);
    }

    let synthesis_input: Vec<InteractionTurn> = conversation
        .iter()
        .enumerate()
        .map(|(idx, record)| InteractionTurn {
            turn_index: idx,
            prompt: record.prompt.clone(),
            answer: Some(record.answer.clone()),
        })
        .collect();

    let synthesis = synthesize_capabilities(&synthesis_input);
    println!(
        "\nSynthesis metrics: turns={} coverage={:.2}",
        synthesis.metrics.turns_total, synthesis.metrics.coverage
    );
    if !synthesis.metrics.missing_required.is_empty() {
        println!(
            "Missing answers for: {:?}",
            synthesis.metrics.missing_required
        );
    }

    if let Some(collector_src) = synthesis.collector {
        println!("\n--- Synthesized Collector ---\n{}", collector_src);
        println!("Registration skipped in demo: synthesized capability not registered.");
    }

    if let Some(planner_src) = synthesis.planner {
        println!("\n--- Synthesized Planner ---\n{}", planner_src);
    }

    if !synthesis.pending_capabilities.is_empty() {
        println!("\n--- Pending Capabilities (needs resolution) ---\n{:?}", synthesis.pending_capabilities);
    }

    if let Some(arbiter) = ccos.get_delegating_arbiter() {
        let schema = synthesis::schema_builder::extract_param_schema(&synthesis_input);
        let domain = "github.issue.triage";
        match arbiter
            .synthesize_capability_from_collector(&schema, &synthesis_input, domain)
            .await
        {
            Ok(rtfs_block) => {
                // Validate: prefer single top-level capability
                match validate_single_capability(&rtfs_block) {
                    Ok(valid_cap) => {
                        println!(
                            "\n--- Synthesized Capability (validated) ---\n{}\n",
                            valid_cap
                        );
                        if let Some(cap_id) = extract_capability_id(&valid_cap) {
                            if let Err(e) = persist_capability(&cap_id, &valid_cap) {
                                println!("Warning: failed to persist capability: {}", e);
                            } else {
                                println!(
                                    "Persisted synthesized capability to capabilities/generated/{}.rtfs",
                                    cap_id
                                );
                            }
                        }
                    }
                    Err(e) => {
                        println!("\nValidation failed for arbiter output: {}", e);
                        println!("--- Raw Arbiter Output ---\n{}\n", rtfs_block);
                    }
                }
            }
            Err(err) => println!("\nDelegating arbiter synthesis failed: {}", err),
        }
    } else {
        println!("\nDelegating arbiter not available in this configuration.");
    }

    Ok(())
}

const FIRST_PROMPT: &str = "What GitHub repository should I monitor for issues?";
const SECOND_PROMPT: &str =
    "What criteria should trigger automatic triage? (e.g., label:critical, no assignee)";

#[derive(Debug, Clone)]
struct ConversationRecord {
    prompt: String,
    answer: String,
}

fn derive_goal_from_answers(convo: &[ConversationRecord]) -> String {
    let repo = convo.get(0).map(|c| c.answer.trim()).unwrap_or("");
    let criteria = convo.get(1).map(|c| c.answer.trim()).unwrap_or("");
    if repo.is_empty() && criteria.is_empty() {
        String::new()
    } else if criteria.is_empty() {
        format!("Monitor and triage issues in repository: {}", repo)
    } else if repo.is_empty() {
        format!("Triage issues matching criteria: {}", criteria)
    } else {
        format!("Auto-triage issues in {} matching: {}", repo, criteria)
    }
}

fn build_two_turn_plan(first_prompt: &str, second_prompt: &str) -> String {
    let first = escape_quotes(first_prompt);
    let second = escape_quotes(second_prompt);
    format!(
        "(do\n  (let [first (call :ccos.user.ask \"{first}\")\n        second (call :ccos.user.ask \"{second}\")]\n    {{:status \"completed\"\n     :conversation [{{:prompt \"{first}\" :answer first}}\n                    {{:prompt \"{second}\" :answer second}}]}}))"
    )
}

fn validate_single_capability(rtfs_source: &str) -> Result<String, String> {
    // Fast-path: enforce starts with (capability
    let trimmed = rtfs_source.trim_start();
    if !trimmed.starts_with("(capability ") {
        // If the model wrapped in a code fence or do-block, attempt to find first capability
        if let Some(idx) = trimmed.find("(capability ") {
            let sub = &trimmed[idx..];
            return Ok(extract_balanced_form(sub).unwrap_or_else(|| sub.to_string()));
        }
        return Err("Output does not start with (capability".to_string());
    }

    // Try parser; if it parses and contains exactly one top-level capability, return original
    match parser::parse(rtfs_source) {
        Ok(items) => {
            let cap_count = items
                .iter()
                .filter(|tl| matches!(tl, rtfs_compiler::ast::TopLevel::Capability(_)))
                .count();
            if cap_count == 1 {
                Ok(trimmed.to_string())
            } else {
                // Fallback: extract the first balanced capability s-expr
                Ok(extract_balanced_form(trimmed).unwrap_or_else(|| trimmed.to_string()))
            }
        }
        Err(_) => Ok(extract_balanced_form(trimmed).unwrap_or_else(|| trimmed.to_string())),
    }
}

fn extract_balanced_form(text: &str) -> Option<String> {
    let mut depth = 0i32;
    let mut started = false;
    let bytes = text.as_bytes();
    let mut end_idx = 0usize;
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'(' {
            depth += 1;
            started = true;
        } else if b == b')' {
            depth -= 1;
        }
        if started && depth == 0 {
            end_idx = i + 1;
            break;
        }
    }
    if started && end_idx > 0 {
        Some(text[..end_idx].to_string())
    } else {
        None
    }
}

fn extract_capability_id(rtfs_source: &str) -> Option<String> {
    for line in rtfs_source.lines() {
        let t = line.trim();
        if t.starts_with("(capability ") {
            // Expect: (capability "name" ...)
            let after = t.trim_start_matches("(capability ").trim_start();
            if let Some(idx) = after.find('"') {
                let rest = &after[idx + 1..];
                if let Some(endq) = rest.find('"') {
                    return Some(rest[..endq].to_string());
                }
            }
        }
    }
    None
}

fn persist_capability(cap_id: &str, rtfs_source: &str) -> std::io::Result<()> {
    let dir = Path::new("rtfs_compiler/capabilities/generated");
    std::fs::create_dir_all(dir)?;
    let file_path = dir.join(format!("{}.rtfs", cap_id));
    std::fs::write(file_path, rtfs_source.as_bytes())
}

fn escape_quotes(text: &str) -> String {
    text.replace('"', "\\\"")
}

fn extract_conversation(value: &Value) -> Vec<ConversationRecord> {
    match value {
        Value::Map(map) => conversation_entries(map),
        _ => Vec::new(),
    }
}

fn conversation_entries(map: &std::collections::HashMap<MapKey, Value>) -> Vec<ConversationRecord> {
    let mut records = Vec::new();
    if let Some(value) = map_get_keyword(map, "conversation") {
        match value {
            Value::Vector(items) | Value::List(items) => {
                for item in items {
                    if let Value::Map(entry_map) = item {
                        if let (Some(prompt), Some(answer)) = (
                            map_get_keyword(entry_map, "prompt"),
                            map_get_keyword(entry_map, "answer"),
                        ) {
                            if let (Value::String(p), Value::String(a)) = (prompt, answer) {
                                records.push(ConversationRecord {
                                    prompt: p.clone(),
                                    answer: a.clone(),
                                });
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
    records
}

fn map_get_keyword<'a>(
    map: &'a std::collections::HashMap<MapKey, Value>,
    key: &str,
) -> Option<&'a Value> {
    map.iter().find_map(|(k, v)| match k {
        MapKey::Keyword(keyword) if keyword.0 == key => Some(v),
        MapKey::String(s) if s == key => Some(v),
        _ => None,
    })
}

#[allow(dead_code)]
async fn register_placeholder_capability(
    ccos: &Arc<CCOS>,
    capability_id: &str,
) -> RuntimeResult<()> {
    println!(
        "Registering synthesized capability `{}` with placeholder handler",
        capability_id
    );

    let marketplace = ccos.get_capability_marketplace();
    let capability_id_owned = capability_id.to_string();
    let handler_id = capability_id_owned.clone();
    let handler = Arc::new(move |input: &Value| -> RuntimeResult<Value> {
        println!(
            "[synth] capability `{}` invoked with input: {}",
            handler_id, input
        );
        Ok(Value::String("synth placeholder response".into()))
    });

    marketplace
        .register_local_capability(
            capability_id_owned.clone(),
            format!("Synthesized capability {}", capability_id_owned),
            "Placeholder handler generated during demo".to_string(),
            handler,
        )
        .await?;

    println!(
        "Capability `{}` registered successfully.",
        capability_id_owned
    );
    Ok(())
}
