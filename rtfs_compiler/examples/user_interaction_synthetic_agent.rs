// Generic Synthetic Agent Builder: Two-turn conversation → capability discovery → executor synthesis
// This demonstrates the full CCOS synthetic agent flow:
// 1. Collect user intent via conversation
// 2. LLM discovers required capabilities from marketplace
// 3. LLM synthesizes executor that orchestrates discovered capabilities
// 4. Register and invoke the synthesized agent

use clap::Parser;
use std::sync::Arc;
use std::fs;
use std::path::Path;
use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json;
use toml;

use rtfs_compiler::ccos::CCOS;
use rtfs_compiler::ccos::intent_graph::config::IntentGraphConfig;
use rtfs_compiler::ccos::types::{Plan, StorableIntent};
use rtfs_compiler::runtime::{RuntimeContext, Value};
use rtfs_compiler::ast::MapKey;
use rtfs_compiler::config::profile_selection::expand_profiles;
use rtfs_compiler::config::types::{AgentConfig, LlmProfile};
use rtfs_compiler::parser;

#[derive(Parser, Debug)]
struct Args {
    /// Path to AgentConfig (TOML or JSON)
    #[arg(long)]
    config: Option<String>,

    /// Optional LLM profile name to select from config
    #[arg(long)]
    profile: Option<String>,

    /// First question to ask user
    #[arg(long, default_value = "What data source should I analyze?")]
    q1: String,

    /// Second question to ask user
    #[arg(long, default_value = "What analysis or action should I perform?")]
    q2: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct CapabilitySearchQuery {
    category: String,
    keywords: Vec<String>,
}

#[derive(Debug, Clone)]
struct ConversationRecord {
    prompt: String,
    answer: String,
}

#[derive(Debug, Clone)]
struct DiscoveredCapability {
    id: String,
    description: String,
    signature: String,
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
            "stub" => { /* no key needed */ }
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

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🤖 CCOS Synthetic Agent Builder");
    println!("═══════════════════════════════════════════════════════════════");
    println!("Collects intent → Discovers capabilities → Synthesizes executor\n");

    let args = Args::parse();

    // Load and apply config
    let mut agent_config: Option<AgentConfig> = None;
    if let Some(cfg_path) = &args.config {
        match load_agent_config(cfg_path) {
            Ok(cfg) => {
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
                                "📋 Using LLM profile: {} (provider={}, model={})\n",
                                p.name, p.provider, p.model
                            );
                            apply_profile_env(p);
                        }
                    }
                }
                agent_config = Some(cfg);
            }
            Err(e) => eprintln!("⚠️  Failed to load agent config {}: {}", cfg_path, e),
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

    println!("✓ CCOS initialized");
    println!(
        "✓ Delegating arbiter available: {}",
        ccos.get_delegating_arbiter().is_some()
    );

    // ═══════════════════════════════════════════════════════════════
    // PHASE 1: COLLECT USER INTENT
    // ═══════════════════════════════════════════════════════════════
    println!("\n┌─────────────────────────────────────────────────────────────┐");
    println!("│ PHASE 1: Collect User Intent                               │");
    println!("└─────────────────────────────────────────────────────────────┘\n");

    let runtime_context = RuntimeContext::controlled(vec!["ccos.user.ask".to_string()]);
    let plan_body = build_two_turn_plan(&args.q1, &args.q2);

    // Phase 0: User's original intent (in real flow, this comes from natural language)
    let original_goal = std::env::var("CCOS_INTENT_GOAL")
        .unwrap_or_else(|_| "Automate a workflow based on collected criteria".to_string());
    
    let intent_id = "intent.synthetic.agent".to_string();
    let mut storable_intent = StorableIntent::new(original_goal.clone());
    storable_intent.intent_id = intent_id.clone();
    storable_intent.name = Some("synthetic-agent-collection".to_string());
    storable_intent.goal = original_goal.clone();

    if let Ok(mut graph) = ccos.get_intent_graph().lock() {
        graph.store_intent(storable_intent)?;
    } else {
        return Err("Failed to lock intent graph".into());
    }

    let plan = Plan::new_rtfs(plan_body.clone(), vec![intent_id]);
    let execution = ccos
        .validate_and_execute_plan(plan, &runtime_context)
        .await?;

    let conversation = extract_conversation(&execution.value);
    if conversation.is_empty() {
        println!("❌ No conversation captured. Set CCOS_INTERACTIVE_ASK=1 for live prompts.");
        return Ok(());
    }

    println!("📝 Conversation transcript:");
    for (idx, turn) in conversation.iter().enumerate() {
        println!("   Q{}: {}", idx + 1, turn.prompt);
        println!("   A{}: {}\n", idx + 1, turn.answer);
    }

    let parameters = extract_parameters(&conversation);

    println!("🎯 Original Intent: {}", original_goal);
    println!("📊 Collected Parameters:");
    for (key, value) in &parameters {
        println!("   :{} = \"{}\"", key, value);
    }

    // ═══════════════════════════════════════════════════════════════
    // PHASE 2: DISCOVER CAPABILITIES
    // ═══════════════════════════════════════════════════════════════
    println!("\n┌─────────────────────────────────────────────────────────────┐");
    println!("│ PHASE 2: Discover Required Capabilities                    │");
    println!("└─────────────────────────────────────────────────────────────┘\n");

    let discovered = if let Some(arbiter) = ccos.get_delegating_arbiter() {
        discover_capabilities(
            &arbiter,
            &original_goal,
            &parameters,
        )
        .await?
    } else {
        println!("⚠️  Arbiter not available, using mock capabilities");
        get_mock_capabilities()
    };

    println!("✓ Found {} relevant capabilities:", discovered.len());
    for cap in &discovered {
        println!("   • {} - {}", cap.id, cap.description);
        println!("     Signature: {}", cap.signature);
    }

    // ═══════════════════════════════════════════════════════════════
    // PHASE 3: SYNTHESIZE EXECUTOR
    // ═══════════════════════════════════════════════════════════════
    println!("\n┌─────────────────────────────────────────────────────────────┐");
    println!("│ PHASE 3: Synthesize Executor Capability                    │");
    println!("└─────────────────────────────────────────────────────────────┘\n");

    let executor_rtfs = if let Some(arbiter) = ccos.get_delegating_arbiter() {
        synthesize_executor(
            &arbiter,
            &original_goal,
            &parameters,
            &discovered,
        )
        .await?
    } else {
        return Err("Arbiter required for synthesis".into());
    };

    println!("✓ Synthesized executor capability:\n");
    match validate_single_capability(&executor_rtfs) {
        Ok(valid_cap) => {
            println!("```rtfs");
            println!("{}", valid_cap);
            println!("```\n");

            if let Some(cap_id) = extract_capability_id(&valid_cap) {
                persist_capability(&cap_id, &valid_cap)?;
                println!("✓ Persisted to capabilities/generated/{}.rtfs\n", cap_id);
            }

            // ═══════════════════════════════════════════════════════════════
            // PHASE 4: REGISTER & INVOKE (optional demo)
            // ═══════════════════════════════════════════════════════════════
            println!("┌─────────────────────────────────────────────────────────────┐");
            println!("│ PHASE 4: Registration & Invocation (Demo)                  │");
            println!("└─────────────────────────────────────────────────────────────┘\n");

            println!("✓ Synthesized capability ready for registration");
            println!("✓ Would invoke with parameters: {:?}", parameters);
            println!("\n🎉 Synthetic agent builder completed successfully!");
        }
        Err(e) => {
            println!("❌ Validation failed: {}", e);
            println!("\n--- Raw Arbiter Output ---\n{}\n", executor_rtfs);
        }
    }

    Ok(())
}

// ═══════════════════════════════════════════════════════════════════
// CAPABILITY DISCOVERY (Stage 2a)
// ═══════════════════════════════════════════════════════════════════

async fn discover_capabilities(
    arbiter: &rtfs_compiler::ccos::arbiter::delegating_arbiter::DelegatingArbiter,
    goal: &str,
    parameters: &HashMap<String, String>,
) -> Result<Vec<DiscoveredCapability>, Box<dyn std::error::Error>> {
    let discovery_prompt = build_discovery_prompt(goal, parameters);

    let show_prompts = std::env::var("RTFS_SHOW_PROMPTS")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);

    if show_prompts {
        println!("--- Capability Discovery Prompt ---\n{}\n--- END PROMPT ---\n", discovery_prompt);
    }

    let response = arbiter.generate_raw_text(&discovery_prompt).await
        .map_err(|e| format!("Discovery LLM call failed: {}", e))?;

    if show_prompts {
        println!("--- Discovery Response ---\n{}\n--- END RESPONSE ---\n", response);
    }

    // Parse JSON array of search queries
    let queries: Vec<CapabilitySearchQuery> = parse_json_response(&response)?;

    println!("🔍 LLM generated {} search queries:", queries.len());
    for q in &queries {
        println!("   • Category: {}, Keywords: {:?}", q.category, q.keywords);
    }

    // For now, return mock capabilities based on queries
    // In full implementation, search marketplace here
    Ok(mock_search_capabilities(&queries))
}

fn build_discovery_prompt(goal: &str, parameters: &HashMap<String, String>) -> String {
    let params_list: Vec<String> = parameters
        .iter()
        .map(|(k, v)| format!("- {} = \"{}\"", k, v))
        .collect();

    format!(
        concat!(
            "You are analyzing a user goal to discover required capabilities.\n\n",
            "## Collected Context (from conversation)\n{}\n\n",
            "## Original User Intent\n{}\n\n",
            "## Your Task\n",
            "Generate 2-5 capability search queries to find tools needed to achieve this goal.\n\n",
            "## Available Categories\n",
            "- github, gitlab, jira (issue tracking)\n",
            "- slack, email, sms (notifications)\n",
            "- database, storage (persistence)\n",
            "- ccos (core system capabilities)\n\n",
            "## Output Format\n",
            "Return ONLY a JSON array, no markdown fences, no explanation:\n",
            "[\n",
            "  {{\"category\": \"github\", \"keywords\": [\"search\", \"issues\"]}},\n",
            "  {{\"category\": \"github\", \"keywords\": [\"label\", \"add\"]}}\n",
            "]\n"
        ),
        params_list.join("\n"),
        goal
    )
}

fn parse_json_response(response: &str) -> Result<Vec<CapabilitySearchQuery>, Box<dyn std::error::Error>> {
    // Strip markdown fences if present
    let cleaned = response
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();

    serde_json::from_str(cleaned).map_err(|e| format!("JSON parse failed: {}", e).into())
}

fn mock_search_capabilities(queries: &[CapabilitySearchQuery]) -> Vec<DiscoveredCapability> {
    let mut results = Vec::new();

    for query in queries {
        match query.category.as_str() {
            "github" => {
                if query.keywords.iter().any(|k| k.contains("search") || k.contains("issues")) {
                    results.push(DiscoveredCapability {
                        id: "github.search.issues".to_string(),
                        description: "Search GitHub issues matching query".to_string(),
                        signature: "(call :github.search.issues {:repo string :query string}) → vector".to_string(),
                    });
                }
                if query.keywords.iter().any(|k| k.contains("label") || k.contains("add")) {
                    results.push(DiscoveredCapability {
                        id: "github.issues.add_label".to_string(),
                        description: "Add label to GitHub issues".to_string(),
                        signature: "(call :github.issues.add_label {:repo string :issues vector :label string}) → map".to_string(),
                    });
                }
            }
            "ccos" | _ => {
                results.push(DiscoveredCapability {
                    id: "ccos.echo".to_string(),
                    description: "Print message to output".to_string(),
                    signature: "(call :ccos.echo {:message string}) → nil".to_string(),
                });
            }
        }
    }

    // Deduplicate by id
    let mut seen = std::collections::HashSet::new();
    results.retain(|cap| seen.insert(cap.id.clone()));

    results
}

fn get_mock_capabilities() -> Vec<DiscoveredCapability> {
    vec![
        DiscoveredCapability {
            id: "ccos.echo".to_string(),
            description: "Print message".to_string(),
            signature: "(call :ccos.echo {:message string})".to_string(),
        }
    ]
}

// ═══════════════════════════════════════════════════════════════════
// EXECUTOR SYNTHESIS (Stage 2b)
// ═══════════════════════════════════════════════════════════════════

async fn synthesize_executor(
    arbiter: &rtfs_compiler::ccos::arbiter::delegating_arbiter::DelegatingArbiter,
    goal: &str,
    parameters: &HashMap<String, String>,
    capabilities: &[DiscoveredCapability],
) -> Result<String, Box<dyn std::error::Error>> {
    let synthesis_prompt = build_synthesis_prompt(goal, parameters, capabilities);

    let show_prompts = std::env::var("RTFS_SHOW_PROMPTS")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);

    if show_prompts {
        println!("--- Executor Synthesis Prompt ---\n{}\n--- END PROMPT ---\n", synthesis_prompt);
    }

    let response = arbiter.generate_raw_text(&synthesis_prompt).await
        .map_err(|e| format!("Synthesis LLM call failed: {}", e))?;

    if show_prompts {
        println!("--- Synthesis Response ---\n{}\n--- END RESPONSE ---\n", response);
    }

    Ok(response)
}

fn build_synthesis_prompt(
    goal: &str,
    parameters: &HashMap<String, String>,
    capabilities: &[DiscoveredCapability],
) -> String {
    let params_section = build_params_section(parameters);
    let caps_section = build_capabilities_section(capabilities);
    let grammar = load_grammar_snippet();

    format!(
        concat!(
            "You are synthesizing an RTFS capability to execute a user intent.\n\n",
            "## Collected Parameters (DO NOT re-ask these)\n{}\n\n",
            "## Original User Intent\n{}\n\n",
            "## Discovered Capabilities (from marketplace)\n{}\n\n",
            "## RTFS Grammar Reference\n{}\n\n",
            "## STRICT FORBIDDEN CONSTRUCTS\n",
            "❌ NO Clojure functions: clojure.string/*, first, second, count, etc.\n",
            "❌ NO step-result (steps are isolated, use parameters)\n",
            "❌ NO defn, fn, def\n",
            "❌ NO #(...) lambdas\n",
            "✅ ONLY: call, let, step, if, match, str, =, get\n\n",
            "## Your Task\n",
            "Synthesize ONE capability that:\n",
            "1. ACCEPTS collected parameters as [:param1 :param2 ...]\n",
            "2. USES parameters directly (they are already bound)\n",
            "3. CALLS discovered capabilities to achieve goal\n",
            "4. RETURNS structured map with :status and results\n\n",
            "## Parameter Access\n",
            "Parameters are ALREADY BOUND. Reference them directly:\n",
            "- NOT: (let [x :param1] ...) ❌\n",
            "- YES: Use param1 directly in expressions ✅\n\n",
            "## Generic Example Template\n",
            "(capability \"domain.executor.v1\"\n",
            "  :description \"Execute task using collected parameters\"\n",
            "  :parameters [:param1 :param2]\n",
            "  :needs_capabilities [:discovered.tool.one :discovered.tool.two :ccos.echo]\n",
            "  :implementation\n",
            "    (do\n",
            "      (step \"Process Input\"\n",
            "        (let ((result (call :discovered.tool.one {{:input param1}})))\n",
            "          (call :ccos.echo {{:message (str \"Processing \" param1)}})))\n",
            "      (step \"Execute Action\"\n",
            "        (call :discovered.tool.two {{:data param2}}))\n",
            "      (step \"Return Results\"\n",
            "        {{:status \"completed\" :param1 param1 :param2 param2}})))\n\n",
            "Start response with `(capability` on first line. NO prose, NO markdown fences.\n"
        ),
        params_section,
        goal,
        caps_section,
        grammar
    )
}

fn build_params_section(parameters: &HashMap<String, String>) -> String {
    if parameters.is_empty() {
        "- (no parameters collected)".to_string()
    } else {
        parameters
            .iter()
            .map(|(k, v)| format!("- :{} = \"{}\"", k, v))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

fn build_capabilities_section(capabilities: &[DiscoveredCapability]) -> String {
    if capabilities.is_empty() {
        "- (no capabilities discovered)".to_string()
    } else {
        capabilities
            .iter()
            .map(|cap| format!("- :{} - {}\n  Signature: {}", cap.id, cap.description, cap.signature))
            .collect::<Vec<_>>()
            .join("\n\n")
    }
}

fn load_grammar_snippet() -> String {
    std::fs::read_to_string("assets/prompts/arbiter/plan_generation/v1/grammar.md")
        .or_else(|_| std::fs::read_to_string("../assets/prompts/arbiter/plan_generation/v1/grammar.md"))
        .unwrap_or_else(|_| {
            concat!(
                "## RTFS Special Forms\n",
                "- (call :capability args...) - invoke capability\n",
                "- (let [var expr ...] body) - local bindings\n",
                "- (step \"Name\" expr) - named execution step\n",
                "- (do expr...) - sequential execution\n",
                "- (if cond then else) - conditional\n"
            ).to_string()
        })
}

// ═══════════════════════════════════════════════════════════════════
// HELPER FUNCTIONS
// ═══════════════════════════════════════════════════════════════════

// Removed derive_goal_from_answers - we use the original Intent.goal instead

fn extract_parameters(convo: &[ConversationRecord]) -> HashMap<String, String> {
    let mut params = HashMap::new();
    for record in convo.iter() {
        let key = derive_param_name(&record.prompt);
        params.insert(key, record.answer.clone());
    }
    params
}

/// Derive a meaningful parameter name from the question
/// Examples:
/// - "What GitHub repository?" → "github/repository"
/// - "What data source should I analyze?" → "data/source"
/// - "What event should trigger notification?" → "event/trigger"
/// - "What criteria?" → "criteria"
fn derive_param_name(question: &str) -> String {
    // Remove question marks and common question starters
    let cleaned = question
        .trim_end_matches('?')
        .trim()
        .to_lowercase()
        .replace("what ", "")
        .replace("which ", "")
        .replace("where ", "")
        .replace("when ", "")
        .replace("how ", "")
        .replace("should i ", "")
        .replace(" i ", " ")
        .replace(" do you ", " ")
        .replace(" you ", " ");
    
    // Common filler words to skip (expanded list)
    let stop_words = [
        "the", "a", "an", "is", "are", "for", "to", "of", "in", "on", "at",
        "should", "would", "could", "will", "can", "do", "does", "did",
        "be", "been", "being", "or", "and", "but", "if", "then", "than",
        "with", "from", "by", "as", "into", "through", "during", "before",
        "after", "above", "below", "between", "under", "again", "further",
    ];
    
    // Extract key words (skip filler words)
    let words: Vec<&str> = cleaned
        .split_whitespace()
        .filter(|w| !stop_words.contains(w))
        .take(2) // Take first 2 meaningful words
        .collect();
    
    if words.is_empty() {
        return "param".to_string();
    }
    
    // Join with slash for namespace-like structure
    words.join("/")
}

fn build_two_turn_plan(q1: &str, q2: &str) -> String {
    let first = escape_quotes(q1);
    let second = escape_quotes(q2);
    format!(
        "(do\n  (let [first (call :ccos.user.ask \"{first}\")\n        second (call :ccos.user.ask \"{second}\")]\n    {{:status \"completed\"\n     :conversation [{{:prompt \"{first}\" :answer first}}\n                    {{:prompt \"{second}\" :answer second}}]}}))"
    )
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

fn validate_single_capability(rtfs_source: &str) -> Result<String, String> {
    let trimmed = rtfs_source.trim_start();
    if !trimmed.starts_with("(capability ") {
        if let Some(idx) = trimmed.find("(capability ") {
            let sub = &trimmed[idx..];
            return Ok(extract_balanced_form(sub).unwrap_or_else(|| sub.to_string()));
        }
        return Err("Output does not start with (capability".to_string());
    }

    match parser::parse(rtfs_source) {
        Ok(items) => {
            let cap_count = items
                .iter()
                .filter(|tl| matches!(tl, rtfs_compiler::ast::TopLevel::Capability(_)))
                .count();
            if cap_count == 1 {
                Ok(trimmed.to_string())
            } else {
                Ok(extract_balanced_form(trimmed)
                    .unwrap_or_else(|| trimmed.to_string()))
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

