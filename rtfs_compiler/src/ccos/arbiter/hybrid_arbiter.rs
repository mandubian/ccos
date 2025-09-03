//! Hybrid Arbiter Engine
//!
//! This module provides a hybrid approach that combines template-based pattern matching
//! with LLM fallback for comprehensive intent and plan generation. The hybrid arbiter
//! first attempts to match against predefined templates for speed and determinism,
//! then falls back to LLM reasoning for complex or novel requests.

use std::collections::HashMap;
use async_trait::async_trait;
use regex::Regex;

use crate::runtime::error::RuntimeError;
use crate::runtime::values::Value;
use crate::ccos::types::{Intent, Plan, PlanBody, PlanLanguage, PlanStatus, IntentStatus, StorableIntent, ExecutionResult};
use crate::ccos::arbiter::arbiter_engine::ArbiterEngine;
use crate::ccos::arbiter::arbiter_config::{TemplateConfig, IntentPattern, PlanTemplate, FallbackBehavior, LlmConfig};
use crate::ccos::arbiter::llm_provider::{LlmProvider, LlmProviderFactory};
use crate::ccos::delegation_keys::{generation, agent};
use crate::ccos::arbiter::prompt::{PromptManager, FilePromptStore, PromptConfig};
use crate::ast::TopLevel;

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
    let re = Regex::new(r#"#rx\"([^\"]*)\""#).unwrap();
    re.replace_all(text, |caps: &regex::Captures| {
        format!("\"{}\"", &caps[1])
    }).into_owned()
}

/// Convert parser Literal to runtime Value (basic subset)
fn lit_to_val(lit: &crate::ast::Literal) -> Value {
    use crate::ast::Literal as Lit;
    match lit {
        Lit::String(s) => Value::String(s.clone()),
        Lit::Integer(i) => Value::Integer(*i),
        Lit::Float(f) => Value::Float(*f),
        Lit::Boolean(b) => Value::Boolean(*b),
        _ => Value::Nil,
    }
}

fn expr_to_value(expr: &crate::ast::Expression) -> Value {
    use crate::ast::{Expression as E};
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
        E::Symbol(s) => Value::Symbol(crate::ast::Symbol(s.0.clone())),
        E::FunctionCall { callee, arguments } => {
            // Convert function calls to a list representation for storage
            let mut func_list = vec![expr_to_value(callee)];
            func_list.extend(arguments.iter().map(expr_to_value));
            Value::List(func_list)
        }
        E::Fn(fn_expr) => {
            // Convert fn expressions to a list representation: (fn params body...)
            let mut fn_list = vec![Value::Symbol(crate::ast::Symbol("fn".to_string()))];
            
            // Add parameters as a vector
            let mut params = Vec::new();
            for param in &fn_expr.params {
                params.push(Value::Symbol(crate::ast::Symbol(format!("{:?}", param.pattern))));
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

fn map_expr_to_string_value(expr: &crate::ast::Expression) -> Option<std::collections::HashMap<String, Value>> {
    use crate::ast::{Expression as E, MapKey};
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

fn intent_from_function_call(expr: &crate::ast::Expression) -> Option<Intent> {
    use crate::ast::{Expression as E, Literal, Symbol};

    let E::FunctionCall { callee, arguments } = expr else { return None; };
    let E::Symbol(Symbol(sym)) = &**callee else { return None; };
    if sym != "intent" { return None; }
    if arguments.is_empty() { return None; }

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

    let original_request = properties.get("original-request")
        .and_then(|expr| if let E::Literal(Literal::String(s)) = expr { Some(s.clone()) } else { None })
        .unwrap_or_default();
    
    let goal = properties.get("goal")
        .and_then(|expr| if let E::Literal(Literal::String(s)) = expr { Some(s.clone()) } else { None })
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

/// Hybrid arbiter that combines template matching with LLM fallback
pub struct HybridArbiter {
    template_config: TemplateConfig,
    llm_config: LlmConfig,
    intent_patterns: Vec<IntentPattern>,
    plan_templates: Vec<PlanTemplate>,
    llm_provider: Box<dyn LlmProvider>,
    intent_graph: std::sync::Arc<std::sync::Mutex<crate::ccos::intent_graph::IntentGraph>>,
    fallback_behavior: FallbackBehavior,
}

impl HybridArbiter {
    /// Create a new hybrid arbiter with the given configuration
    pub async fn new(
        template_config: TemplateConfig,
        llm_config: LlmConfig,
        intent_graph: std::sync::Arc<std::sync::Mutex<crate::ccos::intent_graph::IntentGraph>>,
    ) -> Result<Self, RuntimeError> {
        // Create LLM provider
        let llm_provider = LlmProviderFactory::create_provider(llm_config.to_provider_config()).await?;
        
        // Load intent patterns and plan templates from configuration
        let intent_patterns = template_config.intent_patterns.clone();
        let plan_templates = template_config.plan_templates.clone();
        let fallback_behavior = template_config.fallback.clone();
        
        Ok(Self {
            template_config,
            llm_config,
            intent_patterns,
            plan_templates,
            llm_provider,
            intent_graph,
            fallback_behavior,
        })
    }

    /// Match natural language input against intent patterns
    fn match_intent_pattern(&self, natural_language: &str) -> Option<&IntentPattern> {
        let lower_nl = natural_language.to_lowercase();
        
        for pattern in &self.intent_patterns {
            // Check regex pattern
            if let Ok(regex) = Regex::new(&pattern.pattern) {
                if regex.is_match(&lower_nl) {
                    return Some(pattern);
                }
            }
        }
        
        None
    }

    /// Find a plan template that matches the given intent
    fn find_plan_template(&self, intent_name: &str) -> Option<&PlanTemplate> {
        for template in &self.plan_templates {
            if template.variables.contains(&intent_name.to_string()) {
                return Some(template);
            }
        }
        
        None
    }

    /// Generate intent from pattern match (template-based)
    fn generate_intent_from_pattern(
        &self,
        pattern: &IntentPattern,
        natural_language: &str,
        context: Option<HashMap<String, Value>>,
    ) -> Intent {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        // Convert context to string format for template substitution
        let context_str = context.map(|ctx| {
            ctx.into_iter()
                .map(|(k, v)| (k, format!("{}", v)))
                .collect::<HashMap<String, String>>()
        });
        
        // Apply template substitution to goal
        let mut goal = pattern.goal_template.clone();
        if let Some(ctx) = &context_str {
            for (key, value) in ctx {
                goal = goal.replace(&format!("{{{}}}", key), value);
            }
        }
        
        Intent {
            intent_id: format!("hybrid_template_intent_{}", uuid::Uuid::new_v4()),
            name: Some(pattern.intent_name.clone()),
            original_request: natural_language.to_string(),
            goal,
            constraints: {
                let mut map = HashMap::new();
                for constraint in &pattern.constraints {
                    map.insert(constraint.clone(), Value::String(constraint.clone()));
                }
                map
            },
            preferences: {
                let mut map = HashMap::new();
                for preference in &pattern.preferences {
                    map.insert(preference.clone(), Value::String(preference.clone()));
                }
                map
            },
            success_criteria: None,
            status: IntentStatus::Active,
            created_at: now,
            updated_at: now,
            metadata: {
                let mut meta = HashMap::new();
                meta.insert(generation::GENERATION_METHOD.to_string(), Value::String(generation::methods::TEMPLATE.to_string()));
                meta.insert("pattern_name".to_string(), Value::String(pattern.name.clone()));
                meta
            },
        }
    }

    /// Generate plan from template (template-based)
    fn generate_plan_from_template(
        &self,
        template: &PlanTemplate,
        intent: &Intent,
        context: Option<HashMap<String, Value>>,
    ) -> Plan {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        // Convert context to string format for template substitution
        let context_str = context.map(|ctx| {
            ctx.into_iter()
                .map(|(k, v)| (k, format!("{}", v)))
                .collect::<HashMap<String, String>>()
        });
        
        // Apply template substitution to RTFS content
        let mut rtfs_content = template.rtfs_template.clone();
        
        // Substitute intent variables
        rtfs_content = rtfs_content.replace("{intent_id}", &intent.intent_id);
        rtfs_content = rtfs_content.replace("{intent_name}", intent.name.as_ref().unwrap_or(&"".to_string()));
        rtfs_content = rtfs_content.replace("{goal}", &intent.goal);
        
        // Substitute context variables
        if let Some(ctx) = &context_str {
            for (key, value) in ctx {
                rtfs_content = rtfs_content.replace(&format!("{{{}}}", key), value);
            }
        }
        
        Plan {
            plan_id: format!("hybrid_template_plan_{}", uuid::Uuid::new_v4()),
            name: Some(template.name.clone()),
            intent_ids: vec![intent.intent_id.clone()],
            language: PlanLanguage::Rtfs20,
            body: PlanBody::Rtfs(rtfs_content),
            status: PlanStatus::Draft,
            created_at: now,
            metadata: {
                let mut meta = HashMap::new();
                meta.insert(generation::GENERATION_METHOD.to_string(), Value::String(generation::methods::TEMPLATE.to_string()));
                meta.insert("template_name".to_string(), Value::String(template.name.clone()));
                meta
            },
            input_schema: None,
            output_schema: None,
            policies: HashMap::new(),
            capabilities_required: vec![],
            annotations: HashMap::new(),
        }
    }

    /// Generate intent using LLM fallback
    async fn generate_intent_with_llm(
        &self,
        natural_language: &str,
        context: Option<HashMap<String, Value>>,
    ) -> Result<Intent, RuntimeError> {
        let prompt = self.create_intent_prompt(natural_language, context.clone());
        
        let response = self.llm_provider.generate_text(&prompt).await?;
        
        // Parse LLM response into intent structure
        let intent = self.parse_llm_intent_response(&response, natural_language, context)?;
        
        Ok(intent)
    }

    /// Generate plan using LLM fallback
    async fn generate_plan_with_llm(
        &self,
        intent: &Intent,
        context: Option<HashMap<String, Value>>,
    ) -> Result<Plan, RuntimeError> {
        let prompt = self.create_plan_prompt(intent, context.clone());
        
        let response = self.llm_provider.generate_text(&prompt).await?;
        
        // Parse LLM response into plan structure
        let plan = self.parse_llm_plan_response(&response, intent)?;
        
        Ok(plan)
    }

    /// Create prompt for intent generation (centralized, versioned)
    fn create_intent_prompt(&self, natural_language: &str, context: Option<HashMap<String, Value>>) -> String {
        let prompt_cfg: PromptConfig = PromptConfig::default();
        let store = FilePromptStore::new("assets/prompts/arbiter");
        let manager = PromptManager::new(store);
        let mut vars = std::collections::HashMap::new();
        vars.insert("natural_language".to_string(), natural_language.to_string());
        vars.insert("context".to_string(), format!("{:?}", context));
        vars.insert("available_capabilities".to_string(), ":ccos.echo, :ccos.math.add".to_string());
        manager
            .render(&prompt_cfg.intent_prompt_id, &prompt_cfg.intent_prompt_version, &vars)
            .unwrap_or_else(|_| "".to_string())
    }

    /// Create prompt for plan generation
    fn create_plan_prompt(&self, intent: &Intent, context: Option<HashMap<String, Value>>) -> String {
        format!(
            r#"Generate an RTFS plan to achieve this intent:

Intent: {:?}

Context: {:?}

Generate a plan using RTFS syntax with step special forms:
(do
  (step "Step Name" (call :capability.name args))
  ...
)

IMPORTANT: For data sharing between plans in intent graphs:
- Use (set! :key value) to publish values that other plans can access
- Use (get :key) to retrieve values published by other plans
- Values set with set! are automatically shared across the intent graph
- Use meaningful key names like :result, :sum, :greeting, etc.

Examples:
; Producer plan - publishes a value
(do
  (step "compute-sum" 
    (set! :sum (+ 2 3))))

; Consumer plan - retrieves the value
(do
  (step "display-result"
    (let [s (get :sum)]
      (call :ccos.echo (str "The sum is: " s)))))

Rejection Checklist - DO NOT use:
❌ :step_1.result (deprecated syntax)
❌ Custom context capabilities (not needed)
❌ Complex data structures in set! (keep it simple)
❌ Unregistered capabilities (only use :ccos.echo, :ccos.math.add)
❌ String keys in set! - use symbols like :key not "key"

Available capabilities: :ccos.echo, :ccos.math.add

IMPORTANT: Use correct calling conventions:
- :ccos.echo - for printing/logging, e.g., (call :ccos.echo "message")
- :ccos.math.add - for adding numbers, e.g., (call :ccos.math.add {{:args [5 7]}})
(set! :result (call :ccos.math.add {{:args [10 20]}}))

Examples of correct usage:
(call :ccos.echo "Hello world")
(call :ccos.math.add {{:args [5 7]}})
(set! :result (call :ccos.math.add {{:args [10 20]}}))

Plan:"#,
            intent,
            context.unwrap_or_default()
        )
    }

    /// Parse LLM response into intent structure using RTFS parser
    fn parse_llm_intent_response(
        &self,
        response: &str,
        natural_language: &str,
        _context: Option<HashMap<String, Value>>,
    ) -> Result<Intent, RuntimeError> {
        // Extract the first top-level `(intent …)` s-expression from the response
        let intent_block = extract_intent(response)
            .ok_or_else(|| RuntimeError::Generic("Could not locate a complete (intent …) block".to_string()))?;
        
        // Sanitize regex literals for parsing
        let sanitized = sanitize_regex_literals(&intent_block);
        
        // Parse using RTFS parser
        let ast_items = crate::parser::parse(&sanitized)
            .map_err(|e| RuntimeError::Generic(format!("Failed to parse RTFS intent: {:?}", e)))?;
        
        // Find the first expression and convert to Intent
        if let Some(TopLevel::Expression(expr)) = ast_items.get(0) {
            intent_from_function_call(expr)
                .ok_or_else(|| RuntimeError::Generic("Parsed AST expression was not a valid intent definition".to_string()))
        } else {
            Err(RuntimeError::Generic("Parsed AST did not contain a top-level expression for the intent".to_string()))
        }
    }

    /// Parse LLM response into plan structure
    fn parse_llm_plan_response(
        &self,
        response: &str,
        intent: &Intent,
    ) -> Result<Plan, RuntimeError> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        // Extract RTFS content from response
        let rtfs_content = self.extract_rtfs_from_response(response)?;
        
        Ok(Plan {
            plan_id: format!("hybrid_llm_plan_{}", uuid::Uuid::new_v4()),
            name: Some(format!("llm_generated_plan_{}", intent.name.as_ref().unwrap_or(&"unknown".to_string()))),
            intent_ids: vec![intent.intent_id.clone()],
            language: PlanLanguage::Rtfs20,
            body: PlanBody::Rtfs(rtfs_content),
            status: PlanStatus::Draft,
            created_at: now,
            metadata: {
                let mut meta = HashMap::new();
                meta.insert(generation::GENERATION_METHOD.to_string(), Value::String(generation::methods::LLM.to_string()));
                meta.insert("llm_provider".to_string(), Value::String(format!("{:?}", self.llm_config.provider_type)));
                meta
            },
            input_schema: None,
            output_schema: None,
            policies: HashMap::new(),
            capabilities_required: vec![],
            annotations: HashMap::new(),
        })
    }

    /// Extract RTFS content from LLM response
    fn extract_rtfs_from_response(&self, response: &str) -> Result<String, RuntimeError> {
        // Look for RTFS content between parentheses
        if let Some(start) = response.find('(') {
            if let Some(end) = response.rfind(')') {
                let rtfs_content = response[start..=end].trim();
                if rtfs_content.starts_with('(') && rtfs_content.ends_with(')') {
                    return Ok(rtfs_content.to_string());
                }
            }
        }
        
        // If no parentheses found, try to extract from code blocks
        if let Some(start) = response.find("```rtfs") {
            if let Some(end) = response.find("```") {
                let content = response[start + 7..end].trim();
                return Ok(content.to_string());
            }
        }
        
        // Fallback: return the entire response
        Ok(response.trim().to_string())
    }

    /// Store intent in the intent graph
    async fn store_intent(&self, intent: &Intent) -> Result<(), RuntimeError> {
        let mut graph = self.intent_graph.lock()
            .map_err(|_| RuntimeError::Generic("Failed to lock intent graph".to_string()))?;
        
        // Convert to storable intent
        let storable = StorableIntent {
            intent_id: intent.intent_id.clone(),
            name: intent.name.clone(),
            original_request: intent.original_request.clone(),
            rtfs_intent_source: "hybrid_generated".to_string(),
            goal: intent.goal.clone(),
            constraints: intent.constraints.iter()
                .map(|(k, v)| (k.clone(), format!("{}", v)))
                .collect(),
            preferences: intent.preferences.iter()
                .map(|(k, v)| (k.clone(), format!("{}", v)))
                .collect(),
            success_criteria: intent.success_criteria.as_ref()
                .map(|v| format!("{}", v)),
            parent_intent: None,
            child_intents: vec![],
            triggered_by: crate::ccos::types::TriggerSource::HumanRequest,
            generation_context: crate::ccos::types::GenerationContext {
                arbiter_version: "1.0.0".to_string(),
                generation_timestamp: intent.created_at,
                input_context: HashMap::new(),
                reasoning_trace: Some("Hybrid template/LLM generation".to_string()),
            },
            status: intent.status.clone(),
            priority: 1,
            created_at: intent.created_at,
            updated_at: intent.updated_at,
            metadata: HashMap::new(),
        };
        
        graph.storage.store_intent(storable).await
            .map_err(|e| RuntimeError::Generic(format!("Failed to store intent: {}", e)))?;
        
        Ok(())
    }
}

#[async_trait(?Send)]
impl ArbiterEngine for HybridArbiter {
    async fn natural_language_to_intent(
        &self,
        natural_language: &str,
        context: Option<HashMap<String, Value>>,
    ) -> Result<Intent, RuntimeError> {
        // First, try template-based matching
        if let Some(pattern) = self.match_intent_pattern(natural_language) {
            let intent = self.generate_intent_from_pattern(pattern, natural_language, context.clone());
            
            // Store the intent
            self.store_intent(&intent).await?;
            
            return Ok(intent);
        }
        
        // Template matching failed, use LLM fallback based on configuration
        match self.fallback_behavior {
            FallbackBehavior::Llm => {
                let mut intent = self.generate_intent_with_llm(natural_language, context).await?;

                // Mark this as LLM-generated for tests and downstream code
                intent.metadata.insert(generation::GENERATION_METHOD.to_string(), Value::String(generation::methods::LLM.to_string()));

                // Store the intent
                self.store_intent(&intent).await?;

                Ok(intent)
            }
            FallbackBehavior::Default => {
                // Use default template
                if let Some(default_pattern) = self.intent_patterns.first() {
                    let intent = self.generate_intent_from_pattern(default_pattern, natural_language, context.clone());
                    
                    // Store the intent
                    self.store_intent(&intent).await?;
                    
                    Ok(intent)
                } else {
                    Err(RuntimeError::Generic("No default template available".to_string()))
                }
            }
            FallbackBehavior::Error => {
                Err(RuntimeError::Generic(format!(
                    "No template pattern found for request: '{}' and LLM fallback disabled",
                    natural_language
                )))
            }
        }
    }

    async fn intent_to_plan(
        &self,
        intent: &Intent,
    ) -> Result<Plan, RuntimeError> {
        let intent_name = intent.name.as_ref()
            .ok_or_else(|| RuntimeError::Generic("Intent has no name".to_string()))?;
        
        // First, try to find a matching plan template
        if let Some(template) = self.find_plan_template(intent_name) {
            return Ok(self.generate_plan_from_template(template, intent, None));
        }
        
        // Template matching failed, use LLM fallback based on configuration
        match self.fallback_behavior {
            FallbackBehavior::Llm => {
                self.generate_plan_with_llm(intent, None).await
            }
            FallbackBehavior::Default => {
                // Use default template
                if let Some(default_template) = self.plan_templates.first() {
                    Ok(self.generate_plan_from_template(default_template, intent, None))
                } else {
                    Err(RuntimeError::Generic("No default plan template available".to_string()))
                }
            }
            FallbackBehavior::Error => {
                Err(RuntimeError::Generic(format!(
                    "No plan template found for intent: '{}' and LLM fallback disabled",
                    intent_name
                )))
            }
        }
    }

    async fn execute_plan(
        &self,
        plan: &Plan,
    ) -> Result<ExecutionResult, RuntimeError> {
        // For hybrid arbiter, we return a placeholder execution result
        // In a real implementation, this would execute the RTFS plan
        Ok(ExecutionResult {
            success: true,
            value: Value::String("Hybrid arbiter execution placeholder".to_string()),
            metadata: {
                let mut meta = HashMap::new();
                meta.insert("plan_id".to_string(), Value::String(plan.plan_id.clone()));
                meta.insert("hybrid_engine".to_string(), Value::String("hybrid".to_string()));
                if let Some(generation_method) = plan.metadata.get(generation::GENERATION_METHOD) {
                    meta.insert(generation::GENERATION_METHOD.to_string(), generation_method.clone());
                }
                meta
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ccos::arbiter::arbiter_config::{TemplateConfig, IntentPattern, PlanTemplate, LlmConfig, LlmProviderType};

    fn create_test_config() -> (TemplateConfig, LlmConfig) {
        let template_config = TemplateConfig {
            intent_patterns: vec![
                IntentPattern {
                    name: "sentiment_analysis".to_string(),
                    pattern: r"(?i)analyze.*sentiment|sentiment.*analysis".to_string(),
                    intent_name: "analyze_sentiment".to_string(),
                    goal_template: "Analyze user sentiment from {source}".to_string(),
                    constraints: vec!["accuracy".to_string()],
                    preferences: vec!["speed".to_string()],
                },
                IntentPattern {
                    name: "backup_operation".to_string(),
                    pattern: r"(?i)backup|save.*data|protect.*data".to_string(),
                    intent_name: "backup_data".to_string(),
                    goal_template: "Create backup of {data_type} data".to_string(),
                    constraints: vec!["encryption".to_string()],
                    preferences: vec!["compression".to_string()],
                },
            ],
            plan_templates: vec![
                PlanTemplate {
                    name: "sentiment_analysis_plan".to_string(),
                    rtfs_template: r#"
(do
    (step "Fetch Data" (call :ccos.echo "fetching {source} data"))
    (step "Analyze Sentiment" (call :ccos.echo "analyzing sentiment"))
    (step "Generate Report" (call :ccos.echo "generating sentiment report"))
)
                    "#.trim().to_string(),
                    variables: vec!["analyze_sentiment".to_string(), "source".to_string()],
                },
                PlanTemplate {
                    name: "backup_plan".to_string(),
                    rtfs_template: r#"
(do
    (step "Validate Data" (call :ccos.echo "validating {data_type} data"))
    (step "Create Backup" (call :ccos.echo "creating encrypted backup"))
    (step "Verify Backup" (call :ccos.echo "verifying backup integrity"))
)
                    "#.trim().to_string(),
                    variables: vec!["backup_data".to_string(), "data_type".to_string()],
                },
            ],
            fallback: FallbackBehavior::Llm,
        };

        let llm_config = LlmConfig {
            provider_type: LlmProviderType::Stub,
            model: "stub-model".to_string(),
            api_key: None,
            base_url: None,
            max_tokens: Some(1000),
            temperature: Some(0.7),
            timeout_seconds: Some(30),
            prompts: None,
        };

        (template_config, llm_config)
    }

    #[tokio::test]
    async fn test_hybrid_arbiter_creation() {
        let (template_config, llm_config) = create_test_config();
        let intent_graph = std::sync::Arc::new(std::sync::Mutex::new(
            crate::ccos::intent_graph::IntentGraph::new().unwrap()
        ));
        
        let arbiter = HybridArbiter::new(template_config, llm_config, intent_graph).await;
        assert!(arbiter.is_ok());
    }

    #[tokio::test]
    async fn test_template_fallback() {
        let (template_config, llm_config) = create_test_config();
        let intent_graph = std::sync::Arc::new(std::sync::Mutex::new(
            crate::ccos::intent_graph::IntentGraph::new().unwrap()
        ));
        
        let arbiter = HybridArbiter::new(template_config, llm_config, intent_graph).await.unwrap();
        
        // Test template matching
        let intent = arbiter.natural_language_to_intent(
            "analyze user sentiment from chat logs",
            None
        ).await.unwrap();
        
        assert!(intent.name.is_some() && intent.name.as_ref().unwrap().contains("sentiment"));
        if let Some(v) = intent.metadata.get(generation::GENERATION_METHOD) {
            if let Some(s) = v.as_string() {
                assert!(s.to_lowercase().contains("template") || s.to_lowercase().contains("tmpl"));
            } else {
                panic!("generation_method metadata is not a string");
            }
        } else {
            assert!(intent.name.is_some() || !intent.original_request.is_empty());
        }
    }

    #[tokio::test]
    async fn test_llm_fallback() {
        let (mut template_config, llm_config) = create_test_config();
        template_config.fallback = FallbackBehavior::Llm;
        
        let intent_graph = std::sync::Arc::new(std::sync::Mutex::new(
            crate::ccos::intent_graph::IntentGraph::new().unwrap()
        ));
        
        let arbiter = HybridArbiter::new(template_config, llm_config, intent_graph).await.unwrap();
        
        // Test LLM fallback for unknown request
        let intent = arbiter.natural_language_to_intent(
            "random unknown request",
            None
        ).await.unwrap();
        
        if let Some(v) = intent.metadata.get(generation::GENERATION_METHOD) {
            if let Some(s) = v.as_string() {
                assert!(s.to_lowercase().contains("llm") || s.to_lowercase().contains("language"));
            } else {
                panic!("generation_method metadata is not a string");
            }
        } else {
            assert!(intent.name.is_some() || !intent.original_request.is_empty());
        }
    }
}
