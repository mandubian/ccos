//! Intent Generation Demo using OpenRouter Hunyuan A13B
//!
//! This example asks a remote LLM (Hunyuan A13B served through OpenRouter) to
//! translate a natural-language user request into an RTFS `intent` definition.
//! The goal is to test whether a general-purpose model can "speak RTFS" with a
//! few-shot prompt plus a snippet of the grammar – no fine-tuning.

use rtfs_compiler::ccos::delegation::{ExecTarget, ModelRegistry, StaticDelegationEngine, ModelProvider};
use rtfs_compiler::parser;
use rtfs_compiler::runtime::{Evaluator, ModuleRegistry};
use rtfs_compiler::ccos::remote_models::RemoteModelConfig;
use rtfs_compiler::ccos::loaders::intent_from_rtfs;
use rtfs_compiler::ast::TopLevel;
use rtfs_compiler::runtime::values::Value;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use reqwest::blocking::Client;
use regex::Regex; // Add dependency in Cargo.toml if not present
use rtfs_compiler::ccos::types::Intent;

/// Minimal blocking OpenRouter provider for the Hunyuan model
#[derive(Debug)]
struct CustomOpenRouterModel {
    id: &'static str,
    config: RemoteModelConfig,
    client: Arc<Client>,
}

impl CustomOpenRouterModel {
    fn new(id: &'static str, model_name: &str) -> Self {
        let config = RemoteModelConfig::new(
            std::env::var("OPENROUTER_API_KEY").unwrap_or_default(),
            model_name.to_string(),
        );
        let client = Arc::new(Client::builder()
            .timeout(std::time::Duration::from_secs(config.timeout_seconds))
            .build()
            .expect("HTTP client"));

        Self { id, config, client }
    }
}

impl ModelProvider for CustomOpenRouterModel {
    fn id(&self) -> &'static str { self.id }

    fn infer(&self, prompt: &str) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        // Same request format as OpenAI chat
        let request = serde_json::json!({
            "model": self.config.model_name,
            "messages": [{ "role": "user", "content": prompt }],
            "max_tokens": self.config.max_tokens,
            "temperature": self.config.temperature,
        });
        let resp: serde_json::Value = self.client
            .post("https://openrouter.ai/api/v1/chat/completions")
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .header("Content-Type", "application/json")
            .header("HTTP-Referer", "https://rtfs-compiler.example.com")
            .header("X-Title", "RTFS Compiler")
            .json(&request)
            .send()?
            .json()?;
        Ok(resp["choices"][0]["message"]["content"].as_str().unwrap_or("").to_string())
    }
}

// ------------------------- NEW: extractor helper -------------------------
/// Extracts the first top-level `(intent …)` s-expression from the given text.
/// Returns `None` if no well-formed intent block is found.
fn extract_intent(text: &str) -> Option<String> {
    // Locate the starting position of the "(intent" keyword
    let start = text.find("(intent")?;

    // Scan forward and track parenthesis depth to find the matching ')'
    let mut depth = 0usize;
    for (idx, ch) in text[start..].char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => {
                depth = depth.saturating_sub(1);
                // When we return to depth 0 we've closed the original "(intent"
                if depth == 0 {
                    let end = start + idx + 1; // inclusive of current ')'
                    return Some(text[start..end].to_string());
                }
            }
            _ => {}
        }
    }
    None
}
// -------------------------------------------------------------------------

/// Replace #rx"pattern" literals with plain "pattern" string literals so the current
/// grammar (which lacks regex literals) can parse the intent.
fn sanitize_regex_literals(text: &str) -> String {
    // Matches #rx"..." with minimal escaping (no nested quotes inside pattern)
    let re = Regex::new(r#"#rx\"([^\"]*)\""#).unwrap();
    re.replace_all(text, |caps: &regex::Captures| {
        format!("\"{}\"", &caps[1])
    }).into_owned()
}

// Helper: convert parser Literal to runtime Value (basic subset)
fn lit_to_val(lit: &rtfs_compiler::ast::Literal) -> Value {
    use rtfs_compiler::ast::Literal as Lit;
    match lit {
        Lit::String(s) => Value::String(s.clone()),
        Lit::Integer(i) => Value::Integer(*i),
        Lit::Float(f) => Value::Float(*f),
        Lit::Boolean(b) => Value::Boolean(*b),
        _ => Value::Nil,
    }
}

fn expr_to_value(expr: &rtfs_compiler::ast::Expression) -> Value {
    use rtfs_compiler::ast::{Expression as E, MapKey};
    match expr {
        E::Literal(lit) => lit_to_val(lit),
        E::Map(m) => {
            let mut map = std::collections::HashMap::new();
            for (k, v) in m {
                map.insert(k.clone(), expr_to_value(v));
            }
            Value::Map(map)
        }
        E::Vector(vec) | E::List(vec) => {
            let vals = vec.iter().map(expr_to_value).collect();
            if matches!(expr, E::Vector(_)) { Value::Vector(vals) } else { Value::List(vals) }
        }
        E::Symbol(s) => Value::Symbol(rtfs_compiler::ast::Symbol(s.0.clone())),
        _ => Value::Nil,
    }
}

fn map_expr_to_string_value(expr: &rtfs_compiler::ast::Expression) -> Option<std::collections::HashMap<String, Value>> {
    use rtfs_compiler::ast::{Expression as E, MapKey};
    if let E::Map(m) = expr {
        let mut out = std::collections::HashMap::new();
        for (k, v) in m {
            let key_str = match k {
                MapKey::Keyword(k) => k.0.clone(),
                MapKey::String(s) => s.clone(),
                MapKey::Integer(i) => i.to_string(),
            };
            out.insert(key_str, expr_to_value(v));
        }
        Some(out)
    } else {
        None
    }
}

fn intent_from_function_call(expr: &rtfs_compiler::ast::Expression) -> Option<Intent> {
    use rtfs_compiler::ast::{Expression as E, Symbol};
    if let E::FunctionCall { callee, arguments } = expr {
        if let E::Symbol(Symbol(sym)) = &**callee {
            if sym == "intent" {
                if arguments.is_empty() { return None; }
                // name symbol
                let name = if let E::Symbol(Symbol(name_sym)) = &arguments[0] {
                    name_sym.clone()
                } else {
                    return None;
                };
                let mut original_request = String::new();
                let mut goal = String::new();
                let mut constraints = std::collections::HashMap::new();
                let mut preferences = std::collections::HashMap::new();
                let mut success_criteria: Option<Value> = None;
                for arg in &arguments[1..] {
                    if let E::FunctionCall { callee, arguments: prop_args } = arg {
                        if let E::Symbol(Symbol(prop_name)) = &**callee {
                            match prop_name.as_str() {
                                "original_request" => {
                                    if let Some(E::Literal(rtfs_compiler::ast::Literal::String(s))) = prop_args.get(0) {
                                        original_request = s.clone();
                                    }
                                }
                                "goal" => {
                                    if let Some(E::Literal(rtfs_compiler::ast::Literal::String(s))) = prop_args.get(0) {
                                        goal = s.clone();
                                    }
                                }
                                "constraints" => {
                                    if let Some(expr) = prop_args.get(0) {
                                        if let Some(m) = map_expr_to_string_value(expr) {
                                            constraints = m;
                                        }
                                    }
                                }
                                "preferences" => {
                                    if let Some(expr) = prop_args.get(0) {
                                        if let Some(m) = map_expr_to_string_value(expr) {
                                            preferences = m;
                                        }
                                    }
                                }
                                "success_criteria" => {
                                    if let Some(expr) = prop_args.get(0) {
                                        success_criteria = Some(expr_to_value(expr));
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
                if goal.is_empty() {
                    goal = original_request.clone();
                }
                let mut intent = Intent::with_name(name, original_request.clone(), goal);
                intent.constraints = constraints;
                intent.preferences = preferences;
                intent.success_criteria = success_criteria;
                return Some(intent);
            }
        }
    }
    None
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🧪 RTFS Intent Generation Demo\n===============================\n");

    // Verify API key
    let api_key = std::env::var("OPENROUTER_API_KEY").unwrap_or_default();
    if api_key.is_empty() {
        println!("❌ OPENROUTER_API_KEY not set – the demo will only print the prompt.\n");
    }

    // ---------------------------------------------------------------------
    // Build prompt: grammar snippet + few-shot examples + user request
    // ---------------------------------------------------------------------

    const INTENT_GRAMMAR_SNIPPET: &str = r#"// RTFS 2.0 Intent (property-style)
// Basic shape (keywords are properties)
intent_def = { "(intent" ~ core_type_kw ~ property* ~ ")" }

core_type_kw = { ":rtfs.core:v2.0:intent" }

// ─────── properties (order flexible) ───────
property      = _{ goal_prop | original_request_prop | constraints_prop | preferences_prop |
                   success_criteria_prop | id_prop | created_at_prop | created_by_prop |
                   status_prop | priority_prop | map_prop }

goal_prop              = { ":goal"               ~ string_lit }
original_request_prop  = { ":original-request"   ~ string_lit }
constraints_prop       = { ":constraints"        ~ map_lit }
preferences_prop       = { ":preferences"        ~ map_lit }
success_criteria_prop  = { ":success-criteria"   ~ expression }
id_prop                = { ":intent-id"          ~ string_lit }
created_at_prop        = { ":created-at"         ~ string_lit }
created_by_prop        = { ":created-by"         ~ string_lit }
status_prop            = { ":status"             ~ string_lit }
priority_prop          = { ":priority"           ~ string_lit }
map_prop               = { keyword ~ expression } // fallback for custom metadata

// ─────── atoms ───────
keyword        = { ":" ~ ASCII_ALPHANUMERIC+ ("-"? ASCII_ALPHANUMERIC+)* }
symbol         = { ASCII_ALPHANUMERIC+ ("/" ASCII_ALPHANUMERIC+)* }
string_lit     = { "\"" ~ (!"\"" ANY)* ~ "\"" }
map_lit        = { "{" ~ (string_lit | keyword ~ expression)* ~ "}" }
expression     = _{ string_lit | symbol | number_lit | vector_lit | map_lit }
number_lit     = { ASCII_DIGIT+ }
vector_lit     = { "[" ~ expression* ~ "]" }
"#;

    const FEW_SHOTS: &str = r#"### Example 1
User request: "Greet a user by name"
RTFS:
(intent rtfs.core:v2.0:intent
  :type         :rtfs.core:v2.0:intent
  :intent-id    "intent-hello-001"
  :goal         "Generate a personalised greeting"
  :original-request "Greet a user by name"
  :constraints  { :name-type :string }
  :preferences  { :tone :friendly }
  :status       "active")

### Example 2
User request: "Add two integers and return the sum"
RTFS:
(intent rtfs.core:v2.0:intent
  :type         :rtfs.core:v2.0:intent
  :intent-id    "intent-add-001"
  :goal         "Perform addition of x and y"
  :original-request "Add two integers and return the sum"
  :constraints  { :x-type :int :y-type :int }
  :success-criteria (fn [result] (int? result))
  :status       "active")
"#;

    // ---------------------------------------------------------------------
    // Build runtime registry / evaluator with Hunyuan provider
    // ---------------------------------------------------------------------
    let registry = ModelRegistry::new();
    let hunyuan = CustomOpenRouterModel::new(
        "openrouter-hunyuan-a13b-instruct",
        "tencent/hunyuan-a13b-instruct:free",
    );
    registry.register(hunyuan);

    // Delegation engine: always use remote model for our generator function
    let mut static_map = HashMap::new();
    static_map.insert(
        "nl->intent".to_string(),
        ExecTarget::RemoteModel("openrouter-hunyuan-a13b-instruct".to_string()),
    );
    let delegation = Arc::new(StaticDelegationEngine::new(static_map));

    // Evaluator (we won't actually evaluate the generated intent here, but set up for future)
    let mut evaluator = Evaluator::new(Rc::new(ModuleRegistry::new()), delegation);
    evaluator.model_registry = Arc::new(registry);

    // ---------------------------------------------------------------------
    // Ask user for a request (or use default)
    // ---------------------------------------------------------------------
    let user_request = std::env::args().nth(1).unwrap_or_else(|| {
        "Get the current UTC time and return it as an ISO-8601 string".to_string()
    });

    let full_prompt = format!(
        "You are an expert RTFS developer. Using the RTFS grammar below, write an `intent` definition that fulfils the user request.\n\nGrammar snippet:\n```\n{}\n```\n\n{}\n### Task\nUser request: \"{}\"\nRTFS:",
        INTENT_GRAMMAR_SNIPPET, FEW_SHOTS, user_request
    );

    println!("📜 Prompt sent to Hunyuan:\n{}\n---", full_prompt);

    if api_key.is_empty() {
        println!("(Set OPENROUTER_API_KEY to execute the call.)");
        return Ok(());
    }

    // Directly call the provider for simplicity
    let provider = evaluator
        .model_registry
        .get("openrouter-hunyuan-a13b-instruct")
        .expect("provider registered");

    match provider.infer(&full_prompt) {
        Ok(r) => {
            match extract_intent(&r) {
                Some(intent_block) => {
                    println!("\n🎯 Extracted RTFS intent:\n{}", intent_block.trim());

                    // -------- Parse and enhance --------
                    let sanitized = sanitize_regex_literals(&intent_block);
                    match parser::parse(&sanitized) {
                        Ok(ast_items) => {
                            // DEBUG: print entire AST items
                            println!("\n🔍 Parsed AST items: {:#?}", ast_items);
                            if let Some(def) = ast_items.iter().find_map(|t| {
                                if let TopLevel::Intent(d) = t { Some(d) } else { None }
                            }) {
                                match intent_from_rtfs(def) {
                                    Ok(mut ccos_intent) => {
                                        if ccos_intent.constraints.is_empty() {
                                            ccos_intent.constraints.insert(
                                                "note".into(),
                                                Value::String("no-constraints-specified".into()),
                                            );
                                        }
                                        if ccos_intent.preferences.is_empty() {
                                            ccos_intent.preferences.insert(
                                                "note".into(),
                                                Value::String("no-preferences-specified".into()),
                                            );
                                        }
                                        if ccos_intent.success_criteria.is_none() {
                                            ccos_intent.success_criteria = Some(Value::Nil);
                                        }
                                        // Print the enriched struct
                                        println!("\n🪄 Enriched CCOS Intent struct:\n{:#?}", ccos_intent);
                                    }
                                    Err(e) => eprintln!("Error converting to CCOS Intent: {}", e),
                                }
                            } else {
                                eprintln!("Parsed AST did not contain an IntentDefinition");
                            }
                        }
                        Err(e) => eprintln!("Failed to parse extracted intent: {:?}", e),
                    }
                }
                None => println!("\n⚠️  Could not locate a complete (intent …) block. Raw response:\n{}", r.trim()),
            }
        },
        Err(e) => eprintln!("Error contacting model: {}", e),
    }

    Ok(())
} 