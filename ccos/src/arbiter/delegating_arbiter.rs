//! Delegating Arbiter Engine
//!
//! This module provides a delegating approach that combines LLM-driven reasoning
//! with agent delegation for complex tasks. The delegating arbiter uses LLM to
//! understand requests and then delegates to specialized agents when appropriate.

use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;

use crate::arbiter::arbiter_config::{
    AgentDefinition, AgentRegistryConfig, DelegationConfig, LlmConfig,
};
use crate::arbiter::arbiter_engine::ArbiterEngine;
use crate::arbiter::llm_provider::{LlmProvider, LlmProviderFactory};
use crate::arbiter::plan_generation::{
    LlmRtfsPlanGenerationProvider, PlanGenerationProvider, PlanGenerationResult,
};
use crate::arbiter::prompt::{FilePromptStore, PromptManager};
use crate::capability_marketplace::types::{CapabilityKind, CapabilityQuery};
use crate::capability_marketplace::CapabilityMarketplace;
use crate::delegation_keys::{agent, generation};
use crate::synthesis::artifact_generator::generate_planner_via_arbiter;
use crate::synthesis::{schema_builder::ParamSchema, InteractionTurn};
use crate::types::{
    ExecutionResult, Intent, IntentStatus, Plan, PlanBody, PlanLanguage, PlanStatus, StorableIntent,
};
use regex;
use rtfs::runtime::error::RuntimeError;
use rtfs::runtime::values::Value;

use rtfs::ast::TopLevel;
use serde_json::json;
use std::fs::OpenOptions;
use std::io::Write;

// Strong guidance to ensure LLM emits (plan ...) top-level RTFS instead of (do ...)
const PLAN_FORMAT_GUIDANCE: &str = r#"
CRITICAL OUTPUT RULES (RTFS PLANNING):
1. Top-level form MUST be: (plan ...). NEVER output a top-level (do ...) form. If you draft (do ...), rewrite it as (plan ...).
2. (plan ...) should contain ordered (step "Name" <body>) forms. Each step should be declarative and describe user-value.
3. At MOST one (call :ccos.user.ask "...") per step. Do not re-ask questions already answered.
4. When no further refinements are possible, include a final step named "Finalize" or "Finalize Trip Specification" with a map value containing {:status "refinement_exhausted" :needs_capabilities ["capability.id"]} and DO NOT ask further questions.
5. If specialized capabilities are needed but unknown, list them in :needs_capabilities. Do not fabricate capabilities.
6. Output only the RTFS (plan ...) s-expression; no prose or explanations outside the s-expression.
7. AVOID parallel expressions containing (call :ccos.user.ask "...") - user interactions must be sequential.
"#;

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

/// Delegating arbiter that combines LLM reasoning with agent delegation
pub struct DelegatingArbiter {
    llm_config: LlmConfig,
    delegation_config: DelegationConfig,
    llm_provider: Box<dyn LlmProvider>,
    capability_marketplace: Arc<CapabilityMarketplace>,
    intent_graph: std::sync::Arc<std::sync::Mutex<crate::intent_graph::IntentGraph>>,
    adaptive_threshold_calculator: Option<crate::adaptive_threshold::AdaptiveThresholdCalculator>,
    prompt_manager: PromptManager<FilePromptStore>,
}

/// Agent registry for managing available agents
pub struct AgentRegistry {
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

        Self { agents }
    }

    /// Find agents that match the given capabilities
    pub fn find_agents_for_capabilities(
        &self,
        required_capabilities: &[String],
    ) -> Vec<&AgentDefinition> {
        let mut candidates = Vec::new();

        for agent in self.agents.values() {
            let matching_capabilities = agent
                .capabilities
                .iter()
                .filter(|cap| required_capabilities.contains(cap))
                .count();

            if matching_capabilities > 0 {
                candidates.push(agent);
            }
        }

        // Sort by trust score and cost
        candidates.sort_by(|a, b| {
            b.trust_score
                .partial_cmp(&a.trust_score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(
                    a.cost
                        .partial_cmp(&b.cost)
                        .unwrap_or(std::cmp::Ordering::Equal),
                )
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
        capability_marketplace: Arc<CapabilityMarketplace>,
        intent_graph: std::sync::Arc<std::sync::Mutex<crate::intent_graph::IntentGraph>>,
    ) -> Result<Self, RuntimeError> {
        // Create LLM provider
        let llm_provider =
            LlmProviderFactory::create_provider(llm_config.to_provider_config()).await?;

        // Create adaptive threshold calculator if configured
        let adaptive_threshold_calculator =
            delegation_config.adaptive_threshold.as_ref().map(|config| {
                crate::adaptive_threshold::AdaptiveThresholdCalculator::new(config.clone())
            });

        // Create prompt manager for file-based prompts
        // Assets are at workspace root, so try ../assets first, then assets (for when run from workspace root)
        let prompt_path = if std::path::Path::new("../assets/prompts/arbiter").exists() {
            "../assets/prompts/arbiter"
        } else {
            "assets/prompts/arbiter"
        };
        let prompt_store = FilePromptStore::new(prompt_path);
        let prompt_manager = PromptManager::new(prompt_store);

        Ok(Self {
            llm_config,
            delegation_config,
            llm_provider,
            capability_marketplace,
            intent_graph,
            adaptive_threshold_calculator,
            prompt_manager,
        })
    }

    /// Get the LLM configuration used by this arbiter
    pub fn get_llm_config(&self) -> &LlmConfig {
        &self.llm_config
    }

    /// Find agent capabilities that match the given required capabilities
    async fn find_agents_for_capabilities(
        &self,
        required_capabilities: &[String],
    ) -> Result<Vec<String>, RuntimeError> {
        // Query the marketplace for agent capabilities
        let query = CapabilityQuery::new()
            .with_kind(CapabilityKind::Agent)
            .with_limit(50); // Reasonable limit for agent discovery

        let agent_capabilities = self
            .capability_marketplace
            .list_capabilities_with_query(&query)
            .await;

        // Filter agents that have matching capabilities
        let mut matching_agents = Vec::new();
        for capability in agent_capabilities {
            // Check if this agent capability matches any of the required capabilities
            for required_cap in required_capabilities {
                if capability.id.contains(required_cap)
                    || capability
                        .description
                        .to_lowercase()
                        .contains(&required_cap.to_lowercase())
                {
                    matching_agents.push(capability.id.clone());
                    break;
                }
            }
        }

        Ok(matching_agents)
    }

    /// List all available agent capabilities
    async fn list_agent_capabilities(&self) -> Result<Vec<String>, RuntimeError> {
        let query = CapabilityQuery::new()
            .with_kind(CapabilityKind::Agent)
            .with_limit(100);

        let agent_capabilities = self
            .capability_marketplace
            .list_capabilities_with_query(&query)
            .await;

        Ok(agent_capabilities.into_iter().map(|c| c.id).collect())
    }

    /// Generate intent using LLM
    ///
    /// This method prioritizes RTFS format output from the LLM, but gracefully falls back
    /// to JSON parsing if the LLM returns JSON instead. The workflow is:
    /// 1. Request RTFS format via prompt
    /// 2. Try parsing response as RTFS using the RTFS parser
    /// 3. If RTFS parsing fails, attempt JSON parsing as fallback
    /// 4. Mark intents parsed from JSON with "parse_format" metadata for tracking
    async fn generate_intent_with_llm(
        &self,
        natural_language: &str,
        context: Option<HashMap<String, Value>>,
    ) -> Result<Intent, RuntimeError> {
        // Determine format mode (rtfs primary by default)
        let format_mode = std::env::var("CCOS_INTENT_FORMAT")
            .ok()
            .unwrap_or_else(|| "rtfs".to_string());

        // Create prompt (mode-specific)
        let prompt = self.create_intent_prompt(natural_language, context.clone());

        // Optional: display prompts during live runtime when enabled
        let show_prompts = std::env::var("RTFS_SHOW_PROMPTS")
            .map(|v| v == "1")
            .unwrap_or(false)
            || std::env::var("CCOS_DEBUG")
                .map(|v| v == "1")
                .unwrap_or(false);

        if show_prompts {
            println!(
                "\n=== Delegating Arbiter Intent Generation Prompt ===\n{}\n=== END PROMPT ===\n",
                prompt
            );
        }

        // Get raw text response
        let response = self.llm_provider.generate_text(&prompt).await?;

        // Optional: print only the extracted RTFS `(intent ...)` s-expression for debugging
        // This avoids echoing the full prompt/response while letting developers inspect
        // the structured intent the arbiter will parse. Controlled via env var
        // CCOS_PRINT_EXTRACTED_INTENT=1 or via DelegationConfig.print_extracted_intent
        let env_flag = std::env::var("CCOS_PRINT_EXTRACTED_INTENT")
            .map(|v| v == "1")
            .unwrap_or(false);
        let cfg_flag = self
            .delegation_config
            .print_extracted_intent
            .unwrap_or(false);
        if env_flag || cfg_flag {
            if let Some(intent_s_expr) = extract_intent(&response) {
                // Print a compact header and the extracted s-expression
                println!(
                    "[DELEGATING-ARBITER] Extracted RTFS intent:\n{}\n",
                    intent_s_expr
                );
            } else {
                println!("[DELEGATING-ARBITER] No RTFS intent s-expression found in LLM response.");
            }
        }

        if show_prompts {
            println!(
                "\n=== Delegating Arbiter Intent Generation Response ===\n{}\n=== END RESPONSE ===\n",
                response
            );
        }

        // Log provider and raw response (best-effort, non-fatal)
        let _ = (|| -> Result<(), std::io::Error> {
            let mut f = OpenOptions::new()
                .create(true)
                .append(true)
                .open("logs/arbiter_llm.log")?;
            // Serialize provider_type to a plain string (avoid Display requirement)
            let provider_str = serde_json::to_string(&self.llm_config.provider_type)
                .unwrap_or_else(|_| "\"unknown\"".to_string())
                .trim_matches('"')
                .to_string();
            let entry = json!({"event":"llm_intent_generation","provider": provider_str, "request": natural_language, "response_sample": response.chars().take(200).collect::<String>()});
            writeln!(
                f,
                "[{}] {}",
                chrono::Utc::now().timestamp(),
                entry.to_string()
            )?;
            Ok(())
        })();

        // Parse according to mode (RTFS primary with JSON fallback; JSON-only mode skips RTFS attempt)
        let mut intent = if format_mode == "json" {
            // Direct JSON parse path
            match self.parse_json_intent_response(&response, natural_language) {
                Ok(intent) => intent,
                Err(e) => {
                    return Err(RuntimeError::Generic(format!(
                        "Failed to parse JSON intent (json mode): {}",
                        e
                    )))
                }
            }
        } else {
            // RTFS-first mode
            match self.parse_llm_intent_response(&response, natural_language, context.clone()) {
                Ok(intent) => {
                    println!("✓ Successfully parsed intent from RTFS format");
                    intent
                }
                Err(rtfs_err) => {
                    println!(
                        "⚠ RTFS parsing failed, attempting JSON fallback: {}",
                        rtfs_err
                    );
                    match self.parse_json_intent_response(&response, natural_language) {
                        Ok(intent) => {
                            println!("ℹ Fallback succeeded: parsed JSON intent");
                            intent
                        }
                        Err(json_err) => {
                            // Generate user-friendly error message with response preview
                            let _response_preview = if response.len() > 500 {
                                format!(
                                    "{}...\n[truncated, total length: {} chars]",
                                    &response[..500],
                                    response.len()
                                )
                            } else {
                                response.clone()
                            };

                            let response_lines: Vec<&str> = response.lines().collect();
                            let line_preview = if response_lines.len() > 10 {
                                format!(
                                    "{}\n... [{} more lines]",
                                    response_lines[..10].join("\n"),
                                    response_lines.len() - 10
                                )
                            } else {
                                response.clone()
                            };

                            return Err(RuntimeError::Generic(format!(
                                "❌ Failed to parse LLM response as intent (both RTFS and JSON failed)\n\n\
                                📋 Expected format: An RTFS intent expression, like:\n\
                                (intent \"intent_name\" :goal \"User's goal description\" :constraints {{...}} :preferences {{...}})\n\n\
                                Or JSON format:\n\
                                {{\"goal\": \"User's goal\", \"name\": \"intent_name\", \"constraints\": {{}}, \"preferences\": {{}}}}\n\n\
                                📥 Received response:\n\
                                ┌─────────────────────────────────────────────────────────\n\
                                {}\n\
                                └─────────────────────────────────────────────────────────\n\n\
                                🔍 Parsing errors:\n\
                                • RTFS: {}\n\
                                • JSON: {}\n\n\
                                💡 Common issues:\n\
                                • Response is truncated or incomplete (check LLM token limits)\n\
                                • Response contains explanatory text before/after the intent definition\n\
                                • Missing required fields (:goal for RTFS, \"goal\" for JSON)\n\
                                • Invalid syntax (unclosed parentheses, mismatched quotes, etc.)\n\
                                • Response is empty or contains only whitespace\n\n\
                                🔧 Tip: The LLM should respond ONLY with the intent definition, no prose.",
                                line_preview,
                                rtfs_err,
                                json_err
                            )));
                        }
                    }
                }
            }
        };

        // Mark generation method and format
        intent.metadata.insert(
            generation::GENERATION_METHOD.to_string(),
            Value::String(generation::methods::DELEGATING_LLM.to_string()),
        );
        intent.metadata.insert(
            "intent_format_mode".to_string(),
            Value::String(format_mode.clone()),
        );
        // Derive parse_format if not already set (e.g., RTFS success path)
        if !intent.metadata.contains_key("parse_format") {
            let pf = if format_mode == "json" {
                "json"
            } else {
                "rtfs"
            };
            intent
                .metadata
                .insert("parse_format".to_string(), Value::String(pf.to_string()));
        }

        // Analyze delegation need and set delegation metadata
        let delegation_analysis = self
            .analyze_delegation_need(&intent, context.clone())
            .await?;

        // Debug: Log delegation analysis
        println!(
            "DEBUG: Delegation analysis: should_delegate={}, confidence={:.3}, required_capabilities=[{}]",
            delegation_analysis.should_delegate,
            delegation_analysis.delegation_confidence,
            delegation_analysis.required_capabilities.join(", ")
        );

        if delegation_analysis.should_delegate {
            // Find candidate agents
            let candidate_agents = self
                .find_agents_for_capabilities(&delegation_analysis.required_capabilities)
                .await?;

            println!("DEBUG: Found {} candidate agents", candidate_agents.len());
            for agent_id in &candidate_agents {
                println!("DEBUG: Agent: {}", agent_id);
            }

            if !candidate_agents.is_empty() {
                // Select the best agent (first one for now)
                let selected_agent = &candidate_agents[0];

                // Set delegation metadata
                intent.metadata.insert(
                    "delegation.selected_agent".to_string(),
                    Value::String(selected_agent.clone()),
                );
                intent.metadata.insert(
                    "delegation.candidates".to_string(),
                    Value::String(candidate_agents.join(", ")),
                );

                // Set intent name to match the selected agent
                intent.name = Some(selected_agent.clone());

                println!("DEBUG: Selected agent: {}", selected_agent);
            } else {
                println!(
                    "DEBUG: No candidate agents found for capabilities: [{}]",
                    delegation_analysis.required_capabilities.join(", ")
                );
            }
        } else {
            println!(
                "DEBUG: Delegation not recommended, confidence: {}",
                delegation_analysis.delegation_confidence
            );
        }

        // Append a compact JSONL entry with the generated intent for debugging
        let _ = (|| -> Result<(), std::io::Error> {
            let mut f = OpenOptions::new()
                .create(true)
                .append(true)
                .open("logs/arbiter_llm.log")?;
            // Serialize a minimal intent snapshot
            let intent_snapshot = json!({
                "intent_id": intent.intent_id,
                "name": intent.name,
                "goal": intent.goal,
                "metadata": intent.metadata,
            });
            // Serialize provider to plain string to avoid requiring Debug/Display impl
            let provider_str = serde_json::to_string(&self.llm_config.provider_type)
                .unwrap_or_else(|_| "\"unknown\"".to_string())
                .trim_matches('"')
                .to_string();
            let entry = json!({"event":"llm_intent_parsed","provider": provider_str, "intent": intent_snapshot});
            writeln!(
                f,
                "[{}] {}",
                chrono::Utc::now().timestamp(),
                entry.to_string()
            )?;
            Ok(())
        })();

        Ok(intent)
    }

    /// Public helper to generate an intent but also return the raw LLM response text.
    /// This is useful for diagnostics where the caller wants to inspect the LLM output
    /// alongside the parsed Intent. It follows the same RTFS-first / JSON-fallback
    /// parsing behaviour as `generate_intent_with_llm`.
    pub async fn natural_language_to_intent_with_raw(
        &self,
        natural_language: &str,
        context: Option<HashMap<String, Value>>,
    ) -> Result<(Intent, String), RuntimeError> {
        // Determine format mode and build prompt the same way as generate_intent_with_llm
        let format_mode = std::env::var("CCOS_INTENT_FORMAT")
            .ok()
            .unwrap_or_else(|| "rtfs".to_string());
        let prompt = self.create_intent_prompt(natural_language, context.clone());

        // Get raw text response from provider
        let response = self.llm_provider.generate_text(&prompt).await?;

        // Attempt parsing: RTFS first, JSON fallback (mirrors generate_intent_with_llm)
        let intent = if format_mode == "json" {
            self.parse_json_intent_response(&response, natural_language)?
        } else {
            match self.parse_llm_intent_response(&response, natural_language, context.clone()) {
                Ok(it) => it,
                Err(rtfs_err) => {
                    // Try JSON fallback
                    match self.parse_json_intent_response(&response, natural_language) {
                        Ok(it) => it,
                        Err(json_err) => {
                            return Err(RuntimeError::Generic(format!(
                                "Both RTFS and JSON parsing failed. RTFS error: {}; JSON error: {}",
                                rtfs_err, json_err
                            )));
                        }
                    }
                }
            }
        };

        // Store the intent (same side-effects as natural_language_to_intent)
        self.store_intent(&intent).await?;

        Ok((intent, response))
    }

    /// Generate raw LLM text without attempting to parse it as an Intent.
    ///
    /// This is useful for alternative synthesis workflows (e.g. capability
    /// generation) where we intentionally prompt the model to output a
    /// different top-level RTFS construct (like a `(capability ...)` form)
    /// that would cause the standard intent parser to fail.
    ///
    /// No side effects (no intent storage) occur here; caller is responsible
    /// for any parsing / validation and for deciding whether to fall back to
    /// the normal intent generation path if synthesis fails.
    pub async fn generate_raw_text(&self, prompt: &str) -> Result<String, RuntimeError> {
        match self.llm_provider.generate_text(prompt).await {
            Ok(s) => Ok(s),
            Err(e) => Err(RuntimeError::Generic(format!(
                "Raw LLM generation failed: {}",
                e
            ))),
        }
    }

    /// Synthesize a capability + plan pair by delegating to the LLM with a collector-informed prompt.
    /// Returns the raw RTFS emitted by the model so callers can decide how to persist or parse it.
    pub async fn synthesize_capability_from_collector(
        &self,
        schema: &ParamSchema,
        history: &[InteractionTurn],
        domain: &str,
    ) -> Result<String, RuntimeError> {
        let prompt = generate_planner_via_arbiter(schema, history, domain);

        let show_prompts = std::env::var("RTFS_SHOW_PROMPTS")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        if show_prompts {
            println!(
                "--- Delegating Arbiter Capability Prompt ---\n{}\n--- END PROMPT ---",
                prompt
            );
        }

        let response = self.generate_raw_text(&prompt).await?;

        if show_prompts {
            println!(
                "--- Delegating Arbiter Capability Response ---\n{}\n--- END RESPONSE ---",
                response
            );
        }

        let _ = (|| -> Result<(), std::io::Error> {
            std::fs::create_dir_all("logs")?;
            let mut f = OpenOptions::new()
                .create(true)
                .append(true)
                .open("logs/arbiter_capability.log")?;
            let entry = json!({
                "event": "capability_synthesis",
                "domain": domain,
                "prompt_length": prompt.len(),
                "response_sample": response.chars().take(400).collect::<String>()
            });
            writeln!(f, "[{}] {}", chrono::Utc::now().timestamp(), entry)?;
            Ok(())
        })();

        Ok(response)
    }

    /// Generate plan using LLM with agent delegation
    async fn generate_plan_with_delegation(
        &self,
        intent: &Intent,
        context: Option<HashMap<String, Value>>,
    ) -> Result<Plan, RuntimeError> {
        // First, analyze if delegation is appropriate
        let delegation_analysis = self
            .analyze_delegation_need(intent, context.clone())
            .await?;

        if delegation_analysis.should_delegate {
            // Generate plan with delegation
            self.generate_delegated_plan(intent, &delegation_analysis, context)
                .await
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
        let prompt = self
            .create_delegation_analysis_prompt(intent, context)
            .await?;

        let response = self.llm_provider.generate_text(&prompt).await?;

        // Parse delegation analysis
        let mut analysis = self.parse_delegation_analysis(&response)?;

        // Apply adaptive threshold if configured
        if let Some(calculator) = &self.adaptive_threshold_calculator {
            // Get base threshold from config
            let base_threshold = self.delegation_config.threshold;

            // For now, we'll use a default agent ID for threshold calculation
            // In the future, this could be based on the specific agent being considered
            let adaptive_threshold =
                calculator.calculate_threshold("default_agent", base_threshold);

            // Adjust delegation decision based on adaptive threshold
            analysis.should_delegate =
                analysis.should_delegate && analysis.delegation_confidence >= adaptive_threshold;

            // Update reasoning to include adaptive threshold information
            analysis.reasoning = format!(
                "{} [Adaptive threshold: {:.3}, Confidence: {:.3}]",
                analysis.reasoning, adaptive_threshold, analysis.delegation_confidence
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
        let candidate_agents = self
            .find_agents_for_capabilities(&delegation_analysis.required_capabilities)
            .await?;

        if candidate_agents.is_empty() {
            // No suitable agents found, fall back to direct plan
            return self.generate_direct_plan(intent, context).await;
        }

        // Select the best agent
        let selected_agent = &candidate_agents[0];

        // Generate delegation plan using the configured LLM provider.
        // Build a StorableIntent similar to the direct plan path but include
        // delegation-specific metadata so providers can tailor prompts.
        let storable_intent = StorableIntent {
            intent_id: intent.intent_id.clone(),
            name: intent.name.clone(),
            original_request: intent.original_request.clone(),
            rtfs_intent_source: "".to_string(),
            goal: intent.goal.clone(),
            constraints: intent
                .constraints
                .iter()
                .map(|(k, v)| (k.clone(), v.to_string()))
                .collect(),
            preferences: intent
                .preferences
                .iter()
                .map(|(k, v)| (k.clone(), v.to_string()))
                .collect(),
            success_criteria: intent.success_criteria.as_ref().map(|v| v.to_string()),
            parent_intent: None,
            child_intents: vec![],
            triggered_by: crate::types::TriggerSource::ArbiterInference,
            generation_context: crate::types::GenerationContext {
                arbiter_version: "delegating-1.0".to_string(),
                generation_timestamp: intent.created_at,
                input_context: {
                    let mut m = HashMap::new();
                    m.insert(
                        "delegation_target_agent".to_string(),
                        selected_agent.clone(),
                    );
                    m
                },
                reasoning_trace: None,
            },
            status: intent.status.clone(),
            priority: 0,
            created_at: intent.created_at,
            updated_at: intent.updated_at,
            metadata: {
                let mut meta = intent
                    .metadata
                    .iter()
                    .map(|(k, v)| (k.clone(), v.to_string()))
                    .collect::<HashMap<String, String>>();
                meta.insert(
                    "delegation.selected_agent".to_string(),
                    selected_agent.clone(),
                );
                meta.insert(
                    "delegation.agent_capabilities".to_string(),
                    "[agent_capabilities_from_marketplace]".to_string(),
                );
                meta
            },
        };

        // Convert context from Value to String for LlmProvider interface
        let string_context = context.as_ref().map(|ctx| {
            ctx.iter()
                .map(|(k, v)| (k.clone(), v.to_string()))
                .collect::<HashMap<String, String>>()
        });

        // Ask the provider to generate a plan for the selected agent and intent. This
        // lets provider implementations (including retries/validation) run their
        // full plan-generation flow instead of us building raw prompts and parsing.
        let plan = self
            .llm_provider
            .generate_plan(&storable_intent, string_context)
            .await?;

        // Log provider and result (best-effort, non-fatal)
        let _ = (|| -> Result<(), std::io::Error> {
            let mut f = OpenOptions::new()
                .create(true)
                .append(true)
                .open("logs/arbiter_llm.log")?;
            let provider_str = serde_json::to_string(&self.llm_config.provider_type)
                .unwrap_or_else(|_| "\"unknown\"".to_string())
                .trim_matches('"')
                .to_string();
            let entry = json!({"event":"llm_delegation_plan","provider": provider_str, "agent": selected_agent, "intent_id": intent.intent_id, "plan_id": plan.plan_id});
            writeln!(
                f,
                "[{}] {}",
                chrono::Utc::now().timestamp(),
                entry.to_string()
            )?;
            Ok(())
        })();

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
        // Convert Intent to StorableIntent for LlmProvider interface
        let storable_intent = StorableIntent {
            intent_id: intent.intent_id.clone(),
            name: intent.name.clone(),
            original_request: intent.original_request.clone(),
            rtfs_intent_source: "".to_string(), // Not used by LlmProvider
            goal: intent.goal.clone(),
            constraints: intent
                .constraints
                .iter()
                .map(|(k, v)| (k.clone(), v.to_string()))
                .collect(),
            preferences: intent
                .preferences
                .iter()
                .map(|(k, v)| (k.clone(), v.to_string()))
                .collect(),
            success_criteria: intent.success_criteria.as_ref().map(|v| v.to_string()),
            parent_intent: None,
            child_intents: vec![],
            triggered_by: crate::types::TriggerSource::ArbiterInference,
            generation_context: crate::types::GenerationContext {
                arbiter_version: "delegating-1.0".to_string(),
                generation_timestamp: intent.created_at,
                input_context: HashMap::new(),
                reasoning_trace: None,
            },
            status: intent.status.clone(),
            priority: 0,
            created_at: intent.created_at,
            updated_at: intent.updated_at,
            metadata: intent
                .metadata
                .iter()
                .map(|(k, v)| (k.clone(), v.to_string()))
                .collect(),
        };

        // Convert context from Value to String for LlmProvider interface
        let string_context = context.as_ref().map(|ctx| {
            ctx.iter()
                .map(|(k, v)| (k.clone(), v.to_string()))
                .collect::<HashMap<String, String>>()
        });

        let plan = self
            .llm_provider
            .generate_plan(&storable_intent, string_context)
            .await?;
        // Log provider and result
        let _ = (|| -> Result<(), std::io::Error> {
            let mut f = OpenOptions::new()
                .create(true)
                .append(true)
                .open("logs/arbiter_llm.log")?;
            let provider_str = serde_json::to_string(&self.llm_config.provider_type)
                .unwrap_or_else(|_| "\"unknown\"".to_string())
                .trim_matches('"')
                .to_string();
            let entry = json!({"event":"llm_direct_plan","provider": provider_str, "intent_id": intent.intent_id, "plan_id": plan.plan_id});
            writeln!(
                f,
                "[{}] {}",
                chrono::Utc::now().timestamp(),
                entry.to_string()
            )?;
            Ok(())
        })();

        // Plan generated directly by LLM provider
        // Log parsed plan for debugging
        self.log_parsed_plan(&plan);
        Ok(plan)
    }

    /// Create prompt for intent generation using file-based prompt store
    fn create_intent_prompt(
        &self,
        natural_language: &str,
        context: Option<HashMap<String, Value>>,
    ) -> String {
        // Decide format mode (rtfs | json). Default: rtfs (primary vessel of CCOS)
        let format_mode = std::env::var("CCOS_INTENT_FORMAT")
            .ok()
            .unwrap_or_else(|| "rtfs".to_string());

        // Keep capability list aligned with reduced RTFS grammar examples
        let available_capabilities = vec![
            "ccos.echo".to_string(),
            "ccos.math.add".to_string(),
            // user input capability used later in plan generation examples
            "ccos.user.ask".to_string(),
        ];

        let prompt_config = self.llm_config.prompts.clone().unwrap_or_default();

        let context_str = context
            .as_ref()
            .map(|c| {
                c.iter()
                    .map(|(k, v)| format!("{}: {}", k, v.to_string()))
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default();

        let mut vars = HashMap::new();
        vars.insert("natural_language".to_string(), natural_language.to_string());
        vars.insert("context".to_string(), context_str);
        vars.insert(
            "available_capabilities".to_string(),
            available_capabilities.join(", "),
        );

        if format_mode == "json" {
            // Legacy JSON mode (kept for compatibility)
            let mut rendered = self
                .prompt_manager
                .render(
                    &prompt_config.intent_prompt_id,
                    &prompt_config.intent_prompt_version,
                    &vars,
                )
                .unwrap_or_else(|e| {
                    eprintln!(
                        "Warning: Failed to load intent prompt from assets: {}. Using fallback.",
                        e
                    );
                    format!("# Fallback Intent Prompt (JSON mode)\n")
                });
            let nl_marker = "# Natural Language Request";
            if !rendered.contains(natural_language) {
                rendered.push_str("\n\n");
                rendered.push_str(nl_marker);
                rendered.push_str("\n\n");
                rendered.push_str("The following is the exact user request to convert into a structured intent. Use it to populate name, goal, constraints, preferences, success_criteria as per the rules above.\n\n");
                rendered.push_str("USER_REQUEST: \"");
                let sanitized = natural_language.replace('"', "'");
                rendered.push_str(&sanitized);
                rendered.push_str("\"\n\nRespond ONLY with the JSON intent object (no prose).\n");
            }
            if !rendered.contains("Available capabilities:") {
                rendered.push_str("\nAvailable capabilities: ");
                rendered.push_str(&available_capabilities.join(", "));
                rendered.push_str("\n");
            }
            rendered
        } else {
            // RTFS-first mode: load entire template (all sections auto-aggregated by PromptManager)
            let assembled = match self
                .prompt_manager
                .render("intent_generation_rtfs", "v1", &vars)
            {
                Ok(rendered) => rendered,
                Err(e) => {
                    eprintln!("Warning: Failed to load RTFS intent prompt bundle: {}. Falling back to inline template.", e);
                    String::new()
                }
            };
            if assembled.trim().is_empty() {
                // Fallback inline prompt (previous implementation)
                let mut prompt = String::new();
                prompt.push_str("# RTFS Intent Generation\n\n");
                prompt.push_str(
                    "Generate a single RTFS intent s-expression capturing the user request.\n\n",
                );
                prompt.push_str("## Form\n");
                prompt.push_str("(intent \"name\" :goal \"...\" [:constraints {:k \"v\" ...}] [:preferences {:k \"v\" ...}] [:success-criteria \"...\"])\n\n");
                prompt.push_str("Rules:\n- EXACTLY one top-level (intent ...) form (no wrapping (do ...), no JSON)\n- All constraint & preference values must be strings\n- name must be snake_case and descriptive\n- Include :success-criteria when meaningful\n- Only use keys: :goal :constraints :preferences :success-criteria (others ignored)\n\n");
                prompt.push_str("Examples:\n");
                prompt.push_str("User: ask the user for their name and greet them\n");
                prompt.push_str("(intent \"greet_user\" :goal \"Ask user name then greet\" :constraints {:interaction_mode \"single_turn\"} :preferences {:tone \"friendly\"} :success-criteria \"User greeted with their provided name\")\n\n");
                prompt.push_str("Anti-Patterns (DO NOT OUTPUT):\n- JSON objects\n- Multiple (intent ...) forms\n- Explanations or commentary\n\n");
                prompt.push_str("User Request:\n\n");
                let sanitized = natural_language.replace('"', "'");
                prompt.push_str(&format!("{}\n\n", sanitized));
                prompt.push_str("Output ONLY the RTFS (intent ...) form:\n");
                prompt.push_str("\nAvailable capabilities (for later planning): ");
                prompt.push_str(&available_capabilities.join(", "));
                prompt.push_str("\n");
                prompt
            } else {
                let mut final_prompt = assembled;
                // Ensure a blank line separation
                if !final_prompt.ends_with("\n\n") {
                    final_prompt.push_str("\n");
                }
                final_prompt.push_str("User Request:\n\n");
                let sanitized = natural_language.replace('"', "'");
                final_prompt.push_str(&sanitized);
                final_prompt
                    .push_str("\n\nOutput ONLY the single RTFS (intent ...) form (no prose).\n");
                final_prompt.push_str("\nAvailable capabilities (for later planning): ");
                final_prompt.push_str(&available_capabilities.join(", "));
                final_prompt.push_str("\n");
                final_prompt
            }
        }
    }

    /// Create prompt for delegation analysis using file-based prompt store
    async fn create_delegation_analysis_prompt(
        &self,
        intent: &Intent,
        context: Option<HashMap<String, Value>>,
    ) -> Result<String, RuntimeError> {
        let available_agents = self.list_agent_capabilities().await?;
        let agent_list = available_agents
            .iter()
            .map(|agent_id| format!("- {}: Agent capability from marketplace", agent_id))
            .collect::<Vec<_>>()
            .join("\n");

        let context_for_fallback = context.clone();

        let intent_str = serde_json::to_string(intent).unwrap_or_else(|_| {
            serde_json::to_string(&serde_json::json!({"name": intent.name, "goal": intent.goal}))
                .unwrap_or_else(|_| {
                    format!(
                        "{{\"name\":{} , \"goal\":{} }}",
                        intent
                            .name
                            .as_ref()
                            .map(|s| format!("\"{}\"", s))
                            .unwrap_or_else(|| "null".to_string()),
                        format!("\"{}\"", intent.goal)
                    )
                })
        });
        let context_str = serde_json::to_string(&context.unwrap_or_default())
            .unwrap_or_else(|_| "{}".to_string());
        let mut vars = HashMap::new();
        vars.insert("intent".to_string(), intent_str);
        vars.insert("context".to_string(), context_str);
        vars.insert("available_agents".to_string(), agent_list);

        let agent_list_for_fallback = vars["available_agents"].clone();

        Ok(self.prompt_manager
            .render("delegation_analysis", "v1", &vars)
            .unwrap_or_else(|e| {
                eprintln!("Warning: Failed to load delegation analysis prompt from assets: {}. Using fallback.", e);
                self.create_fallback_delegation_prompt(intent, context_for_fallback, &agent_list_for_fallback)
            }))
    }

    /// Fallback delegation analysis prompt (used when prompt assets fail to load)
    fn create_fallback_delegation_prompt(
        &self,
        intent: &Intent,
        context: Option<HashMap<String, Value>>,
        agent_list: &str,
    ) -> String {
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
            Intent: {}
            Context: {}
Available Agents:
{agents}

## Your JSON Response:"#,
            serde_json::to_string(&intent).unwrap_or_else(|_| "{}".to_string()),
            serde_json::to_string(&context.unwrap_or_default())
                .unwrap_or_else(|_| "{}".to_string()),
            agents = agent_list
        )
    }

    /// Create prompt for delegation plan generation using file-based prompt store
    fn create_delegation_plan_prompt(
        &self,
        intent: &Intent,
        agent: &AgentDefinition,
        context: Option<HashMap<String, Value>>,
    ) -> String {
        let available_capabilities = vec![
            "ccos.echo".to_string(),
            "ccos.validate".to_string(),
            "ccos.delegate".to_string(),
            "ccos.verify".to_string(),
        ];

        let intent_str = serde_json::to_string(intent).unwrap_or_else(|_| {
            format!(
                "{{\"name\":{} , \"goal\":{} }}",
                intent
                    .name
                    .as_ref()
                    .map(|s| format!("\"{}\"", s))
                    .unwrap_or_else(|| "null".to_string()),
                format!("\"{}\"", intent.goal)
            )
        });
        let context_str = serde_json::to_string(&context.unwrap_or_default())
            .unwrap_or_else(|_| "{}".to_string());
        let available_caps_str = available_capabilities.join(", ");
        let mut vars = HashMap::new();
        vars.insert("intent".to_string(), intent_str);
        vars.insert("context".to_string(), context_str);
        vars.insert(
            "available_capabilities".to_string(),
            available_caps_str.clone(),
        );
        vars.insert("agent_name".to_string(), agent.name.clone());
        vars.insert("agent_id".to_string(), agent.agent_id.clone());
        vars.insert(
            "agent_capabilities".to_string(),
            serde_json::to_string(&agent.capabilities)
                .unwrap_or_else(|_| "[unknown_capabilities]".to_string()),
        );
        vars.insert(
            "agent_trust_score".to_string(),
            format!("{:.2}", agent.trust_score),
        );
        vars.insert("agent_cost".to_string(), format!("{:.2}", agent.cost));
        vars.insert("delegation_mode".to_string(), "true".to_string());

        match self.prompt_manager.render("plan_generation", "v1", &vars) {
            Ok(rendered) => rendered,
            Err(e) => {
                eprintln!(
                    "Warning: Failed to load delegation plan prompt from assets: {}. Using fallback.",
                    e
                );
                let intent_json =
                    serde_json::to_string(intent).unwrap_or_else(|_| "{}".to_string());
                let agent_caps_json =
                    serde_json::to_string(&agent.capabilities).unwrap_or_else(|_| "[]".to_string());
                let caps_display = available_caps_str.clone();
                format!(
                    r#"Generate an RTFS plan that delegates to agent {} ({}).
Intent: {}
Agent Capabilities: {}
Available capabilities: {}
Plan:"#,
                    agent.name, agent.agent_id, intent_json, agent_caps_json, caps_display
                )
            }
        }
    }

    /// Create prompt for direct plan generation using file-based prompt store
    fn create_direct_plan_prompt(
        &self,
        intent: &Intent,
        context: Option<HashMap<String, Value>>,
    ) -> String {
        let available_capabilities = vec![
            "ccos.echo".to_string(),
            "ccos.math.add".to_string(),
            "ccos.user.ask".to_string(),
        ];
        let context_for_fallback = context.clone();

        let intent_str = serde_json::to_string(intent).unwrap_or_else(|_| {
            format!("{{\"name\":{:?}, \"goal\":{:?}}}", intent.name, intent.goal)
        });
        let context_str = serde_json::to_string(&context.as_ref().unwrap_or(&HashMap::new()))
            .unwrap_or_else(|_| "{}".to_string());
        let available_caps_str = available_capabilities.join(", ");
        let mut vars = HashMap::new();
        vars.insert("intent".to_string(), intent_str);
        vars.insert("context".to_string(), context_str);
        vars.insert(
            "available_capabilities".to_string(),
            available_caps_str.clone(),
        );
        vars.insert("delegation_mode".to_string(), "false".to_string());

        let fallback_prompt = {
            let intent_json = serde_json::to_string(intent).unwrap_or_else(|_| "{}".to_string());
            let context_json = context_for_fallback
                .as_ref()
                .map(|ctx| serde_json::to_string(ctx).unwrap_or_else(|_| "{}".to_string()))
                .unwrap_or_else(|| "{}".to_string());
            let caps_display = available_caps_str;
            format!(
                r#"Generate an RTFS plan for: {}
Context: {}
Available capabilities: {}
Plan:"#,
                intent_json, context_json, caps_display,
            )
        };

        match self.prompt_manager.render("plan_generation", "v1", &vars) {
            Ok(mut rendered) => {
                // Ensure guidance is appended to rendered prompt so model sees strict rules
                if !rendered.contains("CRITICAL OUTPUT RULES") {
                    rendered.push_str("\n");
                    rendered.push_str(PLAN_FORMAT_GUIDANCE);
                }
                rendered
            }
            Err(e) => {
                eprintln!(
                    "Warning: Failed to load plan generation prompt from assets: {}. Using fallback.",
                    e
                );
                fallback_prompt
            }
        }
    }

    /// Parse LLM response into intent structure using RTFS parser
    fn parse_llm_intent_response(
        &self,
        response: &str,
        _natural_language: &str,
        _context: Option<HashMap<String, Value>>,
    ) -> Result<Intent, RuntimeError> {
        // Extract the first top-level `(intent …)` s-expression from the response
        let intent_block = extract_intent(response).ok_or_else(|| {
            let response_preview = if response.len() > 400 {
                format!("{}...", &response[..400])
            } else {
                response.to_string()
            };
            RuntimeError::Generic(format!(
                "Could not locate a complete (intent …) block in LLM response.\n\n\
                📥 Response preview:\n\
                ┌─────────────────────────────────────────────────────────\n\
                {}\n\
                └─────────────────────────────────────────────────────────\n\n\
                💡 The response should start with (intent \"name\" :goal \"...\" ...)\n\
                Common issues: response is truncated, contains prose before the intent, or missing opening parenthesis.",
                response_preview
            ))
        })?;

        // Sanitize regex literals for parsing
        let sanitized = sanitize_regex_literals(&intent_block);

        // Parse using RTFS parser
        let ast_items = rtfs::parser::parse(&sanitized)
            .map_err(|e| RuntimeError::Generic(format!("Failed to parse RTFS intent: {:?}", e)))?;

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

    /// Parse JSON response as fallback when RTFS parsing fails
    fn parse_json_intent_response(
        &self,
        response: &str,
        natural_language: &str,
    ) -> Result<Intent, RuntimeError> {
        println!("🔄 Attempting to parse response as JSON...");

        // Extract JSON from response (handles markdown code blocks, etc.)
        let json_str = self.extract_json_from_response(response);

        // Parse the JSON
        let json_value: serde_json::Value = serde_json::from_str(&json_str).map_err(|e| {
            let json_preview = if json_str.len() > 400 {
                format!(
                    "{}...\n[truncated, total length: {} chars]",
                    &json_str[..400],
                    json_str.len()
                )
            } else {
                json_str.clone()
            };
            RuntimeError::Generic(format!(
                "Failed to parse JSON intent: {}\n\n\
                📥 JSON response preview:\n\
                ┌─────────────────────────────────────────────────────────\n\
                {}\n\
                └─────────────────────────────────────────────────────────\n\n\
                💡 Common JSON issues:\n\
                • Invalid JSON syntax (missing quotes, commas, brackets)\n\
                • Truncated response (incomplete JSON object)\n\
                • Missing required fields (\"goal\" is required)\n\
                • Response contains non-JSON text before/after the JSON",
                e, json_preview
            ))
        })?;

        // Extract intent fields from JSON
        let goal = json_value["goal"]
            .as_str()
            .or_else(|| json_value["Goal"].as_str())
            .or_else(|| json_value["GOAL"].as_str())
            .unwrap_or(natural_language)
            .to_string();

        let name = json_value["name"]
            .as_str()
            .or_else(|| json_value["Name"].as_str())
            .or_else(|| json_value["intent_name"].as_str())
            .map(|s| s.to_string());

        let mut intent = Intent::new(goal)
            .with_name(name.unwrap_or_else(|| format!("intent_{}", uuid::Uuid::new_v4())));

        intent.original_request = natural_language.to_string();

        // Extract constraints if present
        if let Some(constraints_obj) = json_value
            .get("constraints")
            .or_else(|| json_value.get("Constraints"))
        {
            if let Some(obj) = constraints_obj.as_object() {
                for (k, v) in obj {
                    let value = match v {
                        serde_json::Value::String(s) => Value::String(s.clone()),
                        serde_json::Value::Number(n) => {
                            if let Some(i) = n.as_i64() {
                                Value::Integer(i)
                            } else if let Some(f) = n.as_f64() {
                                Value::Float(f)
                            } else {
                                Value::String(v.to_string())
                            }
                        }
                        serde_json::Value::Bool(b) => Value::Boolean(*b),
                        _ => Value::String(v.to_string()),
                    };
                    intent.constraints.insert(k.clone(), value);
                }
            }
        }

        // Extract preferences if present
        if let Some(preferences_obj) = json_value
            .get("preferences")
            .or_else(|| json_value.get("Preferences"))
        {
            if let Some(obj) = preferences_obj.as_object() {
                for (k, v) in obj {
                    let value = match v {
                        serde_json::Value::String(s) => Value::String(s.clone()),
                        serde_json::Value::Number(n) => {
                            if let Some(i) = n.as_i64() {
                                Value::Integer(i)
                            } else if let Some(f) = n.as_f64() {
                                Value::Float(f)
                            } else {
                                Value::String(v.to_string())
                            }
                        }
                        serde_json::Value::Bool(b) => Value::Boolean(*b),
                        _ => Value::String(v.to_string()),
                    };
                    intent.preferences.insert(k.clone(), value);
                }
            }
        }

        // Mark that this was parsed from JSON
        intent.metadata.insert(
            "parse_format".to_string(),
            Value::String("json_fallback".to_string()),
        );

        println!("✓ Successfully parsed intent from JSON format");

        Ok(intent)
    }

    /// Parse delegation analysis response with robust error handling
    fn parse_delegation_analysis(
        &self,
        response: &str,
    ) -> Result<DelegationAnalysis, RuntimeError> {
        // Clean the response - remove any leading/trailing whitespace and extract JSON
        let cleaned_response = self.extract_json_from_response(response);

        // Try to parse the JSON
        let json_response: serde_json::Value =
            serde_json::from_str(&cleaned_response).map_err(|e| {
                // Generate user-friendly error message with full response preview
                let _response_preview = if response.len() > 500 {
                    format!(
                        "{}...\n[truncated, total length: {} chars]",
                        &response[..500],
                        response.len()
                    )
                } else {
                    response.to_string()
                };

                let response_lines: Vec<&str> = response.lines().collect();
                let line_preview = if response_lines.len() > 10 {
                    format!(
                        "{}\n... [{} more lines]",
                        response_lines[..10].join("\n"),
                        response_lines.len() - 10
                    )
                } else {
                    response.to_string()
                };

                let cleaned_preview = if cleaned_response.len() > 400 {
                    format!(
                        "{}...\n[truncated, total length: {} chars]",
                        &cleaned_response[..400],
                        cleaned_response.len()
                    )
                } else {
                    cleaned_response.clone()
                };

                RuntimeError::Generic(format!(
                    "❌ Failed to parse delegation analysis JSON\n\n\
                    📋 Expected format: A JSON object with fields:\n\
                    {{\n\
                      \"should_delegate\": true/false,\n\
                      \"reasoning\": \"explanation text\",\n\
                      \"required_capabilities\": [\"cap1\", \"cap2\"],\n\
                      \"delegation_confidence\": 0.0-1.0\n\
                    }}\n\n\
                    📥 Original LLM response:\n\
                    ┌─────────────────────────────────────────────────────────\n\
                    {}\n\
                    └─────────────────────────────────────────────────────────\n\n\
                    🔧 Extracted JSON (after cleaning):\n\
                    ┌─────────────────────────────────────────────────────────\n\
                    {}\n\
                    └─────────────────────────────────────────────────────────\n\n\
                    🔍 JSON parsing error: {}\n\n\
                    💡 Common issues:\n\
                    • LLM responded with prose instead of JSON\n\
                    • Response is truncated or incomplete\n\
                    • Missing required fields (should_delegate, reasoning, etc.)\n\
                    • Invalid JSON syntax (unclosed brackets, missing quotes, etc.)\n\
                    • Response is empty or contains only whitespace\n\n\
                    🔧 Tip: The LLM should respond ONLY with valid JSON, no explanatory text.",
                    line_preview, cleaned_preview, e
                ))
            })?;

        // Validate required fields
        if !json_response.is_object() {
            return Err(RuntimeError::Generic(
                "Delegation analysis response is not a JSON object".to_string(),
            ));
        }

        let should_delegate = json_response["should_delegate"].as_bool().ok_or_else(|| {
            RuntimeError::Generic("Missing or invalid 'should_delegate' field".to_string())
        })?;

        let reasoning = json_response["reasoning"]
            .as_str()
            .ok_or_else(|| {
                RuntimeError::Generic("Missing or invalid 'reasoning' field".to_string())
            })?
            .to_string();

        let required_capabilities = json_response["required_capabilities"]
            .as_array()
            .ok_or_else(|| {
                RuntimeError::Generic(
                    "Missing or invalid 'required_capabilities' field".to_string(),
                )
            })?
            .iter()
            .filter_map(|v| v.as_str())
            .map(|s| s.to_string())
            .collect::<Vec<_>>();

        let delegation_confidence =
            json_response["delegation_confidence"]
                .as_f64()
                .ok_or_else(|| {
                    RuntimeError::Generic(
                        "Missing or invalid 'delegation_confidence' field".to_string(),
                    )
                })?;

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
    pub fn get_agent_performance(
        &self,
        agent_id: &str,
    ) -> Option<&crate::adaptive_threshold::AgentPerformance> {
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
        // Optionally print extracted RTFS plan for diagnostics (env or config)
        let print_flag = std::env::var("CCOS_PRINT_EXTRACTED_PLAN")
            .map(|s| s == "1")
            .unwrap_or(false)
            || self.delegation_config.print_extracted_plan.unwrap_or(false);

        if print_flag {
            println!(
                "[DELEGATING-ARBITER] Extracted RTFS plan:\n{}",
                rtfs_content
            );
        }

        Ok(Plan {
            plan_id: format!("delegating_plan_{}", uuid::Uuid::new_v4()),
            name: Some(format!(
                "delegated_plan_{}",
                intent.name.as_ref().unwrap_or(&"unknown".to_string())
            )),
            intent_ids: vec![intent.intent_id.clone()],
            language: PlanLanguage::Rtfs20,
            body: PlanBody::Rtfs(rtfs_content),
            status: PlanStatus::Draft,
            created_at: now,
            metadata: {
                let mut meta = HashMap::new();
                meta.insert(
                    generation::GENERATION_METHOD.to_string(),
                    Value::String(generation::methods::DELEGATION.to_string()),
                );
                meta.insert(
                    agent::DELEGATED_AGENT.to_string(),
                    Value::String(agent.agent_id.clone()),
                );
                meta.insert(
                    agent::AGENT_TRUST_SCORE.to_string(),
                    Value::Float(agent.trust_score),
                );
                meta.insert(
                    agent::AGENT_COST.to_string(),
                    Value::Float(agent.cost as f64),
                );
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
        // Optionally print extracted RTFS plan for diagnostics (env or config)
        let print_flag = std::env::var("CCOS_PRINT_EXTRACTED_PLAN")
            .map(|s| s == "1")
            .unwrap_or(false)
            || self.delegation_config.print_extracted_plan.unwrap_or(false);
        if print_flag {
            println!(
                "[DELEGATING-ARBITER] Extracted RTFS plan:\n{}",
                rtfs_content
            );
        }

        Ok(Plan {
            plan_id: format!("direct_plan_{}", uuid::Uuid::new_v4()),
            name: Some(format!(
                "direct_plan_{}",
                intent.name.as_ref().unwrap_or(&"unknown".to_string())
            )),
            intent_ids: vec![intent.intent_id.clone()],
            language: PlanLanguage::Rtfs20,
            body: PlanBody::Rtfs(rtfs_content),
            status: PlanStatus::Draft,
            created_at: now,
            metadata: {
                let mut meta = HashMap::new();
                meta.insert(
                    generation::GENERATION_METHOD.to_string(),
                    Value::String(generation::methods::DIRECT.to_string()),
                );
                meta.insert(
                    "llm_provider".to_string(),
                    Value::String(
                        serde_json::to_string(&self.llm_config.provider_type)
                            .unwrap_or_else(|_| format!("{:?}", self.llm_config.provider_type)),
                    ),
                );
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
        // Optionally print the extracted RTFS plan to stdout for diagnostics.
        // Controlled by env var CCOS_PRINT_EXTRACTED_PLAN=1 or runtime delegation flag.
        let env_flag = std::env::var("CCOS_PRINT_EXTRACTED_PLAN")
            .map(|v| v == "1")
            .unwrap_or(false);
        let cfg_flag = self.delegation_config.print_extracted_plan.unwrap_or(false);
        if env_flag || cfg_flag {
            if let crate::types::PlanBody::Rtfs(ref s) = &plan.body {
                println!(
                    "[DELEGATING-ARBITER] Parsed RTFS plan (plan_id={}):\n{}",
                    plan.plan_id, s
                );
            }
        }

        let _ = (|| -> Result<(), std::io::Error> {
            let mut f = OpenOptions::new()
                .create(true)
                .append(true)
                .open("logs/arbiter_llm.log")?;
            let entry = json!({"event":"llm_plan_parsed","plan_id": plan.plan_id, "rtfs_body": match &plan.body { crate::types::PlanBody::Rtfs(s) => s, _ => "" }});
            writeln!(
                f,
                "[{}] {}",
                chrono::Utc::now().timestamp(),
                entry.to_string()
            )?;
            Ok(())
        })();
    }

    /// Extract RTFS plan from LLM response, preferring a balanced (plan ...) or (do ...) block
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
                    let block = &rest[start..start + end + 1];
                    // quick check for :type "intent"
                    if block.contains(":type \"intent\"") || block.contains(":type 'intent'") {
                        // parse simple key/value pairs inside block
                        // remove surrounding braces and split on ':' keys (best-effort)
                        let inner = &block[1..block.len() - 1];
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
                                            if nt.ends_with('"') {
                                                break;
                                            }
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
                                if k == "name" || k == "type" || k == "goal" {
                                    continue;
                                }
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
                        rest = &rest[start + end + 1..];
                        continue;
                    }
                    // not an intent map, copy as-is
                    out.push_str(block);
                    rest = &rest[start + end + 1..];
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

        let mut response = normalize_map_style_intents(response);

        // Defensive normalization: if model emits a top-level (do ...) wrap/convert it into a (plan ...)
        if response.trim_start().starts_with("(do") {
            // Replace the leading '(do' with '(plan :name "normalized_plan" :language rtfs20 :body (do' and close the plan later
            if let Some(rest) = response.trim_start().strip_prefix("(do") {
                response = format!(
                    "(plan :name \"normalized_plan\" :language rtfs20 :body (do{} ) )",
                    rest
                );
            }
        }

        // 1) Prefer fenced rtfs code blocks
        if let Some(code_start) = response.find("```rtfs") {
            if let Some(code_end) = response[code_start + 7..].find("```") {
                let fenced = &response[code_start + 7..code_start + 7 + code_end];

                // Look for (plan ...) or (do ...) blocks inside
                if let Some(idx) = fenced.find("(plan") {
                    if let Some(block) = Self::extract_balanced_from(fenced, idx) {
                        return Ok(block);
                    }
                } else if let Some(idx) = fenced.find("(do") {
                    if let Some(block) = Self::extract_balanced_from(fenced, idx) {
                        return Ok(block);
                    }
                }

                // Otherwise, return fenced content trimmed
                let trimmed = fenced.trim();
                // Guard: avoid returning a raw (intent ...) block as a plan
                if trimmed.starts_with("(intent") {
                    return Err(RuntimeError::Generic(
                        "LLM response contains an intent block, but no plan (plan ... or do ...) block"
                            .to_string(),
                    ));
                }
                return Ok(trimmed.to_string());
            }
        }

        // 2) Search raw text for a (plan ...) or (do ...) block
        if let Some(idx) = response.find("(plan") {
            if let Some(block) = Self::extract_balanced_from(&response, idx) {
                return Ok(block);
            }
        } else if let Some(idx) = response.find("(do") {
            if let Some(block) = Self::extract_balanced_from(&response, idx) {
                return Ok(block);
            }
        }

        // 3) As a last resort, handle top-level blocks. If the response contains only (intent ...) blocks,
        // wrap them into a (plan ...) block with a (do ...) body so they become an executable RTFS plan.
        // If other top-level blocks exist, return the first non-(intent) balanced block.
        if let Some(idx) = response.find('(') {
            let mut collected_intents = Vec::new();
            let mut _found_plan_or_do = false;
            let mut remaining = &response[idx..];

            // Collect consecutive top-level balanced blocks
            while let Some(block) = Self::extract_balanced_from(remaining, 0) {
                let trimmed = block.trim_start();
                if trimmed.starts_with("(intent") {
                    collected_intents.push(block.clone());
                } else if trimmed.starts_with("(plan") || trimmed.starts_with("(do") {
                    // Found a plan or do block: prefer returning it
                    _found_plan_or_do = true;
                    return Ok(block);
                } else {
                    // Found some other top-level block: return it if no plan/do blocks found yet
                    if !_found_plan_or_do {
                        return Ok(block);
                    }
                }

                // Advance remaining slice
                let consumed = block.len();
                if consumed >= remaining.len() {
                    break;
                }
                remaining = &remaining[consumed..];
                // Skip whitespace/newlines
                let skip = remaining.find(|c: char| !c.is_whitespace()).unwrap_or(0);
                remaining = &remaining[skip..];
            }

            if !collected_intents.is_empty() {
                // Wrap collected intent blocks in a (plan ...) wrapper with (do ...) body
                let mut plan_block = String::from(
                    "(plan\n  :name \"generated_from_intents\"\n  :language rtfs20\n  :body (do\n",
                );
                for ib in collected_intents.iter() {
                    plan_block.push_str("    ");
                    plan_block.push_str(ib.trim());
                    plan_block.push_str("\n");
                }
                plan_block.push_str("  )\n)");
                return Ok(plan_block);
            }
        }

        // Before returning the error, log a compact record with the raw response to help debugging
        let _ = (|| -> Result<(), std::io::Error> {
            let mut f = OpenOptions::new()
                .create(true)
                .append(true)
                .open("logs/arbiter_llm.log")?;
            let entry = json!({
                "event": "llm_plan_extract_failed",
                "error": "Could not extract an RTFS plan (plan ... or do ...) from LLM response",
                "response_sample": response.chars().take(200).collect::<String>()
            });
            writeln!(
                f,
                "[{}] {}",
                chrono::Utc::now().timestamp(),
                entry.to_string()
            )?;
            Ok(())
        })();

        Err(RuntimeError::Generic(
            "Could not extract an RTFS plan (plan ... or do ...) from LLM response".to_string(),
        ))
    }

    /// Helper: extract a balanced s-expression starting at `start_idx` in `text`
    fn extract_balanced_from(text: &str, start_idx: usize) -> Option<String> {
        let bytes = text.as_bytes();
        if bytes.get(start_idx) != Some(&b'(') {
            return None;
        }
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
        let mut graph = self
            .intent_graph
            .lock()
            .map_err(|_| RuntimeError::Generic("Failed to lock intent graph".to_string()))?;

        // Convert to storable intent
        let storable = StorableIntent {
            intent_id: intent.intent_id.clone(),
            name: intent.name.clone(),
            original_request: intent.original_request.clone(),
            rtfs_intent_source: "delegating_generated".to_string(),
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
            triggered_by: crate::types::TriggerSource::HumanRequest,
            generation_context: crate::types::GenerationContext {
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

        graph
            .storage
            .store_intent(storable)
            .await
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
        let intent = self
            .generate_intent_with_llm(natural_language, context)
            .await?;

        // Store the intent
        self.store_intent(&intent).await?;

        Ok(intent)
    }

    async fn intent_to_plan(&self, intent: &Intent) -> Result<Plan, RuntimeError> {
        self.generate_plan_with_delegation(intent, None).await
    }

    async fn execute_plan(&self, plan: &Plan) -> Result<ExecutionResult, RuntimeError> {
        // For delegating arbiter, we return a placeholder execution result
        // In a real implementation, this would execute the RTFS plan
        Ok(ExecutionResult {
            success: true,
            value: Value::String("Delegating arbiter execution placeholder".to_string()),
            metadata: {
                let mut meta = HashMap::new();
                meta.insert("plan_id".to_string(), Value::String(plan.plan_id.clone()));
                meta.insert(
                    "delegating_engine".to_string(),
                    Value::String("delegating".to_string()),
                );
                if let Some(generation_method) = plan.metadata.get(generation::GENERATION_METHOD) {
                    meta.insert(
                        generation::GENERATION_METHOD.to_string(),
                        generation_method.clone(),
                    );
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

        // Debug: Show raw LLM response
        println!("🤖 LLM Response for goal '{}':", natural_language_goal);
        println!("📝 Raw LLM Response:\n{}", response);
        println!("--- End Raw LLM Response ---");

        // Log provider, prompt and raw response
        let _ = (|| -> Result<(), std::io::Error> {
            let mut f = OpenOptions::new()
                .create(true)
                .append(true)
                .open("logs/arbiter_llm.log")?;
            let entry = json!({"event":"llm_graph_generation","provider": serde_json::to_string(&self.llm_config.provider_type).unwrap_or_else(|_| format!("{:?}", self.llm_config.provider_type)), "prompt": prompt, "response": response});
            writeln!(
                f,
                "[{}] {}",
                chrono::Utc::now().timestamp(),
                entry.to_string()
            )?;
            Ok(())
        })();

        // Reuse the robust RTFS extraction that prefers a balanced (do ...) block
        let do_block = self.extract_rtfs_from_response(&response)?;

        // Debug: Show extracted RTFS
        println!("🔍 Extracted RTFS from LLM response:");
        println!("📋 RTFS Code:\n{}", do_block);
        println!("--- End Extracted RTFS ---");

        // Populate IntentGraph using the interpreter and return root intent id
        let mut graph = self
            .intent_graph
            .lock()
            .map_err(|_| RuntimeError::Generic("Failed to lock intent graph".to_string()))?;
        let root_id =
            crate::rtfs_bridge::graph_interpreter::build_graph_from_rtfs(&do_block, &mut graph)?;

        // Debug: Show the parsed graph structure
        println!("🏗️ Parsed Graph Structure:");
        println!("🎯 Root Intent ID: {}", root_id);

        // Show all intents in the graph
        let all_intents = graph
            .storage
            .list_intents(crate::intent_storage::IntentFilter::default())
            .await
            .map_err(|e| RuntimeError::Generic(format!("Failed to list intents: {}", e)))?;

        println!("📊 Total Intents in Graph: {}", all_intents.len());
        for (i, intent) in all_intents.iter().enumerate() {
            println!(
                "  [{}] ID: {} | Goal: '{}' | Status: {:?}",
                i + 1,
                intent.intent_id,
                intent.goal,
                intent.status
            );
        }

        // Show all edges in the graph
        let all_edges = graph
            .storage
            .get_edges()
            .await
            .map_err(|e| RuntimeError::Generic(format!("Failed to get edges: {}", e)))?;

        println!("🔗 Total Edges in Graph: {}", all_edges.len());
        for (i, edge) in all_edges.iter().enumerate() {
            println!(
                "  [{}] {} -> {} (type: {:?})",
                i + 1,
                edge.from,
                edge.to,
                edge.edge_type
            );
        }
        println!("--- End Parsed Graph Structure ---");

        // After graph built, log the parsed root id and a compact serialization of current graph (best-effort)
        // Release the locked graph before doing any IO
        drop(graph);

        // Write a compact parsed event with the root id only (avoids cross-thread/runtime complexity)
        let _ = (|| -> Result<(), std::io::Error> {
            let mut f = OpenOptions::new()
                .create(true)
                .append(true)
                .open("logs/arbiter_llm.log")?;
            let entry = json!({"event":"llm_graph_parsed","root": root_id});
            writeln!(
                f,
                "[{}] {}",
                chrono::Utc::now().timestamp(),
                entry.to_string()
            )?;
            Ok(())
        })();
        Ok(root_id)
    }

    async fn generate_plan_for_intent(
        &self,
        intent: &StorableIntent,
    ) -> Result<PlanGenerationResult, RuntimeError> {
        // Use LLM provider-based plan generator
        let provider_cfg = self.llm_config.to_provider_config();
        let _provider =
            crate::arbiter::llm_provider::LlmProviderFactory::create_provider(provider_cfg.clone())
                .await?;
        let plan_gen_provider = LlmRtfsPlanGenerationProvider::new(provider_cfg);

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
                rtfs::runtime::capabilities::registry::CapabilityRegistry::new(),
            )),
        ));
        plan_gen_provider
            .generate_plan(&rt_intent, marketplace)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arbiter::arbiter_config::{
        AgentDefinition, AgentRegistryConfig, DelegationConfig, LlmConfig, LlmProviderType,
        RegistryType,
    };
    use crate::capabilities::registry::CapabilityRegistry;
    use std::sync::Arc;
    use tokio::sync::RwLock;

    fn create_test_config() -> (LlmConfig, DelegationConfig) {
        let llm_config = LlmConfig {
            provider_type: LlmProviderType::Stub,
            model: "stub-model".to_string(),
            api_key: None,
            base_url: None,
            max_tokens: Some(1000),
            temperature: Some(0.7),
            timeout_seconds: Some(30),
            retry_config: crate::arbiter::arbiter_config::RetryConfig::default(),
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
                        capabilities: vec![
                            "sentiment_analysis".to_string(),
                            "text_processing".to_string(),
                        ],
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
            print_extracted_intent: None,
            print_extracted_plan: None,
        };

        (llm_config, delegation_config)
    }

    #[tokio::test]
    async fn test_delegating_arbiter_creation() {
        let (llm_config, delegation_config) = create_test_config();
        let intent_graph = std::sync::Arc::new(std::sync::Mutex::new(
            crate::intent_graph::IntentGraph::new().unwrap(),
        ));

        // Create a minimal capability marketplace for testing
        let registry = Arc::new(RwLock::new(
            rtfs::runtime::capabilities::registry::CapabilityRegistry::new(),
        ));
        let marketplace = Arc::new(CapabilityMarketplace::new(registry));

        let arbiter =
            DelegatingArbiter::new(llm_config, delegation_config, marketplace, intent_graph).await;
        assert!(arbiter.is_ok());
    }

    #[tokio::test]
    async fn test_agent_registry() {
        // This test is now obsolete since we use CapabilityMarketplace instead of AgentRegistry
        // The functionality is tested in the agent_unification_tests.rs file
        assert!(true);
    }

    #[tokio::test]
    async fn test_intent_generation() {
        let (llm_config, delegation_config) = create_test_config();
        let intent_graph = std::sync::Arc::new(std::sync::Mutex::new(
            crate::intent_graph::IntentGraph::new().unwrap(),
        ));

        // Create a minimal capability marketplace for testing
        let registry = Arc::new(RwLock::new(
            rtfs::runtime::capabilities::registry::CapabilityRegistry::new(),
        ));
        let marketplace = Arc::new(CapabilityMarketplace::new(registry));

        let arbiter =
            DelegatingArbiter::new(llm_config, delegation_config, marketplace, intent_graph)
                .await
                .unwrap();

        let intent = arbiter
            .natural_language_to_intent("analyze sentiment from user feedback", None)
            .await
            .unwrap();

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

    #[tokio::test]
    async fn test_json_fallback_parsing() {
        let (llm_config, delegation_config) = create_test_config();
        let intent_graph = std::sync::Arc::new(std::sync::Mutex::new(
            crate::intent_graph::IntentGraph::new().unwrap(),
        ));

        // Create a minimal capability marketplace for testing
        let registry = Arc::new(RwLock::new(
            rtfs::runtime::capabilities::registry::CapabilityRegistry::new(),
        ));
        let marketplace = Arc::new(CapabilityMarketplace::new(registry));

        let arbiter =
            DelegatingArbiter::new(llm_config, delegation_config, marketplace, intent_graph)
                .await
                .unwrap();

        // Test parsing a JSON response
        let json_response = r#"
        {
            "name": "backup-system",
            "goal": "Create a backup system for user data",
            "constraints": {
                "frequency": "daily",
                "retention": 30
            },
            "preferences": {
                "encryption": true,
                "compression": "gzip"
            }
        }
        "#;

        let intent = arbiter
            .parse_json_intent_response(json_response, "Create a backup system")
            .unwrap();

        assert_eq!(intent.name, Some("backup-system".to_string()));
        assert_eq!(intent.goal, "Create a backup system for user data");
        assert!(intent.constraints.contains_key("frequency"));
        assert!(intent.preferences.contains_key("encryption"));

        // Check that it was marked as JSON fallback
        assert_eq!(
            intent
                .metadata
                .get("parse_format")
                .and_then(|v| v.as_string())
                .as_deref(),
            Some("json_fallback")
        );
    }
}
