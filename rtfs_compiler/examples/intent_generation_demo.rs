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
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use reqwest::blocking::Client;

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

    const INTENT_GRAMMAR_SNIPPET: &str = r#"// RTFS PEST grammar (CCOS-aligned excerpt)
intent_def     = { "(intent" ~ symbol ~ newline
                    original_request_clause          ~ newline
                    goal_clause                      ~ newline
                    constraints_clause?              ~ newline
                    preferences_clause?              ~ newline
                    success_criteria_clause?         ~ newline
                    ")" }

// ────────────────────────────────────────────── clauses
goal_clause            = { "(goal" ~ string_lit ~ ")" }
constraints_clause     = { "(constraints"  ~ map_lit ~ ")" }
preferences_clause     = { "(preferences"  ~ map_lit ~ ")" }
success_criteria_clause= { "(success_criteria" ~ expression ~ ")" }
original_request_clause = { "(original_request" ~ string_lit ~ ")" }

// ────────────────────────────────────────────── atoms
symbol         = { ASCII_ALPHANUMERIC+ ("/" ASCII_ALPHANUMERIC+)* }
string_lit     = { "\"" ~ (!"\"" ANY)* ~ "\"" }
map_lit        = { "{" ~ (string_lit ~ expression)* ~ "}" }
expression     = _{ string_lit | symbol | number_lit | vector_lit | map_lit }
number_lit     = { ASCII_DIGIT+ }
vector_lit     = { "[" ~ expression* ~ "]" }
newline        = _{ WHITESPACE* }
"#;

    const FEW_SHOTS: &str = r#"### Example 1
User request: "Greet a user by name"
RTFS:
(intent greetings/hello
  (original_request "Greet a user by name")
  (goal "Generate a personalised greeting")
  (constraints { "name-type" "string" })
  (preferences { "tone" "friendly" }))

### Example 2
User request: "Add two integers and return the sum"
RTFS:
(intent math/add
  (original_request "Add two integers and return the sum")
  (goal "Perform addition of x and y")
  (constraints { "x-type" "int" "y-type" "int" })
  (success_criteria (fn [result] (int? result))))
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
        Ok(r) => println!("\n🤖 Suggested RTFS intent:\n{}", r.trim()),
        Err(e) => eprintln!("Error contacting model: {}", e),
    }

    Ok(())
} 