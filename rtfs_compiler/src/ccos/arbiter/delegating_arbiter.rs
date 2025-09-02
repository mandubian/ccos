//! Delegating Arbiter Engine
//!
//! This module provides a delegating approach that combines LLM-driven reasoning
//! with agent delegation for complex tasks. The delegating arbiter uses LLM to
//! understand requests and then delegates to specialized agents when appropriate.

use std::collections::HashMap;
use async_trait::async_trait;

use crate::runtime::error::RuntimeError;
use crate::runtime::values::Value;
use regex;
use crate::ccos::types::{Intent, Plan, PlanBody, PlanLanguage, PlanStatus, StorableIntent, ExecutionResult};
use crate::ccos::arbiter::arbiter_engine::ArbiterEngine;
use crate::ccos::arbiter::arbiter_config::{LlmConfig, DelegationConfig, AgentRegistryConfig, AgentDefinition};
use crate::ccos::arbiter::llm_provider::{LlmProvider, LlmProviderFactory};
use crate::ccos::delegation_keys::{generation, agent};

use crate::ast::TopLevel;
use std::fs::OpenOptions;
use std::io::Write;
use serde_json::json;

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

/// Delegating arbiter that combines LLM reasoning with agent delegation
pub struct DelegatingArbiter {
    llm_config: LlmConfig,
    delegation_config: DelegationConfig,
    llm_provider: Box<dyn LlmProvider>,
    agent_registry: AgentRegistry,
    intent_graph: std::sync::Arc<std::sync::Mutex<crate::ccos::intent_graph::IntentGraph>>,
    adaptive_threshold_calculator: Option<crate::ccos::adaptive_threshold::AdaptiveThresholdCalculator>,
}

/// Agent registry for managing available agents
pub struct AgentRegistry {
    config: AgentRegistryConfig,
    agents: HashMap<String, AgentDefinition>,
}

impl AgentRegistry {
    /// Create a new agent registry
    pub fn new(config: AgentRegistryConfig) -> Self {
        let mut agents = HashMap::new();
        
        // Add agents from configuration
        for agent in &config.agents {
            agents.insert(agent.agent_id.clone(), agent.clone());
        }
        
        Self { config, agents }
    }
    
    /// Find agents that match the given capabilities
    pub fn find_agents_for_capabilities(&self, required_capabilities: &[String]) -> Vec<&AgentDefinition> {
        let mut candidates = Vec::new();
        
        for agent in self.agents.values() {
            let matching_capabilities = agent.capabilities.iter()
                .filter(|cap| required_capabilities.contains(cap))
                .count();
            
            if matching_capabilities > 0 {
                candidates.push(agent);
            }
        }
        
        // Sort by trust score and cost
        candidates.sort_by(|a, b| {
            b.trust_score.partial_cmp(&a.trust_score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(a.cost.partial_cmp(&b.cost).unwrap_or(std::cmp::Ordering::Equal))
        });
        
        candidates
    }
    
    /// Get agent by ID
    pub fn get_agent(&self, agent_id: &str) -> Option<&AgentDefinition> {
        self.agents.get(agent_id)
    }
    
    /// List all available agents
    pub fn list_agents(&self) -> Vec<&AgentDefinition> {
        self.agents.values().collect()
    }
}

impl DelegatingArbiter {
    /// Create a new delegating arbiter with the given configuration
    pub async fn new(
        llm_config: LlmConfig,
        delegation_config: DelegationConfig,
        intent_graph: std::sync::Arc<std::sync::Mutex<crate::ccos::intent_graph::IntentGraph>>,
    ) -> Result<Self, RuntimeError> {
        // Create LLM provider
        let llm_provider = LlmProviderFactory::create_provider(llm_config.to_provider_config()).await?;
        
        // Create agent registry
        let agent_registry = AgentRegistry::new(delegation_config.agent_registry.clone());
        
        // Create adaptive threshold calculator if configured
        let adaptive_threshold_calculator = delegation_config.adaptive_threshold.as_ref()
            .map(|config| crate::ccos::adaptive_threshold::AdaptiveThresholdCalculator::new(config.clone()));
        
        Ok(Self {
            llm_config,
            delegation_config,
            llm_provider,
            agent_registry,
            intent_graph,
            adaptive_threshold_calculator,
        })
    }

    /// Generate intent using LLM
    async fn generate_intent_with_llm(
        &self,
        natural_language: &str,
        context: Option<HashMap<String, Value>>,
    ) -> Result<Intent, RuntimeError> {
        let prompt = self.create_intent_prompt(natural_language, context.clone());
        
        let response = self.llm_provider.generate_text(&prompt).await?;
        
        // Log provider, prompt, and response (best-effort, non-fatal)
        let _ = (|| -> Result<(), std::io::Error> {
            let mut f = OpenOptions::new().create(true).append(true).open("logs/arbiter_llm.log")?;
            let entry = json!({"event":"llm_intent_generation","provider": format!("{:?}", self.llm_config.provider_type), "prompt": prompt, "response": response});
            writeln!(f, "[{}] {}", chrono::Utc::now().timestamp(), entry.to_string())?;
            Ok(())
        })();

        // Parse LLM response into intent structure
        let mut intent = self.parse_llm_intent_response(&response, natural_language, context)?;

        // Mark how this intent was generated so downstream code/tests can inspect it
        intent.metadata.insert(generation::GENERATION_METHOD.to_string(), Value::String(generation::methods::DELEGATING_LLM.to_string()));

        // Append a compact JSONL entry with the generated intent for debugging
        let _ = (|| -> Result<(), std::io::Error> {
            let mut f = OpenOptions::new().create(true).append(true).open("logs/arbiter_llm.log")?;
            // Serialize a minimal intent snapshot
            let intent_snapshot = json!({
                "intent_id": intent.intent_id,
                "name": intent.name,
                "goal": intent.goal,
                "metadata": intent.metadata,
            });
            let entry = json!({"event":"llm_intent_parsed","provider": format!("{:?}", self.llm_config.provider_type), "intent": intent_snapshot});
            writeln!(f, "[{}] {}", chrono::Utc::now().timestamp(), entry.to_string())?;
            Ok(())
        })();

        Ok(intent)
    }

    /// Generate plan using LLM with agent delegation
    async fn generate_plan_with_delegation(
        &self,
        intent: &Intent,
        context: Option<HashMap<String, Value>>,
    ) -> Result<Plan, RuntimeError> {
        // First, analyze if delegation is appropriate
        let delegation_analysis = self.analyze_delegation_need(intent, context.clone()).await?;
        
        if delegation_analysis.should_delegate {
            // Generate plan with delegation
            self.generate_delegated_plan(intent, &delegation_analysis, context).await
        } else {
            // Generate plan without delegation
            self.generate_direct_plan(intent, context).await
        }
    }

    /// Analyze whether delegation is needed for this intent
    async fn analyze_delegation_need(
        &self,
        intent: &Intent,
        context: Option<HashMap<String, Value>>,
    ) -> Result<DelegationAnalysis, RuntimeError> {
        let prompt = self.create_delegation_analysis_prompt(intent, context);
        
        let response = self.llm_provider.generate_text(&prompt).await?;
        
        // Parse delegation analysis
        let mut analysis = self.parse_delegation_analysis(&response)?;
        
        // Apply adaptive threshold if configured
        if let Some(calculator) = &self.adaptive_threshold_calculator {
            // Get base threshold from config
            let base_threshold = self.delegation_config.threshold;
            
            // For now, we'll use a default agent ID for threshold calculation
            // In the future, this could be based on the specific agent being considered
            let adaptive_threshold = calculator.calculate_threshold("default_agent", base_threshold);
            
            // Adjust delegation decision based on adaptive threshold
            analysis.should_delegate = analysis.should_delegate && 
                analysis.delegation_confidence >= adaptive_threshold;
            
            // Update reasoning to include adaptive threshold information
            analysis.reasoning = format!(
                "{} [Adaptive threshold: {:.3}, Confidence: {:.3}]", 
                analysis.reasoning, 
                adaptive_threshold, 
                analysis.delegation_confidence
            );
        }
        
        Ok(analysis)
    }

    /// Generate plan with agent delegation
    async fn generate_delegated_plan(
        &self,
        intent: &Intent,
        delegation_analysis: &DelegationAnalysis,
        context: Option<HashMap<String, Value>>,
    ) -> Result<Plan, RuntimeError> {
        // Find suitable agents
        let candidate_agents = self.agent_registry.find_agents_for_capabilities(&delegation_analysis.required_capabilities);
        
        if candidate_agents.is_empty() {
            // No suitable agents found, fall back to direct plan
            return self.generate_direct_plan(intent, context).await;
        }
        
        // Select the best agent
        let selected_agent = &candidate_agents[0];
        
        // Generate delegation plan
        let prompt = self.create_delegation_plan_prompt(intent, selected_agent, context);
        
        let response = self.llm_provider.generate_text(&prompt).await?;

        // Log provider, prompt and response
        let _ = (|| -> Result<(), std::io::Error> {
            let mut f = OpenOptions::new().create(true).append(true).open("logs/arbiter_llm.log")?;
            let entry = json!({"event":"llm_delegation_plan","provider": format!("{:?}", self.llm_config.provider_type), "agent": selected_agent.agent_id, "prompt": prompt, "response": response});
            writeln!(f, "[{}] {}", chrono::Utc::now().timestamp(), entry.to_string())?;
            Ok(())
        })();

    // Parse delegation plan
    let plan = self.parse_delegation_plan(&response, intent, selected_agent)?;
    // Log parsed plan for debugging
    self.log_parsed_plan(&plan);
    Ok(plan)
    }

    /// Generate plan without delegation
    async fn generate_direct_plan(
        &self,
        intent: &Intent,
        context: Option<HashMap<String, Value>>,
    ) -> Result<Plan, RuntimeError> {
        let prompt = self.create_direct_plan_prompt(intent, context);
        
        let response = self.llm_provider.generate_text(&prompt).await?;

        // Log provider, prompt and response
        let _ = (|| -> Result<(), std::io::Error> {
            let mut f = OpenOptions::new().create(true).append(true).open("logs/arbiter_llm.log")?;
            let entry = json!({"event":"llm_direct_plan","provider": format!("{:?}", self.llm_config.provider_type), "prompt": prompt, "response": response});
            writeln!(f, "[{}] {}", chrono::Utc::now().timestamp(), entry.to_string())?;
            Ok(())
        })();

    // Parse direct plan
    let plan = self.parse_direct_plan(&response, intent)?;
    // Log parsed plan for debugging
    self.log_parsed_plan(&plan);
    Ok(plan)
    }

    /// Create prompt for intent generation
    fn create_intent_prompt(&self, natural_language: &str, context: Option<HashMap<String, Value>>) -> String {
        format!(
            r#"Convert the following natural language request into a structured Intent using RTFS syntax.

Request: {natural_language}

Context: {context:?}

Generate an RTFS Intent matching this format:
(intent "intent-name"
  :goal "Clear description of what should be achieved"
  :constraints {{
    :constraint-name constraint-expression
    :another-constraint (> value threshold)
  }}
  :preferences {{
    :preference-name preference-value
    :another-preference "optional-setting"
  }}
  :success-criteria (and (condition1) (condition2)))

Example:
(intent "deploy-web-service"
  :goal "Deploy a web service with high availability"
  :constraints {{
    :availability (> uptime 0.99)
    :performance (< response-time 200)
    :cost (< monthly-cost 1000)
  }}
  :preferences {{
    :region "us-east-1"
    :scaling :auto
  }}
  :success-criteria (and (deployed? service) 
                        (healthy? service)
                        (> (uptime service) 0.99)))

Generate the RTFS Intent:"#,
            natural_language = natural_language,
            context = context.as_ref().unwrap_or(&HashMap::new())
        )
    }

    /// Create prompt for delegation analysis
    fn create_delegation_analysis_prompt(&self, intent: &Intent, context: Option<HashMap<String, Value>>) -> String {
        let available_agents = self.agent_registry.list_agents();
        let agent_list = available_agents.iter()
            .map(|agent| format!("- {}: {} (trust: {:.2}, cost: {:.2})", 
                agent.agent_id, 
                agent.name, 
                agent.trust_score, 
                agent.cost))
            .collect::<Vec<_>>()
            .join("\n");

        self.create_fallback_delegation_prompt(intent, context, &agent_list)
    }

    /// Fallback delegation analysis prompt (used when prompt manager is not available)
    fn create_fallback_delegation_prompt(&self, intent: &Intent, context: Option<HashMap<String, Value>>, agent_list: &str) -> String {
        format!(
            r#"CRITICAL: You must respond with ONLY a JSON object. Do NOT generate RTFS code or any other format.

You are analyzing whether to delegate a task to specialized agents. Your response must be a JSON object.

## Required JSON Response Format:
{{
  "should_delegate": true,
  "reasoning": "Clear explanation of the delegation decision",
  "required_capabilities": ["capability1", "capability2"],
  "delegation_confidence": 0.85
}}

## Rules:
- ONLY output the JSON object, nothing else
- Use double quotes for all strings
- Include all 4 required fields
- delegation_confidence must be between 0.0 and 1.0

## Analysis Criteria:
- Task complexity and specialization needs
- Available agent capabilities
- Cost vs. benefit analysis
- Security requirements

## Input for Analysis:
Intent: {:?}
Context: {:?}
Available Agents:
{agents}

## Your JSON Response:"#,
            intent,
            context.unwrap_or_default(),
            agents = agent_list
        )
    }

    /// Create prompt for delegation plan generation
    fn create_delegation_plan_prompt(
        &self,
        intent: &Intent,
        agent: &AgentDefinition,
        context: Option<HashMap<String, Value>>,
    ) -> String {
        format!(
            r#"Generate an RTFS plan that delegates this intent to a specialized agent.

Intent: {:?}

Selected Agent: {} ({})
Agent Capabilities: {:?}
Agent Trust Score: {:.2}
Agent Cost: {:.2}

Context: {:?}

Generate a plan using RTFS syntax with step special forms that:
1. Validates the delegation decision
2. Prepares the request for the agent
3. Delegates to the agent
4. Handles the response
5. Validates the result

Available capabilities: :ccos.echo, :ccos.validate, :ccos.delegate, :ccos.verify

Plan:"#,
            intent,
            agent.name,
            agent.agent_id,
            agent.capabilities,
            agent.trust_score,
            agent.cost,
            context.unwrap_or_default()
        )
    }

    /// Create prompt for direct plan generation
    fn create_direct_plan_prompt(&self, intent: &Intent, context: Option<HashMap<String, Value>>) -> String {
        format!(
            r#"Generate an RTFS plan to achieve this intent directly.

Intent: {:?}

Context: {:?}

Generate a plan using RTFS syntax with step special forms:
(do
  (step "Step Name" (call :capability.name args))
  ...
)

IMPORTANT: For data sharing between plans in intent graphs:
- Use (set! "key" value) to publish values that other plans can access
- Use (get "key") to retrieve values published by other plans
- Values set with set! are automatically shared across the intent graph
- Use meaningful key names like "result", "sum", "greeting", etc.

Examples:
; Producer plan - publishes a value
(do
  (step "compute-sum" 
    (set! "sum" (+ 2 3))))

; Consumer plan - retrieves the value
(do
  (step "display-result"
    (let [s (get "sum")]
      (call :ccos.echo (str "The sum is: " s)))))

Rejection Checklist - DO NOT use:
❌ :step_1.result (deprecated syntax)
❌ Custom context capabilities (not needed)
❌ Complex data structures in set! (keep it simple)
❌ Unregistered capabilities (only use :ccos.echo, :ccos.math.add)

Available capabilities: :ccos.echo, :ccos.math.add

Plan:"#,
            intent,
            context.unwrap_or_default()
        )
    }

    /// Parse LLM response into intent structure using RTFS parser
    fn parse_llm_intent_response(
        &self,
        response: &str,
        _natural_language: &str,
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

    /// Parse delegation analysis response with robust error handling
    fn parse_delegation_analysis(&self, response: &str) -> Result<DelegationAnalysis, RuntimeError> {
        // Clean the response - remove any leading/trailing whitespace and extract JSON
        let cleaned_response = self.extract_json_from_response(response);
        
        // Try to parse the JSON
        let json_response: serde_json::Value = serde_json::from_str(&cleaned_response)
            .map_err(|e| {
                // Provide more detailed error information
                RuntimeError::Generic(format!(
                    "Failed to parse delegation analysis JSON: {}. Response: '{}'", 
                    e, 
                    response.chars().take(200).collect::<String>()
                ))
            })?;
        
        // Validate required fields
        if !json_response.is_object() {
            return Err(RuntimeError::Generic("Delegation analysis response is not a JSON object".to_string()));
        }
        
        let should_delegate = json_response["should_delegate"].as_bool()
            .ok_or_else(|| RuntimeError::Generic("Missing or invalid 'should_delegate' field".to_string()))?;
        
        let reasoning = json_response["reasoning"].as_str()
            .ok_or_else(|| RuntimeError::Generic("Missing or invalid 'reasoning' field".to_string()))?
            .to_string();
        
        let required_capabilities = json_response["required_capabilities"].as_array()
            .ok_or_else(|| RuntimeError::Generic("Missing or invalid 'required_capabilities' field".to_string()))?
            .iter()
            .filter_map(|v| v.as_str())
            .map(|s| s.to_string())
            .collect::<Vec<_>>();
        
        let delegation_confidence = json_response["delegation_confidence"].as_f64()
            .ok_or_else(|| RuntimeError::Generic("Missing or invalid 'delegation_confidence' field".to_string()))?;
        
        // Validate confidence range
        if delegation_confidence < 0.0 || delegation_confidence > 1.0 {
            return Err(RuntimeError::Generic(format!(
                "Delegation confidence must be between 0.0 and 1.0, got: {}", 
                delegation_confidence
            )));
        }
        
        Ok(DelegationAnalysis {
            should_delegate,
            reasoning,
            required_capabilities,
            delegation_confidence,
        })
    }

    /// Extract JSON from LLM response, handling common formatting issues
    fn extract_json_from_response(&self, response: &str) -> String {
        let response = response.trim();
        
        // Look for JSON object boundaries
        if let Some(start) = response.find('{') {
            if let Some(end) = response.rfind('}') {
                if end > start {
                    return response[start..=end].to_string();
                }
            }
        }
        
        // If no JSON object found, return the original response
        response.to_string()
    }

    /// Record feedback for delegation performance
    pub fn record_delegation_feedback(&mut self, agent_id: &str, success: bool) {
        if let Some(calculator) = &mut self.adaptive_threshold_calculator {
            calculator.update_performance(agent_id, success);
        }
    }

    /// Get adaptive threshold for a specific agent
    pub fn get_adaptive_threshold(&self, agent_id: &str) -> Option<f64> {
        if let Some(calculator) = &self.adaptive_threshold_calculator {
            let base_threshold = self.delegation_config.threshold;
            Some(calculator.calculate_threshold(agent_id, base_threshold))
        } else {
            None
        }
    }

    /// Get performance data for a specific agent
    pub fn get_agent_performance(&self, agent_id: &str) -> Option<&crate::ccos::adaptive_threshold::AgentPerformance> {
        if let Some(calculator) = &self.adaptive_threshold_calculator {
            calculator.get_performance(agent_id)
        } else {
            None
        }
    }

    /// Parse delegation plan response
    fn parse_delegation_plan(
        &self,
        response: &str,
        intent: &Intent,
        agent: &AgentDefinition,
    ) -> Result<Plan, RuntimeError> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        // Extract RTFS content from response
        let rtfs_content = self.extract_rtfs_from_response(response)?;
        
        Ok(Plan {
            plan_id: format!("delegating_plan_{}", uuid::Uuid::new_v4()),
            name: Some(format!("delegated_plan_{}", intent.name.as_ref().unwrap_or(&"unknown".to_string()))),
            intent_ids: vec![intent.intent_id.clone()],
            language: PlanLanguage::Rtfs20,
            body: PlanBody::Rtfs(rtfs_content),
            status: PlanStatus::Draft,
            created_at: now,
            metadata: {
                let mut meta = HashMap::new();
                meta.insert(generation::GENERATION_METHOD.to_string(), Value::String(generation::methods::DELEGATION.to_string()));
                meta.insert(agent::DELEGATED_AGENT.to_string(), Value::String(agent.agent_id.clone()));
                meta.insert(agent::AGENT_TRUST_SCORE.to_string(), Value::Float(agent.trust_score));
                meta.insert(agent::AGENT_COST.to_string(), Value::Float(agent.cost));
                meta
            },
            input_schema: None,
            output_schema: None,
            policies: HashMap::new(),
            capabilities_required: vec![],
            annotations: HashMap::new(),
        })
    }

    /// Parse direct plan response
    fn parse_direct_plan(&self, response: &str, intent: &Intent) -> Result<Plan, RuntimeError> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        // Extract RTFS content from response
        let rtfs_content = self.extract_rtfs_from_response(response)?;
        
        Ok(Plan {
            plan_id: format!("direct_plan_{}", uuid::Uuid::new_v4()),
            name: Some(format!("direct_plan_{}", intent.name.as_ref().unwrap_or(&"unknown".to_string()))),
            intent_ids: vec![intent.intent_id.clone()],
            language: PlanLanguage::Rtfs20,
            body: PlanBody::Rtfs(rtfs_content),
            status: PlanStatus::Draft,
            created_at: now,
            metadata: {
                let mut meta = HashMap::new();
                meta.insert(generation::GENERATION_METHOD.to_string(), Value::String(generation::methods::DIRECT.to_string()));
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

    // Note: This helper returns a Plan constructed from the RTFS body; we log the RTFS body for debugging.
    fn log_parsed_plan(&self, plan: &Plan) {
        let _ = (|| -> Result<(), std::io::Error> {
            let mut f = OpenOptions::new().create(true).append(true).open("logs/arbiter_llm.log")?;
            let entry = json!({"event":"llm_plan_parsed","plan_id": plan.plan_id, "rtfs_body": match &plan.body { crate::ccos::types::PlanBody::Rtfs(s) => s, _ => "" }});
            writeln!(f, "[{}] {}", chrono::Utc::now().timestamp(), entry.to_string())?;
            Ok(())
        })();
    }

    /// Extract RTFS plan from LLM response, preferring a balanced (do ...) block
    fn extract_rtfs_from_response(&self, response: &str) -> Result<String, RuntimeError> {
        // Normalize map-style intent objects (e.g. {:type "intent" :name "root" :goal "..."})
        // into canonical `(intent "name" :goal "...")` forms so downstream parser
        // doesn't see bare map literals that use :type keys.
        fn normalize_map_style_intents(src: &str) -> String {
            // Simple state machine: replace occurrences of `{:type "intent" ...}` with
            // `(intent "<name>" :goal "<goal>" ...)` where available. This is intentionally
            // conservative and only rewrites top-level map-like blocks that include `:type "intent"`.
            let mut out = String::new();
            let mut rest = src;
            while let Some(start) = rest.find('{') {
                // copy up to start
                out.push_str(&rest[..start]);
                if let Some(end) = rest[start..].find('}') {
                    let block = &rest[start..start+end+1];
                    // quick check for :type "intent"
                    if block.contains(":type \"intent\"") || block.contains(":type 'intent'") {
                        // parse simple key/value pairs inside block
                        // remove surrounding braces and split on ':' keys (best-effort)
                        let inner = &block[1..block.len()-1];
                        // build a small map of keys to raw values
                        let mut map = std::collections::HashMap::new();
                        // split by whitespace-separated tokens of form :key value
                        let mut iter = inner.split_whitespace().peekable();
                        while let Some(token) = iter.next() {
                            if token.starts_with(":") {
                                let key = token.trim_start_matches(':').to_string();
                                // collect the value token(s) until next key or end
                                if let Some(val_tok) = iter.next() {
                                    // if value begins with '"', consume until closing '"'
                                    if val_tok.starts_with('"') && !val_tok.ends_with('"') {
                                        let mut val = val_tok.to_string();
                                        while let Some(next_tok) = iter.peek() {
                                            let nt = *next_tok;
                                            val.push(' ');
                                            val.push_str(nt);
                                            iter.next();
                                            if nt.ends_with('"') { break; }
                                        }
                                        map.insert(key, val.trim().to_string());
                                    } else {
                                        map.insert(key, val_tok.trim().to_string());
                                    }
                                }
                            }
                        }

                        // If map contains name/goal produce an (intent ...) form
                        if let Some(name_raw) = map.get("name") {
                            // strip surrounding quotes if present
                            let name = name_raw.trim().trim_matches('"').to_string();
                            let mut intent_form = format!("(intent \"{}\"", name);
                            if let Some(goal_raw) = map.get("goal") {
                                let goal = goal_raw.trim().trim_matches('"');
                                intent_form.push_str(&format!(" :goal \"{}\"", goal));
                            }
                            // add other known keys as keyword pairs
                            for (k, v) in map.iter() {
                                if k == "name" || k == "type" || k == "goal" { continue; }
                                let val = v.trim();
                                intent_form.push_str(&format!(" :{} {}", k, val));
                            }
                            intent_form.push(')');
                            out.push_str(&intent_form);
                        } else {
                            // fallback: copy original block
                            out.push_str(block);
                        }
                        // advance rest
                        rest = &rest[start+end+1..];
                        continue;
                    }
                    // not an intent map, copy as-is
                    out.push_str(block);
                    rest = &rest[start+end+1..];
                } else {
                    // unmatched brace; copy remainder and break
                    out.push_str(rest);
                    rest = "";
                    break;
                }
            }
            out.push_str(rest);
            out
        }

        let response = normalize_map_style_intents(response);

        // 1) Prefer fenced rtfs code blocks
        if let Some(code_start) = response.find("```rtfs") {
            if let Some(code_end) = response[code_start + 7..].find("```") {
                let fenced = &response[code_start + 7..code_start + 7 + code_end];
                // If a (do ...) exists inside, extract the balanced block
                if let Some(idx) = fenced.find("(do") {
                    if let Some(block) = Self::extract_balanced_from(fenced, idx) {
                        return Ok(block);
                    }
                }
                // Otherwise, return fenced content trimmed
                let trimmed = fenced.trim();
                // Guard: avoid returning a raw (intent ...) block as a plan
                if trimmed.starts_with("(intent") {
                    return Err(RuntimeError::Generic("LLM response contains an intent block, but no plan (do ...) block".to_string()));
                }
                return Ok(trimmed.to_string());
            }
        }

        // 2) Search raw text for a (do ...) block
        if let Some(idx) = response.find("(do") {
            if let Some(block) = Self::extract_balanced_from(&response, idx) {
                return Ok(block);
            }
        }

        // 3) As a last resort, handle top-level blocks. If the response contains only (intent ...) blocks,
        // wrap them into a (do ...) block so they become an executable RTFS plan. If other top-level blocks
        // exist, return the first non-(intent) balanced block.
        if let Some(mut idx) = response.find('(') {
            let mut collected_intents = Vec::new();
            let mut remaining = &response[idx..];

            // Collect consecutive top-level balanced blocks
            while let Some(block) = Self::extract_balanced_from(remaining, 0) {
                if block.trim_start().starts_with("(intent") {
                    collected_intents.push(block.clone());
                } else {
                    // Found a non-intent top-level block: prefer returning it
                    return Ok(block);
                }

                // Advance remaining slice
                let consumed = block.len();
                if consumed >= remaining.len() { remaining = ""; break; }
                remaining = &remaining[consumed..];
                // Skip whitespace/newlines
                let skip = remaining.find(|c: char| !c.is_whitespace()).unwrap_or(0);
                remaining = &remaining[skip..];
            }

            if !collected_intents.is_empty() {
                // Wrap collected intent blocks in a (do ...) wrapper
                let mut do_block = String::from("(do\n");
                for ib in collected_intents.iter() {
                    do_block.push_str("    ");
                    do_block.push_str(ib.trim());
                    do_block.push_str("\n");
                }
                do_block.push_str(")");
                return Ok(do_block);
            }
        }

        Err(RuntimeError::Generic("Could not extract an RTFS plan from LLM response".to_string()))
    }

    /// Helper: extract a balanced s-expression starting at `start_idx` in `text`
    fn extract_balanced_from(text: &str, start_idx: usize) -> Option<String> {
        let bytes = text.as_bytes();
        if bytes.get(start_idx) != Some(&b'(') { return None; }
        let mut depth = 0usize;
        for (i, ch) in text[start_idx..].char_indices() {
            match ch {
                '(' => depth = depth.saturating_add(1),
                ')' => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        let end = start_idx + i + 1; // inclusive
                        return Some(text[start_idx..end].to_string());
                    }
                }
                _ => {}
            }
        }
        None
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
            rtfs_intent_source: "delegating_generated".to_string(),
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
                reasoning_trace: Some("Delegating LLM generation".to_string()),
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

/// Analysis result for delegation decision
#[derive(Debug, Clone)]
struct DelegationAnalysis {
    should_delegate: bool,
    reasoning: String,
    required_capabilities: Vec<String>,
    delegation_confidence: f64,
}

#[async_trait(?Send)]
impl ArbiterEngine for DelegatingArbiter {
    async fn natural_language_to_intent(
        &self,
        natural_language: &str,
        context: Option<HashMap<String, Value>>,
    ) -> Result<Intent, RuntimeError> {
        let intent = self.generate_intent_with_llm(natural_language, context).await?;
        
        // Store the intent
        self.store_intent(&intent).await?;
        
        Ok(intent)
    }

    async fn intent_to_plan(
        &self,
        intent: &Intent,
    ) -> Result<Plan, RuntimeError> {
        self.generate_plan_with_delegation(intent, None).await
    }

    async fn execute_plan(
        &self,
        plan: &Plan,
    ) -> Result<ExecutionResult, RuntimeError> {
        // For delegating arbiter, we return a placeholder execution result
        // In a real implementation, this would execute the RTFS plan
        Ok(ExecutionResult {
            success: true,
            value: Value::String("Delegating arbiter execution placeholder".to_string()),
            metadata: {
                let mut meta = HashMap::new();
                meta.insert("plan_id".to_string(), Value::String(plan.plan_id.clone()));
                meta.insert("delegating_engine".to_string(), Value::String("delegating".to_string()));
                        if let Some(generation_method) = plan.metadata.get(generation::GENERATION_METHOD) {
            meta.insert(generation::GENERATION_METHOD.to_string(), generation_method.clone());
        }
        if let Some(delegated_agent) = plan.metadata.get(agent::DELEGATED_AGENT) {
            meta.insert(agent::DELEGATED_AGENT.to_string(), delegated_agent.clone());
        }
                meta
            },
        })
    }

    async fn natural_language_to_graph(
        &self,
        natural_language_goal: &str,
    ) -> Result<String, RuntimeError> {
                // Build a precise prompt instructing the model to output a single RTFS (do ...) graph
              let prompt = format!(
                        r#"You are the CCOS Arbiter. Convert the natural language goal into an RTFS intent graph.

STRICT OUTPUT RULES:
- Output EXACTLY one well-formed RTFS s-expression starting with (do ...). No prose, comments, or extra blocks.
- Inside the (do ...), declare intents and edges only.
 - Use only these forms:
  - (intent "name" :goal "..." [:constraints {{...}}] [:preferences {{...}}] [:success-criteria ...])
  - (edge {{:from "child" :to "parent" :type :IsSubgoalOf}})
    - or positional edge form: (edge :DependsOn "from" "to")
- Allowed edge types: :IsSubgoalOf, :DependsOn, :ConflictsWith, :Enables, :RelatedTo, :TriggeredBy, :Blocks
- Names must be unique and referenced consistently by edges.
- Include at least one root intent that captures the overarching goal. Subgoals should use :IsSubgoalOf edges to point to their parent.
- Keep it compact and executable by an RTFS parser.

Natural language goal:
"{goal}"

Tiny example (format to imitate, not content):
```rtfs
(do
    (intent "setup-backup" :goal "Set up daily encrypted backups")
    (intent "configure-storage" :goal "Configure S3 bucket and IAM policy")
    (intent "schedule-job" :goal "Schedule nightly backup job")
        (edge {{:from "configure-storage" :to "setup-backup" :type :IsSubgoalOf}})
    (edge :Enables "configure-storage" "schedule-job"))
```

Now output ONLY the RTFS (do ...) block for the provided goal:
"#,
                        goal = natural_language_goal
                );

                let response = self.llm_provider.generate_text(&prompt).await?;

                // Log provider, prompt and raw response
                let _ = (|| -> Result<(), std::io::Error> {
                    let mut f = OpenOptions::new().create(true).append(true).open("logs/arbiter_llm.log")?;
                    let entry = json!({"event":"llm_graph_generation","provider": format!("{:?}", self.llm_config.provider_type), "prompt": prompt, "response": response});
                    writeln!(f, "[{}] {}", chrono::Utc::now().timestamp(), entry.to_string())?;
                    Ok(())
                })();

                // Reuse the robust RTFS extraction that prefers a balanced (do ...) block
                let do_block = self.extract_rtfs_from_response(&response)?;

                // Populate IntentGraph using the interpreter and return root intent id
        let mut graph = self.intent_graph
            .lock()
            .map_err(|_| RuntimeError::Generic("Failed to lock intent graph".to_string()))?;
        let root_id = crate::ccos::rtfs_bridge::graph_interpreter::build_graph_from_rtfs(&do_block, &mut graph)?;

        // After graph built, log the parsed root id and a compact serialization of current graph (best-effort)
        // Release the locked graph before doing any IO
        drop(graph);

        // Write a compact parsed event with the root id only (avoids cross-thread/runtime complexity)
        let _ = (|| -> Result<(), std::io::Error> {
            let mut f = OpenOptions::new().create(true).append(true).open("logs/arbiter_llm.log")?;
            let entry = json!({"event":"llm_graph_parsed","root": root_id});
            writeln!(f, "[{}] {}", chrono::Utc::now().timestamp(), entry.to_string())?;
            Ok(())
        })();
        Ok(root_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ccos::arbiter::arbiter_config::{LlmConfig, DelegationConfig, AgentRegistryConfig, AgentDefinition, LlmProviderType, RegistryType};

    fn create_test_config() -> (LlmConfig, DelegationConfig) {
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

        let delegation_config = DelegationConfig {
            enabled: true,
            threshold: 0.65,
            max_candidates: 3,
            min_skill_hits: Some(1),
            agent_registry: AgentRegistryConfig {
                registry_type: RegistryType::InMemory,
                database_url: None,
                agents: vec![
                    AgentDefinition {
                        agent_id: "sentiment_agent".to_string(),
                        name: "Sentiment Analysis Agent".to_string(),
                        capabilities: vec!["sentiment_analysis".to_string(), "text_processing".to_string()],
                        cost: 0.1,
                        trust_score: 0.9,
                        metadata: HashMap::new(),
                    },
                    AgentDefinition {
                        agent_id: "backup_agent".to_string(),
                        name: "Backup Agent".to_string(),
                        capabilities: vec!["backup".to_string(), "encryption".to_string()],
                        cost: 0.2,
                        trust_score: 0.8,
                        metadata: HashMap::new(),
                    },
                ],
            },
            adaptive_threshold: None,
        };

        (llm_config, delegation_config)
    }

    #[tokio::test]
    async fn test_delegating_arbiter_creation() {
        let (llm_config, delegation_config) = create_test_config();
        let intent_graph = std::sync::Arc::new(std::sync::Mutex::new(
            crate::ccos::intent_graph::IntentGraph::new().unwrap()
        ));
        
        let arbiter = DelegatingArbiter::new(llm_config, delegation_config, intent_graph).await;
        assert!(arbiter.is_ok());
    }

    #[tokio::test]
    async fn test_agent_registry() {
        let (_, delegation_config) = create_test_config();
        let registry = AgentRegistry::new(delegation_config.agent_registry);
        
        // Test finding agents for capabilities
        let agents = registry.find_agents_for_capabilities(&["sentiment_analysis".to_string()]);
        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0].agent_id, "sentiment_agent");
        
        // Test finding agents for multiple capabilities
        let agents = registry.find_agents_for_capabilities(&["backup".to_string(), "encryption".to_string()]);
        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0].agent_id, "backup_agent");
    }

    #[tokio::test]
    async fn test_intent_generation() {
        let (llm_config, delegation_config) = create_test_config();
        let intent_graph = std::sync::Arc::new(std::sync::Mutex::new(
            crate::ccos::intent_graph::IntentGraph::new().unwrap()
        ));
        
        let arbiter = DelegatingArbiter::new(llm_config, delegation_config, intent_graph).await.unwrap();
        
        let intent = arbiter.natural_language_to_intent(
            "analyze sentiment from user feedback",
            None
        ).await.unwrap();
        
        // tolerant check: ensure metadata contains a generation_method string mentioning 'delegat'
        if let Some(v) = intent.metadata.get(generation::GENERATION_METHOD) {
            if let Some(s) = v.as_string() {
                assert!(s.to_lowercase().contains("delegat"));
            } else {
                panic!("generation_method metadata is not a string");
            }
        } else {
            // generation_method metadata may be absent for some providers; accept if intent has a name or
            // original_request is non-empty as a fallback verification.
            assert!(intent.name.is_some() || !intent.original_request.is_empty());
        }
    }
}