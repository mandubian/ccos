use super::config::DelegationConfig;
use super::intent_parsing::extract_all_json_from_response;
use super::llm_provider::LlmProvider;
use super::prompt::{FilePromptStore, PromptManager};
use crate::adaptive_threshold::AdaptiveThresholdCalculator;
use crate::types::Intent;
use crate::capability_marketplace::types::CapabilityManifest;
use rtfs::runtime::error::RuntimeError;
use rtfs::runtime::values::Value;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

/// Analysis result for delegation decision
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DelegationAnalysis {
    /// Whether the task should be delegated
    pub should_delegate: bool,
    /// Reasoning for the decision
    pub reasoning: String,
    /// Capabilities required for the task
    pub required_capabilities: Vec<String>,
    /// Confidence score for delegation (0.0-1.0)
    pub delegation_confidence: f64,
}

pub struct DelegationAnalyzer {
    llm_provider: Arc<dyn LlmProvider>,
    prompt_manager: PromptManager<FilePromptStore>,
    config: DelegationConfig,
    adaptive_threshold_calculator: Option<AdaptiveThresholdCalculator>,
}

impl DelegationAnalyzer {
    pub fn new(
        llm_provider: Arc<dyn LlmProvider>,
        prompt_manager: PromptManager<FilePromptStore>,
        config: DelegationConfig,
        adaptive_threshold_calculator: Option<AdaptiveThresholdCalculator>,
    ) -> Self {
        Self {
            llm_provider,
            prompt_manager,
            config,
            adaptive_threshold_calculator,
        }
    }

    /// Analyze whether delegation is needed for this intent
    pub async fn analyze_need(
        &self,
        intent: &Intent,
        context: Option<HashMap<String, Value>>,
        available_agents: &[CapabilityManifest],
    ) -> Result<DelegationAnalysis, RuntimeError> {
        // Debug: Log the intent being analyzed
        eprintln!(
            "DEBUG: Analyzing delegation for intent: name={:?}, goal='{}'",
            intent.name, intent.goal
        );

        let prompt = self
            .create_delegation_analysis_prompt(intent, context, available_agents)
            .await?;

        // Debug: Log the prompt being sent (first 500 chars)
        let prompt_preview = if prompt.len() > 500 {
            format!("{}...", &prompt[..500])
        } else {
            prompt.clone()
        };
        eprintln!(
            "DEBUG: Delegation analysis prompt preview: {}",
            prompt_preview
        );

        let response = self.llm_provider.generate_text(&prompt).await?;

        // Parse delegation analysis
        let mut analysis = self.parse_delegation_analysis(&response)?;

        // Apply adaptive threshold if configured
        if let Some(calculator) = &self.adaptive_threshold_calculator {
            // Get base threshold from config
            let base_threshold = self.config.threshold;

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

    /// Create prompt for delegation analysis using file-based prompt store
    async fn create_delegation_analysis_prompt(
        &self,
        intent: &Intent,
        context: Option<HashMap<String, Value>>,
        available_agents: &[CapabilityManifest],
    ) -> Result<String, RuntimeError> {
        let agent_list = available_agents
            .iter()
            .map(|manifest| {
                format!(
                    "- {}: {} (ID: {})",
                    manifest.name, manifest.description, manifest.id
                )
            })
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

        Ok(self
            .prompt_manager
            .render("delegation_analysis", "v1", &vars)
            .unwrap_or_else(|e| {
                eprintln!(
                    "Warning: Failed to load delegation analysis prompt from assets: {}. Using fallback.",
                    e
                );
                self.create_fallback_delegation_prompt(
                    intent,
                    context_for_fallback,
                    &agent_list_for_fallback,
                )
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
{}

## Your JSON Response:"#,
            serde_json::to_string(&intent).unwrap_or_else(|_| "{}".to_string()),
            serde_json::to_string(&context.unwrap_or_default())
                .unwrap_or_else(|_| "{}".to_string()),
            agent_list
        )
    }

    /// Parse delegation analysis response with robust error handling
    fn parse_delegation_analysis(
        &self,
        response: &str,
    ) -> Result<DelegationAnalysis, RuntimeError> {
        // Log the raw response
        println!("Raw delegation analysis response: {}", response);
        // Clean the response - remove any leading/trailing whitespace and extract JSON
        let json_blobs = extract_all_json_from_response(response);

        if json_blobs.is_empty() {
            return Err(RuntimeError::Generic(
                "No JSON found in delegation analysis response".to_string(),
            ));
        }

        // Try to parse the last JSON blob
        let last_blob = json_blobs.last().unwrap();
        let json_response: serde_json::Value = serde_json::from_str(last_blob).map_err(|e| {
            // Generate user-friendly error message with full response preview
            let response_preview = if response.len() > 500 {
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

            let cleaned_preview = if last_blob.len() > 400 {
                format!(
                    "{}...\n[truncated, total length: {} chars]",
                    &last_blob[..400],
                    last_blob.len()
                )
            } else {
                last_blob.clone()
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

    /// Record feedback for delegation performance
    pub fn record_delegation_feedback(&mut self, agent_id: &str, success: bool) {
        if let Some(calculator) = &mut self.adaptive_threshold_calculator {
            calculator.update_performance(agent_id, success);
        }
    }

    /// Get adaptive threshold for a specific agent
    pub fn get_adaptive_threshold(&self, agent_id: &str) -> Option<f64> {
        if let Some(calculator) = &self.adaptive_threshold_calculator {
            let base_threshold = self.config.threshold;
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
}
