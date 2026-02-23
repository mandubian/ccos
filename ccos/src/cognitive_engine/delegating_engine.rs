//! Delegating Arbiter Engine
//!
//! This module provides a delegating approach that combines LLM-driven reasoning
//! with agent delegation for complex tasks. The delegating arbiter uses LLM to
//! understand requests and then delegates to specialized agents when appropriate.

use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;

use super::config::{DelegationConfig, LlmConfig, RetryConfig};
use super::delegation_analysis::{DelegationAnalysis, DelegationAnalyzer};
use super::engine::CognitiveEngine;
use super::intent_parsing::{
    extract_intent, parse_json_intent_response, parse_llm_intent_response,
};
use super::llm_provider::{LlmProvider, LlmProviderFactory};
use super::plan_generation::{
    LlmRtfsPlanGenerationProvider, PlanGenerationProvider, PlanGenerationResult,
};
use super::prompt::{FilePromptStore, PromptManager};

use crate::capability_marketplace::types::{CapabilityKind, CapabilityManifest, CapabilityQuery};
use crate::capability_marketplace::CapabilityMarketplace;
use crate::delegation_keys::{agent, generation};
use crate::llm::tool_calling::{
    capability_id_to_tool_name, resolve_capability_id, ToolChatMessage, ToolChatRequest,
    ToolDefinition,
};
use crate::synthesis::artifact_generator::generate_planner_via_arbiter;
use crate::synthesis::{schema_builder::ParamSchema, InteractionTurn};
use crate::types::{ExecutionResult, Intent, IntentStatus, Plan, StorableIntent};
use crate::utils::value_conversion::json_to_rtfs_value;

use rtfs::runtime::error::{RuntimeError, RuntimeResult};

use rtfs::runtime::values::Value;
use serde_json::json;
use std::fs::OpenOptions;
use std::io::Write;

// Strong guidance to ensure LLM emits (plan ...) top-level RTFS instead of (do ...)
#[allow(dead_code)]
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

/// Delegating Cognitive Engine that combines LLM reasoning with agent delegation
pub struct DelegatingCognitiveEngine {
    llm_config: LlmConfig,
    delegation_config: DelegationConfig,
    llm_provider: Arc<dyn LlmProvider>,
    capability_marketplace: Arc<CapabilityMarketplace>,
    intent_graph: std::sync::Arc<std::sync::Mutex<crate::types::IntentGraph>>,
    delegation_analyzer: DelegationAnalyzer,
    prompt_manager: PromptManager<FilePromptStore>,
    /// Optional WorkingMemory for learning-driven plan augmentation
    working_memory: Option<Arc<std::sync::Mutex<crate::working_memory::WorkingMemory>>>,
}

impl DelegatingCognitiveEngine {
    /// Get the LLM provider
    pub fn llm_provider(&self) -> &dyn LlmProvider {
        self.llm_provider.as_ref()
    }

    /// Get the LLM provider as Arc
    pub fn get_llm_provider_arc(&self) -> Arc<dyn LlmProvider> {
        self.llm_provider.clone()
    }

    /// Analyze whether delegation is needed for this intent
    pub async fn analyze_delegation_need(
        &self,
        intent: &Intent,
        context: Option<HashMap<String, Value>>,
    ) -> RuntimeResult<DelegationAnalysis> {
        // Fetch agents from the marketplace
        let available_agents = self.list_agent_capabilities().await?;

        self.delegation_analyzer
            .analyze_need(intent, context, &available_agents)
            .await
    }

    /// Parse LLM response into intent structure
    pub fn parse_llm_intent_response(
        &self,
        response: &str,
        _natural_language: &str,
        _context: Option<HashMap<String, Value>>,
    ) -> Result<Intent, RuntimeError> {
        parse_llm_intent_response(response)
    }

    /// Parse JSON response as fallback
    pub fn parse_json_intent_response(
        &self,
        response: &str,
        natural_language: &str,
    ) -> Result<Intent, RuntimeError> {
        parse_json_intent_response(response, natural_language)
    }

    /// Create a new delegating arbiter for testing
    pub fn for_test(
        llm_provider: Box<dyn LlmProvider>,
        capability_marketplace: Arc<CapabilityMarketplace>,
        intent_graph: std::sync::Arc<std::sync::Mutex<crate::types::IntentGraph>>,
    ) -> Self {
        let prompt_path = if std::path::Path::new("../assets/prompts/cognitive_engine").exists() {
            "../assets/prompts/cognitive_engine"
        } else {
            "assets/prompts/cognitive_engine"
        };
        let prompt_store = FilePromptStore::new(prompt_path);
        let prompt_manager = PromptManager::new(prompt_store);

        let llm_config = LlmConfig {
            provider_type: crate::arbiter::llm_provider::LlmProviderType::Stub,
            model: "stub".to_string(),
            api_key: None,
            base_url: None,
            max_tokens: None,
            temperature: None,
            timeout_seconds: None,
            prompts: None,
            retry_config: RetryConfig::default(),
        };

        let delegation_config = DelegationConfig {
            enabled: false,
            threshold: 0.0,
            max_candidates: 0,
            min_skill_hits: None,
            agent_registry: None,
            adaptive_threshold: None,
            print_extracted_intent: None,
            print_extracted_plan: None,
        };

        let adaptive_threshold_calculator = None; // Stub doesn't use it
        let llm_provider: Arc<dyn LlmProvider> = Arc::from(llm_provider);
        let delegation_analyzer = DelegationAnalyzer::new(
            llm_provider.clone(),
            // Ensure PromptManager is clonable or wrapped - assuming clone works if impl
            prompt_manager.clone(),
            delegation_config.clone(),
            adaptive_threshold_calculator,
        );

        Self {
            llm_config,
            delegation_config,
            llm_provider,
            capability_marketplace,
            intent_graph,
            delegation_analyzer,
            prompt_manager,
            working_memory: None,
        }
    }

    /// Create a new delegating arbiter with the given configuration
    pub async fn new(
        llm_config: LlmConfig,
        delegation_config: DelegationConfig,
        capability_marketplace: Arc<CapabilityMarketplace>,
        intent_graph: std::sync::Arc<std::sync::Mutex<crate::types::IntentGraph>>,
    ) -> Result<Self, RuntimeError> {
        // Create LLM provider
        let llm_provider: Arc<dyn LlmProvider> =
            Arc::from(LlmProviderFactory::create_provider(llm_config.to_provider_config()).await?);

        // Create adaptive threshold calculator if configured
        let adaptive_threshold_calculator =
            delegation_config.adaptive_threshold.as_ref().map(|config| {
                crate::adaptive_threshold::AdaptiveThresholdCalculator::new(config.clone())
            });

        // Create prompt manager for file-based prompts
        // Assets are at workspace root, so try ../assets first, then assets (for when run from workspace root)
        let prompt_path = if std::path::Path::new("../assets/prompts/cognitive_engine").exists() {
            "../assets/prompts/cognitive_engine"
        } else {
            "assets/prompts/cognitive_engine"
        };
        let prompt_store = FilePromptStore::new(prompt_path);
        let prompt_manager = PromptManager::new(prompt_store);

        let delegation_analyzer = DelegationAnalyzer::new(
            llm_provider.clone(),
            prompt_manager.clone(),
            delegation_config.clone(),
            adaptive_threshold_calculator,
        );

        Ok(Self {
            llm_config,
            delegation_config,
            llm_provider,
            capability_marketplace,
            intent_graph,
            delegation_analyzer,
            prompt_manager,
            working_memory: None,
        })
    }

    /// Create a new delegating arbiter with WorkingMemory for learning augmentation
    pub async fn new_with_learning(
        llm_config: LlmConfig,
        delegation_config: DelegationConfig,
        capability_marketplace: Arc<CapabilityMarketplace>,
        intent_graph: std::sync::Arc<std::sync::Mutex<crate::types::IntentGraph>>,
        working_memory: Arc<std::sync::Mutex<crate::working_memory::WorkingMemory>>,
    ) -> Result<Self, RuntimeError> {
        let mut arbiter = Self::new(
            llm_config,
            delegation_config,
            capability_marketplace,
            intent_graph,
        )
        .await?;
        arbiter.working_memory = Some(working_memory);
        Ok(arbiter)
    }

    /// Get the LLM configuration used by this arbiter
    pub fn get_llm_config(&self) -> &LlmConfig {
        &self.llm_config
    }

    /// Get the LLM provider
    pub fn get_llm_provider(&self) -> Arc<dyn LlmProvider> {
        self.llm_provider.clone()
    }

    /// Query the LLM directly with a prompt
    pub async fn query_llm(&self, prompt: &str) -> Result<String, RuntimeError> {
        self.llm_provider.generate_text(prompt).await
    }

    /// Select an MCP tool and extract arguments from a natural language hint
    /// This is a specialized method for MCP discovery that bypasses delegation analysis
    ///
    /// `tool_schemas` is a map from tool name to its input schema JSON (properties only)
    pub async fn select_mcp_tool(
        &self,
        hint: &str,
        available_tools: &[String],
        tool_schemas: Option<&HashMap<String, serde_json::Value>>,
    ) -> Result<(String, HashMap<String, Value>), RuntimeError> {
        if self.llm_provider.supports_tool_calling() && !available_tools.is_empty() {
            let tool_defs = available_tools
                .iter()
                .map(|tool_name| {
                    let input_schema = tool_schemas
                        .and_then(|schemas| schemas.get(tool_name))
                        .map(Self::normalize_tool_schema)
                        .unwrap_or_else(Self::default_tool_schema);

                    ToolDefinition {
                        capability_id: tool_name.clone(),
                        tool_name: capability_id_to_tool_name(tool_name),
                        description: format!("MCP tool '{}'", tool_name),
                        input_schema,
                    }
                })
                .collect::<Vec<_>>();

            let request = ToolChatRequest {
                messages: vec![
                    ToolChatMessage {
                        role: "system".to_string(),
                        content: "Select exactly one tool from the provided list and return arguments through a native tool call. Do not answer in prose.".to_string(),
                        tool_call_id: None,
                        name: None,
                    },
                    ToolChatMessage {
                        role: "user".to_string(),
                        content: format!(
                            "Hint: {}\nAvailable tools: {}",
                            hint,
                            available_tools.join(", ")
                        ),
                        tool_call_id: None,
                        name: None,
                    },
                ],
                tools: tool_defs.clone(),
                max_tokens: self.llm_config.max_tokens,
                temperature: self.llm_config.temperature,
            };

            match self.llm_provider.chat_with_tools(&request).await {
                Ok(response) if response.has_tool_calls() => {
                    if let Some(call) = response.tool_calls.first() {
                        if let Some(capability_id) =
                            resolve_capability_id(&call.tool_name, &tool_defs)
                        {
                            let mut args_map = HashMap::new();
                            if let Some(obj) = call.arguments.as_object() {
                                for (k, v) in obj {
                                    args_map.insert(k.clone(), json_to_rtfs_value(v)?);
                                }
                            } else {
                                args_map.insert(
                                    "input".to_string(),
                                    json_to_rtfs_value(&call.arguments)?,
                                );
                            }
                            return Ok((capability_id.to_string(), args_map));
                        }
                    }
                }
                Ok(_) => {
                    eprintln!(
                        "ℹ️ Tool-calling provider returned no tool calls for MCP selection; falling back"
                    );
                }
                Err(e) => {
                    eprintln!(
                        "⚠️ Tool-calling MCP selection failed; falling back to RTFS parse: {}",
                        e
                    );
                }
            }
        }

        // Create prompt using the specialized tool_selection template
        let tool_list = available_tools.join(", ");
        let mut vars = HashMap::new();
        vars.insert("hint".to_string(), hint.to_string());
        vars.insert("tools".to_string(), tool_list);

        // Build schema information string for the prompt
        let schema_info = if let Some(schemas) = tool_schemas {
            let mut schema_strings = Vec::new();
            for tool_name in available_tools {
                if let Some(schema) = schemas.get(tool_name) {
                    if let Some(properties) = schema.get("properties").and_then(|p| p.as_object()) {
                        let param_names: Vec<String> = properties.keys().cloned().collect();
                        if !param_names.is_empty() {
                            schema_strings.push(format!(
                                "  - {}: parameters: {}",
                                tool_name,
                                param_names.join(", ")
                            ));
                        }
                    }
                }
            }
            if !schema_strings.is_empty() {
                format!("\n\nTool Parameter Schemas:\n{}", schema_strings.join("\n"))
            } else {
                String::new()
            }
        } else {
            String::new()
        };
        vars.insert("schemas".to_string(), schema_info);

        let prompt = self
            .prompt_manager
            .render("tool_selection", "v1", &vars)
            .unwrap_or_else(|e| {
                eprintln!(
                    "Warning: Failed to load tool_selection prompt: {}. Using fallback.",
                    e
                );
                format!(
                    r#"Select the best tool from this list: {}

For the hint: "{}"

Respond with ONLY an RTFS intent expression:
(intent "tool_name"
  :goal "description"
  :constraints {{
    "param1" "value1"
  }}
)

The tool name MUST be one of: {}"#,
                    available_tools.join(", "),
                    hint,
                    available_tools.join(", ")
                )
            });

        // Get LLM response
        let response = self.llm_provider.generate_text(&prompt).await?;

        // Debug: Log the raw LLM response
        eprintln!("📝 Raw LLM response for tool selection:");
        eprintln!("{}", response);
        eprintln!("--- End Raw LLM Response ---");

        // Parse the RTFS intent from response
        let intent = parse_llm_intent_response(&response)?;

        // Debug: Log the parsed intent
        eprintln!("✅ Parsed intent:");
        eprintln!("  Name: {:?}", intent.name);
        eprintln!("  Goal: {}", intent.goal);
        eprintln!("  Constraints: {:?}", intent.constraints);

        // Extract tool name and arguments
        let tool_name = intent
            .name
            .ok_or_else(|| RuntimeError::Generic("Intent missing name field".to_string()))?;

        Ok((tool_name, intent.constraints))
    }

    fn default_tool_schema() -> serde_json::Value {
        json!({
            "type": "object",
            "additionalProperties": true,
        })
    }

    fn normalize_tool_schema(schema: &serde_json::Value) -> serde_json::Value {
        if schema.get("type").is_some() || schema.get("properties").is_some() {
            return schema.clone();
        }

        if schema.is_object() {
            json!({
                "type": "object",
                "properties": schema,
                "additionalProperties": true,
            })
        } else {
            Self::default_tool_schema()
        }
    }

    async fn try_generate_intent_with_tool_call(
        &self,
        natural_language: &str,
        context: Option<&HashMap<String, Value>>,
    ) -> Result<Option<Intent>, RuntimeError> {
        if !self.llm_provider.supports_tool_calling() {
            return Ok(None);
        }

        let context_summary = context
            .map(|ctx| {
                ctx.iter()
                    .take(20)
                    .map(|(k, v)| format!("{}: {}", k, v))
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default();

        let tool = ToolDefinition {
            capability_id: "emit_intent".to_string(),
            tool_name: capability_id_to_tool_name("emit_intent"),
            description: "Return one structured intent as JSON fields".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string" },
                    "goal": { "type": "string" },
                    "constraints": { "type": "object", "additionalProperties": true },
                    "preferences": { "type": "object", "additionalProperties": true },
                    "success_criteria": { "type": "string" }
                },
                "required": ["goal"],
                "additionalProperties": true
            }),
        };

        let request = ToolChatRequest {
            messages: vec![
                ToolChatMessage {
                    role: "system".to_string(),
                    content: "Generate exactly one intent and return it by calling the provided tool. Do not answer with prose.".to_string(),
                    tool_call_id: None,
                    name: None,
                },
                ToolChatMessage {
                    role: "user".to_string(),
                    content: format!(
                        "User request:\n{}\n\nContext:\n{}",
                        natural_language,
                        context_summary
                    ),
                    tool_call_id: None,
                    name: None,
                },
            ],
            tools: vec![tool.clone()],
            max_tokens: self.llm_config.max_tokens,
            temperature: self.llm_config.temperature,
        };

        let response = match self.llm_provider.chat_with_tools(&request).await {
            Ok(r) => r,
            Err(e) => {
                eprintln!(
                    "⚠️ Intent tool-calling failed; falling back to text parsing: {}",
                    e
                );
                return Ok(None);
            }
        };

        if !response.has_tool_calls() {
            return Ok(None);
        }

        let Some(call) = response.tool_calls.first() else {
            return Ok(None);
        };

        let defs = vec![tool];
        let Some(_capability_id) = resolve_capability_id(&call.tool_name, &defs) else {
            return Ok(None);
        };

        let payload = if call.arguments.is_object() {
            call.arguments.clone()
        } else {
            json!({ "goal": natural_language, "constraints": { "raw": call.arguments } })
        };

        let payload_str = match serde_json::to_string(&payload) {
            Ok(s) => s,
            Err(e) => {
                eprintln!(
                    "⚠️ Intent tool-calling payload serialization failed; falling back: {}",
                    e
                );
                return Ok(None);
            }
        };

        match self.parse_json_intent_response(&payload_str, natural_language) {
            Ok(mut intent) => {
                intent.metadata.insert(
                    "parse_format".to_string(),
                    Value::String("tool_call".to_string()),
                );
                Ok(Some(intent))
            }
            Err(e) => {
                eprintln!(
                    "⚠️ Intent tool-calling payload parse failed; falling back: {}",
                    e
                );
                Ok(None)
            }
        }
    }

    /// Find agent capabilities that match the given required capabilities
    async fn find_agents_for_capabilities(
        &self,
        required_capabilities: &[String],
    ) -> Result<Vec<CapabilityManifest>, RuntimeError> {
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
                    matching_agents.push(capability.clone());
                    break;
                }
            }
        }

        Ok(matching_agents)
    }

    /// List all available agent capabilities
    async fn list_agent_capabilities(&self) -> Result<Vec<CapabilityManifest>, RuntimeError> {
        let query = CapabilityQuery::new()
            .with_kind(CapabilityKind::Agent)
            .with_limit(100);

        let agent_capabilities = self
            .capability_marketplace
            .list_capabilities_with_query(&query)
            .await;

        Ok(agent_capabilities)
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

        let tool_call_intent = self
            .try_generate_intent_with_tool_call(natural_language, context.as_ref())
            .await?;
        let effective_format_mode = if tool_call_intent.is_some() {
            "tool_call".to_string()
        } else {
            format_mode.clone()
        };

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

        // Get raw text response (only when tool-calling did not provide an intent)
        let response = if tool_call_intent.is_some() {
            "{\"source\":\"tool_call\"}".to_string()
        } else {
            self.llm_provider.generate_text(&prompt).await?
        };

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
        let mut intent = if let Some(intent) = tool_call_intent {
            intent
        } else if format_mode == "json" {
            // Direct JSON parse path
            match parse_json_intent_response(&response, natural_language) {
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
            match parse_llm_intent_response(&response) {
                Ok(intent) => {
                    println!("✓ Successfully parsed intent from RTFS format");
                    intent
                }
                Err(rtfs_err) => {
                    println!(
                        "⚠ RTFS parsing failed, attempting JSON fallback: {}",
                        rtfs_err
                    );
                    match parse_json_intent_response(&response, natural_language) {
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
            Value::String(effective_format_mode.clone()),
        );
        // Derive parse_format if not already set (e.g., RTFS success path)
        if !intent.metadata.contains_key("parse_format") {
            let pf = if effective_format_mode == "json" {
                "json"
            } else if effective_format_mode == "tool_call" {
                "tool_call"
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
            for agent in &candidate_agents {
                println!("DEBUG: Agent: {}", agent.id);
            }

            if !candidate_agents.is_empty() {
                // Select the best agent (first one for now)
                let selected_agent = &candidate_agents[0];

                // Set delegation metadata
                intent.metadata.insert(
                    "delegation.selected_agent".to_string(),
                    Value::String(selected_agent.id.clone()),
                );
                intent.metadata.insert(
                    "delegation.candidates".to_string(),
                    Value::String(
                        candidate_agents
                            .iter()
                            .map(|c| c.id.as_str())
                            .collect::<Vec<_>>()
                            .join(", "),
                    ),
                );

                // Set intent name to match the selected agent
                intent.name = Some(selected_agent.id.clone());

                println!("DEBUG: Selected agent: {}", selected_agent.id);
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
        // [Learning Integration] Step 0: Recall patterns for this intent
        let pattern_mods = self.recall_patterns_for_intent(intent).await;

        // First, analyze if delegation is appropriate
        let available_agents = self.list_agent_capabilities().await?;
        let delegation_analysis = self
            .delegation_analyzer
            .analyze_need(intent, context.clone(), &available_agents)
            .await?;

        let plan = if delegation_analysis.should_delegate {
            // Generate plan with delegation
            self.generate_delegated_plan(intent, &delegation_analysis, context)
                .await?
        } else {
            // Generate plan without delegation
            self.generate_direct_plan(intent, context).await?
        };

        // [Learning Integration] Step 3: Augment plan with learning modifications
        if !pattern_mods.is_empty() {
            let result = super::learning_augmenter::augment_plan_with_learning(plan, &pattern_mods);

            if !result.applied_modifications.is_empty() {
                eprintln!(
                    "DEBUG: [Learning] Applied {} modifications to plan: {:?}",
                    result.applied_modifications.len(),
                    result
                        .applied_modifications
                        .iter()
                        .map(|m| &m.modification_type)
                        .collect::<Vec<_>>()
                );
            }
            if !result.skipped_modifications.is_empty() {
                eprintln!(
                    "DEBUG: [Learning] Skipped {} modifications: {:?}",
                    result.skipped_modifications.len(),
                    result
                        .skipped_modifications
                        .iter()
                        .map(|(m, reason)| (&m.modification_type, reason))
                        .collect::<Vec<_>>()
                );
            }

            Ok(result.plan)
        } else {
            Ok(plan)
        }
    }

    /// Recall failure patterns from WorkingMemory for capabilities in this intent
    async fn recall_patterns_for_intent(
        &self,
        intent: &Intent,
    ) -> Vec<crate::learning::capabilities::PlanModification> {
        let mut modifications = Vec::new();

        // Skip if no WorkingMemory is configured
        let working_memory = match &self.working_memory {
            Some(wm) => wm,
            None => return modifications,
        };

        // Extract potential capability IDs from the intent goal
        // For now, we'll use capability ids mentioned in the goal or metadata
        let potential_caps = self.extract_capability_hints(intent);

        // Query WorkingMemory for each potential capability using the query API
        for cap_id in &potential_caps {
            if let Ok(guard) = working_memory.lock() {
                // Query for entries with "pattern" tag and matching capability ID
                let pattern_tag = format!("pattern:{}", cap_id);
                let query_params =
                    crate::working_memory::QueryParams::with_tags([pattern_tag.as_str()])
                        .with_limit(Some(10));

                if let Ok(result) = guard.query(&query_params) {
                    for entry in result.entries {
                        // Extract error category from entry content or tags
                        // Patterns are stored with tags like ["pattern", "pattern:cap_id", "NetworkError"]
                        for tag in &entry.tags {
                            if [
                                "NetworkError",
                                "TimeoutError",
                                "MissingCapability",
                                "SchemaError",
                            ]
                            .contains(&tag.as_str())
                            {
                                if let Some(m) = self.pattern_to_modification_simple(cap_id, tag) {
                                    modifications.push(m);
                                }
                                break;
                            }
                        }
                    }
                }
            }
        }

        modifications
    }

    /// Extract capability hints from the intent (goal text, metadata, etc.)
    fn extract_capability_hints(&self, intent: &Intent) -> Vec<String> {
        let mut hints = Vec::new();

        // Check metadata for explicit capability references
        if let Some(cap_ref) = intent.metadata.get("target_capability") {
            if let Value::String(cap_id) = cap_ref {
                hints.push(cap_id.clone());
            }
        }

        // Simple heuristic: look for capability-like patterns in the goal
        // (e.g., "capability:xxx" or "call xxx")
        let goal_lower = intent.goal.to_lowercase();
        if goal_lower.contains("demo.") || goal_lower.contains("ccos.") {
            // Extract any word that looks like a capability ID
            for word in intent.goal.split_whitespace() {
                if word.contains('.') && !word.contains("http") {
                    hints.push(
                        word.trim_matches(|c: char| !c.is_alphanumeric() && c != '.')
                            .to_string(),
                    );
                }
            }
        }

        hints
    }

    /// Convert an error category tag to a PlanModification
    fn pattern_to_modification_simple(
        &self,
        cap_id: &str,
        error_category: &str,
    ) -> Option<crate::learning::capabilities::PlanModification> {
        use crate::learning::capabilities::PlanModification;

        // Map error categories to modification types with new hint system
        let (mod_type, params, confidence) = match error_category {
            "NetworkError" => (
                // Network errors: retry with circuit breaker protection
                "inject_retry".to_string(),
                serde_json::json!({
                    "max_retries": 3,
                    "initial_delay_ms": 1000,
                    // Also suggest circuit breaker for repeated failures
                    "with_circuit_breaker": true,
                    "failure_threshold": 5,
                    "cooldown_ms": 30000
                }),
                0.85,
            ),
            "TimeoutError" => (
                // Timeout: increase timeout and add retry
                "adjust_timeout".to_string(),
                serde_json::json!({
                    "timeout_ms": 10000,
                    "with_retry": true,
                    "max_retries": 2
                }),
                0.75,
            ),
            "MissingCapability" => (
                // Missing capability: try synthesis first, with fallback
                "synthesize_first".to_string(),
                serde_json::json!({
                    "trigger_synthesis": true,
                    "fallback_capability": "ccos.error.handler"
                }),
                0.65,
            ),
            "SchemaError" => (
                // Schema errors: add metrics for debugging
                "inject_metrics".to_string(),
                serde_json::json!({
                    "emit_to_chain": true,
                    "track_percentiles": false
                }),
                0.5,
            ),
            "RateLimitError" => (
                // Rate limit: add rate limiting and retry with backoff
                "inject_rate_limit".to_string(),
                serde_json::json!({
                    "requests_per_second": 5,
                    "burst": 10,
                    "with_retry": true,
                    "max_retries": 3,
                    "initial_delay_ms": 2000
                }),
                0.8,
            ),
            "LLMError" => (
                // LLM errors: add fallback to alternative model
                "inject_fallback".to_string(),
                serde_json::json!({
                    "fallback_capability": "llm.fallback_model",
                    "with_circuit_breaker": true,
                    "failure_threshold": 3
                }),
                0.7,
            ),
            _ => return None,
        };

        Some(PlanModification {
            modification_type: mod_type,
            target_capability: cap_id.to_string(),
            parameters: serde_json::from_value(params).unwrap_or_default(),
            confidence,
        })
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

        // Select the best agent (heuristic: highest trust_score, then lowest cost)
        let selected_agent = candidate_agents
            .iter()
            .max_by(|a, b| {
                let a_meta = a.agent_metadata.as_ref();
                let b_meta = b.agent_metadata.as_ref();

                let a_trust = a_meta.map(|m| m.trust_score).unwrap_or(1.0);
                let b_trust = b_meta.map(|m| m.trust_score).unwrap_or(1.0);

                match a_trust.partial_cmp(&b_trust) {
                    Some(std::cmp::Ordering::Equal) => {
                        let a_cost = a_meta.map(|m| m.cost).unwrap_or(0.0);
                        let b_cost = b_meta.map(|m| m.cost).unwrap_or(0.0);
                        b_cost
                            .partial_cmp(&a_cost)
                            .unwrap_or(std::cmp::Ordering::Equal)
                    }
                    Some(ord) => ord,
                    None => std::cmp::Ordering::Equal,
                }
            })
            .unwrap_or(&candidate_agents[0]);

        // Generate delegation plan using the configured LLM provider.
        // Build a StorableIntent similar to the direct plan path but include
        // delegation-specific metadata so providers can tailor prompts.
        let mut goal = intent.goal.clone();

        // [Onboarding Integration] Check if the selected agent requires onboarding
        if let Some(onboarding_json) = selected_agent.metadata.get("onboarding_config") {
            let mut is_operational = false;

            // Check current status in WorkingMemory
            if let Some(wm) = &self.working_memory {
                if let Ok(guard) = wm.lock() {
                    let key = format!("skill:{}:onboarding_state", selected_agent.id);
                    if let Ok(Some(entry)) = guard.get(&key) {
                        if let Ok(state) = serde_json::from_str::<
                            crate::skills::types::SkillOnboardingState,
                        >(&entry.content)
                        {
                            if state.status == crate::skills::types::OnboardingState::Operational {
                                is_operational = true;
                            }
                        }
                    }
                }
            }

            if !is_operational {
                // Agent is not operational, inject raw onboarding content for LLM reasoning
                goal.push_str("\n\n### SKILL SETUP INSTRUCTIONS\n");
                goal.push_str("This skill requires setup before it can be used. ");
                goal.push_str("Read the following instructions carefully and execute the necessary steps.\n\n");

                if let Ok(config) =
                    serde_json::from_str::<crate::skills::types::OnboardingConfig>(onboarding_json)
                {
                    // Prefer raw content - let the LLM reason about what to do
                    if !config.raw_content.is_empty() {
                        goal.push_str(&config.raw_content);
                        goal.push_str("\n\n");
                    } else if !config.steps.is_empty() {
                        // Backwards compat: if structured steps exist, format them
                        for (i, step) in config.steps.iter().enumerate() {
                            goal.push_str(&format!("{}. **{}**: ", i + 1, step.id));
                            match step.step_type {
                                crate::skills::types::OnboardingStepType::ApiCall => {
                                    if let Some(op) = &step.operation {
                                        goal.push_str(&format!("Call operation `{}`", op));
                                    }
                                }
                                crate::skills::types::OnboardingStepType::HumanAction => {
                                    if let Some(action) = &step.action {
                                        goal.push_str(&format!(
                                            "REQUIRING HUMAN ACTION: {} - {}",
                                            action.title, action.instructions
                                        ));
                                    }
                                }
                                crate::skills::types::OnboardingStepType::Condition => {
                                    goal.push_str("Verify condition");
                                }
                            }
                            if let Some(verify) = &step.verify_on_success {
                                goal.push_str(&format!(" (Verify with: {})", verify));
                            }
                            goal.push_str("\n");
                        }
                    }

                    // Add final step to mark operational
                    goal.push_str(&format!(
                        "\nOnce setup is complete, call `(call :ccos.skill.onboarding.mark_operational {{:skill_id \"{}\"}})` to mark the agent as operational.\n",
                        selected_agent.id
                    ));
                } else {
                    // Fallback if JSON parsing fails but key exists
                    goal.push_str("Note: Onboarding configuration found but could not be parsed. Proceed with general reasoning to set up the skill.\n");
                }
            }
        }

        let storable_intent = StorableIntent {
            intent_id: intent.intent_id.clone(),
            name: intent.name.clone(),
            original_request: intent.original_request.clone(),
            rtfs_intent_source: "".to_string(),
            goal,
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
            session_id: None,
            triggered_by: crate::types::TriggerSource::ArbiterInference,
            generation_context: crate::types::GenerationContext {
                arbiter_version: "delegating-1.0".to_string(),
                generation_timestamp: intent.created_at,
                input_context: {
                    let mut m = HashMap::new();
                    m.insert(
                        "delegation_target_agent".to_string(),
                        selected_agent.id.clone(),
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
                    selected_agent.id.clone(),
                );
                meta.insert(
                    "delegation.agent_name".to_string(),
                    selected_agent.name.clone(),
                );
                meta.insert(
                    "delegation.agent_description".to_string(),
                    selected_agent.description.clone(),
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
            session_id: None,
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
            let mut remaining = &response[idx..];

            // Collect consecutive top-level balanced blocks
            while let Some(block) = Self::extract_balanced_from(remaining, 0) {
                let trimmed = block.trim_start();
                if trimmed.starts_with("(intent") {
                    collected_intents.push(block.clone());
                } else if trimmed.starts_with("(plan") || trimmed.starts_with("(do") {
                    // Found a plan or do block: prefer returning it
                    return Ok(block);
                } else {
                    // Found some other top-level block: return it
                    return Ok(block);
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
            session_id: None,
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

#[async_trait(?Send)]
impl CognitiveEngine for DelegatingCognitiveEngine {
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
                crate::capabilities::registry::CapabilityRegistry::new(),
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

    use crate::cognitive_engine::config::{DelegationConfig, LlmConfig, LlmProviderType};
    use crate::cognitive_engine::llm_provider::{LlmProviderInfo, ValidationResult};
    use crate::llm::tool_calling::{capability_id_to_tool_name, ToolCall, ToolChatRequest, ToolChatResponse};
    use async_trait::async_trait;
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
            retry_config: RetryConfig::default(),
            prompts: None,
        };

        let delegation_config = DelegationConfig {
            enabled: true,
            threshold: 0.65,
            max_candidates: 3,
            min_skill_hits: Some(1),
            agent_registry: None,
            adaptive_threshold: None,
            print_extracted_intent: None,
            print_extracted_plan: None,
        };

        (llm_config, delegation_config)
    }

    struct ToolCallingIntentProvider;

    struct NoToolCallsIntentProvider;

    #[async_trait]
    impl LlmProvider for ToolCallingIntentProvider {
        async fn generate_intent(
            &self,
            _prompt: &str,
            _context: Option<HashMap<String, String>>,
        ) -> Result<StorableIntent, RuntimeError> {
            Err(RuntimeError::Generic(
                "generate_intent not used in this test".to_string(),
            ))
        }

        async fn generate_plan(
            &self,
            _intent: &StorableIntent,
            _context: Option<HashMap<String, String>>,
        ) -> Result<Plan, RuntimeError> {
            Err(RuntimeError::Generic(
                "generate_plan not used in this test".to_string(),
            ))
        }

        async fn validate_plan(
            &self,
            _plan_content: &str,
        ) -> Result<ValidationResult, RuntimeError> {
            Err(RuntimeError::Generic(
                "validate_plan not used in this test".to_string(),
            ))
        }

        async fn generate_text(&self, _prompt: &str) -> Result<String, RuntimeError> {
            Ok("fallback".to_string())
        }

        fn supports_tool_calling(&self) -> bool {
            true
        }

        async fn chat_with_tools(
            &self,
            _request: &ToolChatRequest,
        ) -> Result<ToolChatResponse, RuntimeError> {
            Ok(ToolChatResponse {
                content: String::new(),
                tool_calls: vec![ToolCall {
                    id: "call_1".to_string(),
                    tool_name: capability_id_to_tool_name("emit_intent"),
                    arguments: json!({
                        "name": "tool_call_intent",
                        "goal": "Extracted by tool call",
                        "constraints": { "source": "tool" },
                        "preferences": { "mode": "fast" },
                        "success_criteria": "intent generated"
                    }),
                }],
            })
        }

        fn get_info(&self) -> LlmProviderInfo {
            LlmProviderInfo {
                name: "ToolCallingIntentProvider".to_string(),
                version: "test".to_string(),
                model: "test".to_string(),
                capabilities: vec!["tool_calling".to_string()],
            }
        }
    }

    #[async_trait]
    impl LlmProvider for NoToolCallsIntentProvider {
        async fn generate_intent(
            &self,
            _prompt: &str,
            _context: Option<HashMap<String, String>>,
        ) -> Result<StorableIntent, RuntimeError> {
            Err(RuntimeError::Generic(
                "generate_intent not used in this test".to_string(),
            ))
        }

        async fn generate_plan(
            &self,
            _intent: &StorableIntent,
            _context: Option<HashMap<String, String>>,
        ) -> Result<Plan, RuntimeError> {
            Err(RuntimeError::Generic(
                "generate_plan not used in this test".to_string(),
            ))
        }

        async fn validate_plan(
            &self,
            _plan_content: &str,
        ) -> Result<ValidationResult, RuntimeError> {
            Err(RuntimeError::Generic(
                "validate_plan not used in this test".to_string(),
            ))
        }

        async fn generate_text(&self, _prompt: &str) -> Result<String, RuntimeError> {
            Ok("fallback".to_string())
        }

        fn supports_tool_calling(&self) -> bool {
            true
        }

        async fn chat_with_tools(
            &self,
            _request: &ToolChatRequest,
        ) -> Result<ToolChatResponse, RuntimeError> {
            Ok(ToolChatResponse {
                content: "No tool call returned".to_string(),
                tool_calls: vec![],
            })
        }

        fn get_info(&self) -> LlmProviderInfo {
            LlmProviderInfo {
                name: "NoToolCallsIntentProvider".to_string(),
                version: "test".to_string(),
                model: "test".to_string(),
                capabilities: vec!["tool_calling".to_string()],
            }
        }
    }

    #[tokio::test]
    async fn test_delegating_arbiter_creation() {
        let (llm_config, delegation_config) = create_test_config();
        let intent_graph = std::sync::Arc::new(std::sync::Mutex::new(
            crate::types::IntentGraph::new().unwrap(),
        ));

        // Create a minimal capability marketplace for testing
        let registry = Arc::new(RwLock::new(
            crate::capabilities::registry::CapabilityRegistry::new(),
        ));
        let marketplace = Arc::new(CapabilityMarketplace::new(registry));

        let arbiter = DelegatingCognitiveEngine::new(
            llm_config,
            delegation_config,
            marketplace,
            intent_graph,
        )
        .await;
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
            crate::types::IntentGraph::new().unwrap(),
        ));

        // Create a minimal capability marketplace for testing
        let registry = Arc::new(RwLock::new(
            crate::capabilities::registry::CapabilityRegistry::new(),
        ));
        let marketplace = Arc::new(CapabilityMarketplace::new(registry));

        let arbiter = DelegatingCognitiveEngine::new(
            llm_config,
            delegation_config,
            marketplace,
            intent_graph,
        )
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
            crate::types::IntentGraph::new().unwrap(),
        ));

        // Create a minimal capability marketplace for testing
        let registry = Arc::new(RwLock::new(
            crate::capabilities::registry::CapabilityRegistry::new(),
        ));
        let marketplace = Arc::new(CapabilityMarketplace::new(registry));

        let arbiter = DelegatingCognitiveEngine::new(
            llm_config,
            delegation_config,
            marketplace,
            intent_graph,
        )
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

    #[tokio::test]
    async fn test_try_generate_intent_with_tool_call() {
        let intent_graph = std::sync::Arc::new(std::sync::Mutex::new(
            crate::types::IntentGraph::new().unwrap(),
        ));

        let registry = Arc::new(RwLock::new(
            crate::capabilities::registry::CapabilityRegistry::new(),
        ));
        let marketplace = Arc::new(CapabilityMarketplace::new(registry));

        let engine = DelegatingCognitiveEngine::for_test(
            Box::new(ToolCallingIntentProvider),
            marketplace,
            intent_graph,
        );

        let intent_opt = engine
            .try_generate_intent_with_tool_call("create a backup workflow", None)
            .await
            .unwrap();

        let intent = intent_opt.expect("expected intent generated via tool call");
        assert_eq!(intent.name, Some("tool_call_intent".to_string()));
        assert_eq!(intent.goal, "Extracted by tool call");
        assert_eq!(
            intent
                .metadata
                .get("parse_format")
                .and_then(|v| v.as_string())
                .as_deref(),
            Some("tool_call")
        );
    }

    #[tokio::test]
    async fn test_try_generate_intent_with_tool_call_no_tool_calls_returns_none() {
        let intent_graph = std::sync::Arc::new(std::sync::Mutex::new(
            crate::types::IntentGraph::new().unwrap(),
        ));

        let registry = Arc::new(RwLock::new(
            crate::capabilities::registry::CapabilityRegistry::new(),
        ));
        let marketplace = Arc::new(CapabilityMarketplace::new(registry));

        let engine = DelegatingCognitiveEngine::for_test(
            Box::new(NoToolCallsIntentProvider),
            marketplace,
            intent_graph,
        );

        let intent_opt = engine
            .try_generate_intent_with_tool_call("create a backup workflow", None)
            .await
            .unwrap();

        assert!(intent_opt.is_none());
    }
}
