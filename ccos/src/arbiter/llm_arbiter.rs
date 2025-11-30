//! LLM-Driven Arbiter Implementation
//!
//! This module provides an LLM-driven implementation of the Arbiter that uses
//! LLM providers to generate intents and plans with structured prompts.

use crate::delegation_keys::generation;
use crate::intent_graph::IntentGraph;
use crate::rtfs_bridge::graph_interpreter::build_graph_from_rtfs;
use crate::types::{
    ExecutionResult, GenerationContext, Intent, IntentStatus, Plan, StorableIntent, TriggerSource,
};
use async_trait::async_trait;
use regex;
use rtfs::runtime::error::RuntimeError;
use rtfs::runtime::values::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use super::arbiter_config::ArbiterConfig;
use super::arbiter_engine::ArbiterEngine;
use super::llm_provider::{LlmProvider, LlmProviderConfig, LlmProviderFactory};
use super::plan_generation::{
    LlmRtfsPlanGenerationProvider, PlanGenerationProvider, PlanGenerationResult,
};
use super::prompt::{FilePromptStore, PromptConfig, PromptManager};
use rtfs::ast::TopLevel;

/// Extract the first top-level `(intent …)` s-expression from the given text.
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

/// Replace #rx"pattern" literals with plain "pattern" string literals so the current
/// grammar (which lacks regex literals) can parse the intent.
fn sanitize_regex_literals(text: &str) -> String {
    // Matches #rx"..." with minimal escaping (no nested quotes inside pattern)
    let re = regex::Regex::new(r#"#rx\"([^\"]*)\""#).unwrap();
    re.replace_all(text, |caps: &regex::Captures| format!("\"{}\"", &caps[1]))
        .into_owned()
}

/// Convert parser Literal to runtime Value (basic subset)
fn lit_to_val(lit: &rtfs::ast::Literal) -> Value {
    use rtfs::ast::Literal as Lit;
    match lit {
        Lit::String(s) => Value::String(s.clone()),
        Lit::Integer(i) => Value::Integer(*i),
        Lit::Float(f) => Value::Float(*f),
        Lit::Boolean(b) => Value::Boolean(*b),
        _ => Value::Nil,
    }
}

fn expr_to_value(expr: &rtfs::ast::Expression) -> Value {
    use rtfs::ast::Expression as E;
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
            if matches!(expr, E::Vector(_)) {
                Value::Vector(vals)
            } else {
                Value::List(vals)
            }
        }
        E::Symbol(s) => Value::Symbol(rtfs::ast::Symbol(s.0.clone())),
        E::FunctionCall { callee, arguments } => {
            // Convert function calls to a list representation for storage
            let mut func_list = vec![expr_to_value(callee)];
            func_list.extend(arguments.iter().map(expr_to_value));
            Value::List(func_list)
        }
        E::Fn(fn_expr) => {
            // Convert fn expressions to a list representation: (fn params body...)
            let mut fn_list = vec![Value::Symbol(rtfs::ast::Symbol("fn".to_string()))];

            // Add parameters as a vector
            let mut params = Vec::new();
            for param in &fn_expr.params {
                params.push(Value::Symbol(rtfs::ast::Symbol(format!(
                    "{:?}",
                    param.pattern
                ))));
            }
            fn_list.push(Value::Vector(params));

            // Add body expressions
            for body_expr in &fn_expr.body {
                fn_list.push(expr_to_value(body_expr));
            }

            Value::List(fn_list)
        }
        _ => Value::Nil,
    }
}

fn map_expr_to_string_value(
    expr: &rtfs::ast::Expression,
) -> Option<std::collections::HashMap<String, Value>> {
    use rtfs::ast::{Expression as E, MapKey};
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

fn intent_from_function_call(expr: &rtfs::ast::Expression) -> Option<Intent> {
    use rtfs::ast::{Expression as E, Literal, Symbol};

    let E::FunctionCall { callee, arguments } = expr else {
        return None;
    };
    let E::Symbol(Symbol(sym)) = &**callee else {
        return None;
    };
    if sym != "intent" {
        return None;
    }
    if arguments.is_empty() {
        return None;
    }

    // The first argument is the intent name/type, can be either a symbol or string literal
    let name = if let E::Symbol(Symbol(name_sym)) = &arguments[0] {
        name_sym.clone()
    } else if let E::Literal(Literal::String(name_str)) = &arguments[0] {
        name_str.clone()
    } else {
        return None; // First argument must be a symbol or string
    };

    let mut properties = HashMap::new();
    let mut args_iter = arguments[1..].chunks_exact(2);
    while let Some([key_expr, val_expr]) = args_iter.next() {
        if let E::Literal(Literal::Keyword(k)) = key_expr {
            properties.insert(k.0.clone(), val_expr);
        }
    }

    let original_request = properties
        .get("original-request")
        .and_then(|expr| {
            if let E::Literal(Literal::String(s)) = expr {
                Some(s.clone())
            } else {
                None
            }
        })
        .unwrap_or_default();

    let goal = properties
        .get("goal")
        .and_then(|expr| {
            if let E::Literal(Literal::String(s)) = expr {
                Some(s.clone())
            } else {
                None
            }
        })
        .unwrap_or_else(|| original_request.clone());

    let mut intent = Intent::new(goal).with_name(name);

    if let Some(expr) = properties.get("constraints") {
        if let Some(m) = map_expr_to_string_value(expr) {
            intent.constraints = m;
        }
    }

    if let Some(expr) = properties.get("preferences") {
        if let Some(m) = map_expr_to_string_value(expr) {
            intent.preferences = m;
        }
    }

    if let Some(expr) = properties.get("success-criteria") {
        let value = expr_to_value(expr);
        intent.success_criteria = Some(value);
    }

    Some(intent)
}

/// LLM-driven arbiter implementation
pub struct LlmArbiter {
    config: ArbiterConfig,
    llm_provider: Box<dyn LlmProvider>,
    intent_graph: Arc<Mutex<IntentGraph>>,
}

impl LlmArbiter {
    /// Create a new LLM-driven arbiter
    pub async fn new(
        config: ArbiterConfig,
        intent_graph: Arc<Mutex<IntentGraph>>,
    ) -> Result<Self, RuntimeError> {
        let llm_config = config.llm_config.as_ref().ok_or_else(|| {
            RuntimeError::Generic("LLM configuration required for LlmArbiter".to_string())
        })?;

        let llm_provider_config = LlmProviderConfig {
            provider_type: llm_config.provider_type.clone(),
            model: llm_config.model.clone(),
            api_key: llm_config.api_key.clone(),
            base_url: llm_config.base_url.clone(),
            max_tokens: llm_config.max_tokens,
            temperature: llm_config.temperature,
            timeout_seconds: llm_config.timeout_seconds,
            retry_config: llm_config.retry_config.clone(),
        };

        let llm_provider = LlmProviderFactory::create_provider(llm_provider_config).await?;

        Ok(Self {
            config,
            llm_provider,
            intent_graph,
        })
    }

    /// Generate a structured prompt for intent generation (centralized, versioned)
    fn generate_intent_prompt(
        &self,
        natural_language: &str,
        context: Option<HashMap<String, Value>>,
    ) -> String {
        let prompt_cfg: PromptConfig = self
            .config
            .llm_config
            .as_ref()
            .and_then(|c| c.prompts.clone())
            .unwrap_or_default();
        let store = FilePromptStore::new("assets/prompts/arbiter");
        let manager = PromptManager::new(store);
        let context_str =
            serde_json::to_string(&context).unwrap_or_else(|_| format!("{:?}", context));
        let available_capabilities = vec!["ccos.echo".to_string(), "ccos.math.add".to_string()];
        let available_caps_str = serde_json::to_string(&available_capabilities)
            .unwrap_or_else(|_| format!("{:?}", available_capabilities));
        let mut vars = std::collections::HashMap::new();
        vars.insert("natural_language".to_string(), natural_language.to_string());
        vars.insert("context".to_string(), context_str.clone());
        vars.insert(
            "available_capabilities".to_string(),
            available_caps_str.clone(),
        );

        manager
            .render(
                &prompt_cfg.intent_prompt_id,
                &prompt_cfg.intent_prompt_version,
                &vars,
            )
            .unwrap_or_else(|_| "".to_string())
    }

    /// Generate a structured prompt for plan generation
    fn generate_plan_prompt(&self, intent: &Intent) -> String {
        let available_capabilities = vec!["ccos.echo".to_string(), "ccos.math.add".to_string()];
        let available_caps_str = serde_json::to_string(&available_capabilities)
            .unwrap_or_else(|_| format!("{:?}", available_capabilities));
        let mut s = String::new();
        s.push_str("Generate an RTFS plan to achieve this intent:\n\n");
        s.push_str("Intent Goal: ");
        s.push_str(&intent.goal);
        s.push_str("\n\nAvailable capabilities: ");
        s.push_str(&available_caps_str);
        s.push_str(
            "\n\nGenerate a plan using RTFS syntax with step special forms. The plan should:\n",
        );
        s.push_str("1. Use (step \"Step Name\" (call :capability.name args)) for each step\n");
        s.push_str("2. Use (do ...) to group multiple steps\n");
        s.push_str("3. Include appropriate error handling\n");
        s.push_str("4. Be specific and actionable\n\n");
        s.push_str("Example plan structure:\n(do\n  (step \"Fetch Data\" (call :ccos.echo \"fetching data\"))\n  (step \"Process Data\" (call :ccos.echo \"processing data\"))\n  (step \"Generate Report\" (call :ccos.echo \"report generated\"))\n)\n\nGenerate the plan:");
        s
    }

    /// Parse RTFS response into intent structure using RTFS parser
    fn parse_rtfs_intent_response(
        &self,
        response: &str,
        _natural_language: &str,
        _context: Option<HashMap<String, Value>>,
    ) -> Result<Intent, RuntimeError> {
        // Extract the first top-level `(intent …)` s-expression from the response
        let intent_block = extract_intent(response).ok_or_else(|| {
            RuntimeError::Generic("Could not locate a complete (intent …) block".to_string())
        })?;

        // Sanitize regex literals for parsing
        let sanitized = sanitize_regex_literals(&intent_block);

        // Parse using RTFS parser
        let ast_items = rtfs::parser::parse(&sanitized)
            .map_err(|e| RuntimeError::Generic(format!("Failed to parse RTFS intent: {}", e)))?;

        // Find the first expression and convert to Intent
        if let Some(TopLevel::Expression(expr)) = ast_items.get(0) {
            intent_from_function_call(&expr).ok_or_else(|| {
                RuntimeError::Generic(
                    "Parsed AST expression was not a valid intent definition".to_string(),
                )
            })
        } else {
            Err(RuntimeError::Generic(
                "Parsed AST did not contain a top-level expression for the intent".to_string(),
            ))
        }
    }

    /// Generate a structured prompt instructing the LLM to emit a full RTFS intent graph.
    /// Output must be a single (do ...) s-expression containing (intent ...) and (edge ...)
    fn generate_graph_prompt(&self, natural_language_goal: &str) -> String {
        // Marker to help stub provider route responses deterministically
        let marker = "GENERATE_INTENT_GRAPH";

        // Keep grammar aligned with graph_interpreter.rs docstring
        let body = r#"You are an Arbiter that translates a natural language goal into an RTFS intent graph.

Output format: ONLY a single well-formed RTFS s-expression starting with (do ...). No prose, no JSON.

Inside (do ...), include:
- One or more intent definitions using ONE of these forms (both accepted):
    (intent "name" :goal "..." :constraints {{...}} :preferences {{...}} :success-criteria <expr>)
    {{:type "intent" :name "name" :goal "..."}}
- Zero or more edges using ONE of these forms:
    (edge {{:from "child" :to "parent" :type :IsSubgoalOf}})
    (edge :DependsOn "from" "to")

Edge types allowed: :IsSubgoalOf, :DependsOn, :ConflictsWith, :Enables, :RelatedTo, :TriggeredBy, :Blocks

Constraints:
- Names must be simple strings. Goals must be clear strings.
- Do NOT include markdown fences, comments, or extra text.

Example shape (illustrative):
(do
  {{:type "intent" :name "root" :goal "Overall goal"}}
  {{:type "intent" :name "fetch" :goal "Fetch data"}}
  (edge :IsSubgoalOf "fetch" "root")
)

Now generate the RTFS graph for this goal:"#;

        let mut s = String::new();
        s.push_str(marker);
        s.push_str("\n");
        s.push_str(body);
        s.push_str("\n\"");
        s.push_str(natural_language_goal);
        s.push_str("\"");
        s
    }

    /// Extract the first top-level (do ...) s-expression from a text blob.
    fn extract_do_block(text: &str) -> Option<String> {
        let start = text.find("(do");
        let start = match start {
            Some(s) => s,
            None => return None,
        };
        let mut depth = 0usize;
        for (idx, ch) in text[start..].char_indices() {
            match ch {
                '(' => depth += 1,
                ')' => {
                    if depth == 0 {
                        return None;
                    }
                    depth -= 1;
                    if depth == 0 {
                        let end = start + idx + 1;
                        return Some(text[start..end].to_string());
                    }
                }
                _ => {}
            }
        }
        None
    }

    /// Store intent in the intent graph
    async fn store_intent(&self, intent: &Intent) -> Result<(), RuntimeError> {
        let mut graph = self
            .intent_graph
            .lock()
            .map_err(|_| RuntimeError::Generic("Failed to lock IntentGraph".to_string()))?;

        let storable_intent = StorableIntent {
            intent_id: intent.intent_id.clone(),
            name: intent.name.clone(),
            original_request: intent.original_request.clone(),
            rtfs_intent_source: String::new(),
            goal: intent.goal.clone(),
            constraints: intent
                .constraints
                .iter()
                .map(|(k, v)| (k.clone(), format!("{}", v)))
                .collect(),
            preferences: intent
                .preferences
                .iter()
                .map(|(k, v)| (k.clone(), format!("{}", v)))
                .collect(),
            success_criteria: intent.success_criteria.as_ref().map(|v| format!("{}", v)),
            parent_intent: None,
            child_intents: vec![],
            triggered_by: TriggerSource::HumanRequest,
            generation_context: GenerationContext {
                arbiter_version: "llm-arbiter-1.0".to_string(),
                generation_timestamp: intent.created_at,
                input_context: HashMap::new(),
                reasoning_trace: None,
            },
            status: IntentStatus::Active,
            priority: 1,
            created_at: intent.created_at,
            updated_at: intent.updated_at,
            metadata: intent
                .metadata
                .iter()
                .map(|(k, v)| (k.clone(), format!("{}", v)))
                .collect(),
        };

        graph.store_intent(storable_intent)?;
        Ok(())
    }

    /// Validate plan using LLM provider
    async fn validate_plan(&self, plan: &Plan) -> Result<bool, RuntimeError> {
        let plan_content = match &plan.body {
            crate::types::PlanBody::Rtfs(content) => content,
            crate::types::PlanBody::Wasm(_) => {
                return Err(RuntimeError::Generic(
                    "WASM plans not supported for validation".to_string(),
                ));
            }
        };

        let validation_result = self.llm_provider.validate_plan(plan_content).await?;
        Ok(validation_result.is_valid)
    }
}

#[async_trait(?Send)]
impl ArbiterEngine for LlmArbiter {
    async fn natural_language_to_intent(
        &self,
        natural_language: &str,
        context: Option<HashMap<String, Value>>,
    ) -> Result<Intent, RuntimeError> {
        // Generate prompt for intent generation
        let prompt = self.generate_intent_prompt(natural_language, context.clone());

        // Debug mode flag (set RTFS_ARBITER_DEBUG=1 or reuse RTFS_SHOW_PROMPTS)
        let debug = std::env::var("RTFS_ARBITER_DEBUG")
            .map(|v| v == "1")
            .unwrap_or(false)
            || std::env::var("RTFS_SHOW_PROMPTS").is_ok();

        if debug {
            println!("\n📝 Intent generation prompt:");
            println!("--- PROMPT START ---");
            println!("{}", prompt);
            println!("--- PROMPT END ---");
        }

        // Use LLM provider to generate text response
        let response = self.llm_provider.generate_text(&prompt).await?;

        if debug {
            println!("\n🤖 LLM response (intent):");
            println!("--- RESPONSE START ---");
            println!("{}", response);
            println!("--- RESPONSE END ---");
        }

        // Parse RTFS response into intent structure
        let mut intent = self.parse_rtfs_intent_response(&response, natural_language, context)?;

        // Ensure the original user request is recorded on the intent
        intent.original_request = natural_language.to_string();

        // Mark generation method for downstream consumers/tests
        intent.metadata.insert(
            generation::GENERATION_METHOD.to_string(),
            rtfs::runtime::values::Value::String(generation::methods::LLM.to_string()),
        );

        if debug {
            println!("\n📋 Parsed intent:");
            println!("  Name: {}", intent.name.as_deref().unwrap_or("<none>"));
            println!("  Goal: {}", intent.goal);
            println!("  Original Request: {}", intent.original_request);
            if !intent.constraints.is_empty() {
                println!("  Constraints: {} keys", intent.constraints.len());
            }
            if !intent.preferences.is_empty() {
                println!("  Preferences: {} keys", intent.preferences.len());
            }
        }

        // Store the generated intent
        self.store_intent(&intent).await?;

        Ok(intent)
    }

    async fn intent_to_plan(&self, intent: &Intent) -> Result<Plan, RuntimeError> {
        // Generate prompt for plan generation
        let _prompt = self.generate_plan_prompt(intent);

        // Use LLM provider to generate plan
        // Build a storable intent shell to pass to provider (using runtime intent fields)
        let storable = StorableIntent {
            intent_id: intent.intent_id.clone(),
            name: intent.name.clone(),
            original_request: intent.original_request.clone(),
            rtfs_intent_source: String::new(),
            goal: intent.goal.clone(),
            constraints: HashMap::new(),
            preferences: HashMap::new(),
            success_criteria: None,
            parent_intent: None,
            child_intents: vec![],
            triggered_by: TriggerSource::HumanRequest,
            generation_context: GenerationContext {
                arbiter_version: "llm-arbiter-1.0".to_string(),
                generation_timestamp: intent.created_at,
                input_context: HashMap::new(),
                reasoning_trace: None,
            },
            status: IntentStatus::Active,
            priority: 0,
            created_at: intent.created_at,
            updated_at: intent.updated_at,
            metadata: HashMap::new(),
        };
        let plan = self.llm_provider.generate_plan(&storable, None).await?;

        Ok(plan)
    }

    async fn execute_plan(&self, _plan: &Plan) -> Result<ExecutionResult, RuntimeError> {
        Ok(ExecutionResult {
            success: true,
            value: Value::String("LLM arbiter execution placeholder".to_string()),
            metadata: HashMap::new(),
        })
    }

    async fn natural_language_to_graph(
        &self,
        natural_language_goal: &str,
    ) -> Result<String, RuntimeError> {
        // Generate RTFS graph prompt and ask provider to emit a (do ...) graph form
        let prompt = self.generate_graph_prompt(natural_language_goal);

        let debug = std::env::var("RTFS_ARBITER_DEBUG")
            .map(|v| v == "1")
            .unwrap_or(false)
            || std::env::var("RTFS_SHOW_PROMPTS").is_ok();
        if debug {
            println!("\n🧭 Intent-Graph generation prompt:\n--- PROMPT START ---\n{}\n--- PROMPT END ---", prompt);
        }

        let response = self.llm_provider.generate_text(&prompt).await?;
        if debug {
            println!(
                "\n🤖 LLM response (graph):\n--- RESPONSE START ---\n{}\n--- RESPONSE END ---",
                response
            );
        }

        // Extract first (do ...) block from response
        let do_block = Self::extract_do_block(&response).ok_or_else(|| {
            RuntimeError::Generic(
                "Could not locate a complete (do ...) RTFS graph block".to_string(),
            )
        })?;

        // Populate IntentGraph using the interpreter and return root intent id
        let mut graph = self
            .intent_graph
            .lock()
            .map_err(|_| RuntimeError::Generic("Failed to lock IntentGraph".to_string()))?;
        let root_id = build_graph_from_rtfs(&do_block, &mut graph)?;
        Ok(root_id)
    }

    async fn generate_plan_for_intent(
        &self,
        intent: &StorableIntent,
    ) -> Result<PlanGenerationResult, RuntimeError> {
        // Use LLM provider-based plan generator
        let provider_cfg = self
            .config
            .llm_config
            .as_ref()
            .ok_or_else(|| RuntimeError::Generic("llm_config missing".to_string()))?
            .to_provider_config();
        let provider = LlmRtfsPlanGenerationProvider::new(provider_cfg);

        // Convert storable intent back to runtime Intent (minimal fields)
        let rt_intent = Intent {
            intent_id: intent.intent_id.clone(),
            name: intent.name.clone(),
            original_request: intent.original_request.clone(),
            goal: intent.goal.clone(),
            constraints: HashMap::new(),
            preferences: HashMap::new(),
            success_criteria: None,
            status: IntentStatus::Active,
            created_at: intent.created_at,
            updated_at: intent.updated_at,
            metadata: HashMap::new(),
        };

        // For now, we don't pass a real marketplace; provider currently doesn't use it.
        let marketplace = Arc::new(crate::capability_marketplace::CapabilityMarketplace::new(
            Arc::new(tokio::sync::RwLock::new(
                crate::capabilities::registry::CapabilityRegistry::new(),
            )),
        ));
        provider.generate_plan(&rt_intent, marketplace).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arbiter::arbiter_config::{ArbiterConfig, LlmConfig, LlmProviderType};

    #[tokio::test]
    async fn test_llm_arbiter_creation() {
        let config = ArbiterConfig {
            engine_type: crate::arbiter::arbiter_config::ArbiterEngineType::Llm,
            llm_config: Some(LlmConfig {
                provider_type: LlmProviderType::Stub,
                model: "stub-model".to_string(),
                api_key: None,
                base_url: None,
                max_tokens: Some(1000),
                temperature: Some(0.7),
                timeout_seconds: Some(30),
                retry_config: crate::arbiter::arbiter_config::RetryConfig::default(),
                prompts: None,
            }),
            delegation_config: None,
            capability_config: crate::arbiter::arbiter_config::CapabilityConfig::default(),
            security_config: crate::arbiter::arbiter_config::SecurityConfig::default(),
            template_config: None,
        };

        let intent_graph = Arc::new(Mutex::new(IntentGraph::new().unwrap()));
        let arbiter = LlmArbiter::new(config, intent_graph).await.unwrap();

        // Test that the arbiter was created successfully
        assert!(arbiter.llm_provider.get_info().name.contains("Stub"));
    }

    #[tokio::test]
    async fn test_llm_arbiter_intent_generation() {
        let config = ArbiterConfig {
            engine_type: crate::arbiter::arbiter_config::ArbiterEngineType::Llm,
            llm_config: Some(LlmConfig {
                provider_type: LlmProviderType::Stub,
                model: "stub-model".to_string(),
                api_key: None,
                base_url: None,
                max_tokens: Some(1000),
                temperature: Some(0.7),
                timeout_seconds: Some(30),
                retry_config: crate::arbiter::arbiter_config::RetryConfig::default(),
                prompts: None,
            }),
            delegation_config: None,
            capability_config: crate::arbiter::arbiter_config::CapabilityConfig::default(),
            security_config: crate::arbiter::arbiter_config::SecurityConfig::default(),
            template_config: None,
        };

        let intent_graph = Arc::new(Mutex::new(IntentGraph::new().unwrap()));
        let arbiter = LlmArbiter::new(config, intent_graph).await.unwrap();

        let intent = arbiter
            .natural_language_to_intent("analyze sentiment", None)
            .await
            .unwrap();

        assert!(!intent.intent_id.is_empty());
        assert!(!intent.goal.is_empty());
        // accept minor variations (case or expanded phrasing) from the LLM stub
        let req = intent.original_request.to_lowercase();
        assert!(req.contains("analyze") && req.contains("sentiment"));
    }

    #[tokio::test]
    async fn test_llm_arbiter_plan_generation() {
        let config = ArbiterConfig {
            engine_type: crate::arbiter::arbiter_config::ArbiterEngineType::Llm,
            llm_config: Some(LlmConfig {
                provider_type: LlmProviderType::Stub,
                model: "stub-model".to_string(),
                api_key: None,
                base_url: None,
                max_tokens: Some(1000),
                temperature: Some(0.7),
                timeout_seconds: Some(30),
                retry_config: crate::arbiter::arbiter_config::RetryConfig::default(),
                prompts: None,
            }),
            delegation_config: None,
            capability_config: crate::arbiter::arbiter_config::CapabilityConfig::default(),
            security_config: crate::arbiter::arbiter_config::SecurityConfig::default(),
            template_config: None,
        };

        let intent_graph = Arc::new(Mutex::new(IntentGraph::new().unwrap()));
        let arbiter = LlmArbiter::new(config, intent_graph).await.unwrap();

        let intent = arbiter
            .natural_language_to_intent("optimize performance", None)
            .await
            .unwrap();
        let plan = arbiter.intent_to_plan(&intent).await.unwrap();

        assert!(!plan.plan_id.is_empty());
        assert!(matches!(plan.body, crate::types::PlanBody::Rtfs(_)));
        assert_eq!(plan.intent_ids, vec![intent.intent_id]);
    }

    #[tokio::test]
    async fn test_llm_arbiter_full_processing() {
        let config = ArbiterConfig {
            engine_type: crate::arbiter::arbiter_config::ArbiterEngineType::Llm,
            llm_config: Some(LlmConfig {
                provider_type: LlmProviderType::Stub,
                model: "stub-model".to_string(),
                api_key: None,
                base_url: None,
                max_tokens: Some(1000),
                temperature: Some(0.7),
                timeout_seconds: Some(30),
                retry_config: crate::arbiter::arbiter_config::RetryConfig::default(),
                prompts: None,
            }),
            delegation_config: None,
            capability_config: crate::arbiter::arbiter_config::CapabilityConfig::default(),
            security_config: crate::arbiter::arbiter_config::SecurityConfig::default(),
            template_config: None,
        };

        let intent_graph = Arc::new(Mutex::new(IntentGraph::new().unwrap()));
        let arbiter = LlmArbiter::new(config, intent_graph).await.unwrap();

        let result = arbiter
            .process_natural_language("analyze user sentiment", None)
            .await
            .unwrap();

        assert!(result.success);
    }
}
