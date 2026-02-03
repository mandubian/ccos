//! LLM Integration for CCOS Agent
//!
//! Provides intelligent message processing using LLM providers.
//! The Agent uses this to understand user intent and plan capability execution.

use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::{info, warn};

/// LLM configuration for the agent
#[derive(Debug, Clone)]
pub struct LlmConfig {
    pub provider: String,
    pub api_key: String,
    pub model: String,
    pub base_url: Option<String>,
}

/// LLM client for the agent
pub struct AgentLlmClient {
    config: LlmConfig,
    client: Client,
}

/// A planned action to execute
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PlannedAction {
    pub capability_id: String,
    pub reasoning: String,
    pub inputs: serde_json::Value,
}

/// The agent's plan for processing a message
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AgentPlan {
    pub understanding: String,
    pub actions: Vec<PlannedAction>,
    pub response: String,
}

impl AgentLlmClient {
    /// Create a new LLM client
    pub fn new(config: LlmConfig) -> anyhow::Result<Self> {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .build()?;

        Ok(Self { config, client })
    }

    /// Process a user message and generate a plan
    pub async fn process_message(
        &self,
        message: &str,
        context: &[String],
        capabilities: &[String],
    ) -> anyhow::Result<AgentPlan> {
        match self.config.provider.as_str() {
            "openai" | "openrouter" | "google" | "gemini" => {
                self.process_with_openai(message, context, capabilities)
                    .await
            }
            "anthropic" => {
                self.process_with_anthropic(message, context, capabilities)
                    .await
            }
            _ => {
                // Fallback to simple echo for testing
                Ok(AgentPlan {
                    understanding: format!("User said: {}", message),
                    actions: vec![],
                    response: format!("Echo: {}", message),
                })
            }
        }
    }

    /// Process using OpenAI-compatible API
    async fn process_with_openai(
        &self,
        message: &str,
        _context: &[String], // context handling to be added
        capabilities: &[String],
    ) -> anyhow::Result<AgentPlan> {
        let base_url = self
            .config
            .base_url
            .as_ref()
            .map(|u| u.trim_end_matches('/').to_string())
            .unwrap_or_else(|| {
                if self.config.provider == "google" || self.config.provider == "gemini" {
                    "https://generativelanguage.googleapis.com/v1beta/openai".to_string()
                } else {
                    "https://api.openai.com/v1".to_string()
                }
            });

        let url = format!("{}/chat/completions", base_url);

        let caps_list = if capabilities.is_empty() {
            "- ccos.chat.egress.* - Send outbound messages\n- ccos.skill.* - Load and execute skills".to_string()
        } else {
            capabilities.join("\n")
        };

        let system_prompt = format!(
            r#"You are a CCOS agent. Your job is to:
1. Understand the user's message
2. Plan which capabilities to execute
3. Provide a helpful response

IMPORTANT: You receive the ACTUAL message content directly, not UUID pointers. The Gateway has already resolved any quarantine references. Work with the message content provided.

When working with skills:
- Use ccos.skill.load with: {{ "url": "..." }} to load skill definitions.
- Once loaded, the skill_definition will contain onboarding steps and available operations.
- Execute each onboarding step using ccos.skill.execute with: {{ "skill": "skill_id", "operation": "operation_name", "params": {{...}} }}.
- PLAN AND EXECUTE ALL onboarding steps until the skill is fully operational.

You have access to these capabilities:
{}

Respond in JSON format:
{{
  "understanding": "brief description of what user wants",
  "actions": [
    {{
      "capability_id": "capability.name",
      "reasoning": "why this capability",
      "inputs": {{ "param": "value" }}
    }}
  ],
  "response": "natural language response to user"
}}"#,
            caps_list
        );

        let request_body = json!({
            "model": self.config.model,
            "messages": [
                {
                    "role": "system",
                    "content": system_prompt
                },
                {
                    "role": "user",
                    "content": message
                }
            ],
            "temperature": 0.7,
            "max_tokens": 1000
        });

        // Debug: log system prompt length
        info!(
            "Sending to LLM - System prompt length: {} chars, User message: {}",
            system_prompt.len(),
            message
        );

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await?;

        if !response.status().is_success() {
            let error_text = response.text().await?;
            anyhow::bail!("OpenAI API error: {}", error_text);
        }

        let response_json: serde_json::Value = response.json().await?;
        let content = response_json["choices"][0]["message"]["content"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Invalid response format"))?
            .to_string();

        // Strip markdown code blocks if present
        let json_text = if content.trim().starts_with("```") {
            let lines: Vec<&str> = content.lines().collect();
            if lines.len() >= 2
                && lines.first().unwrap().starts_with("```")
                && lines.last().unwrap().starts_with("```")
            {
                lines[1..lines.len() - 1].join("\n")
            } else {
                content.clone()
            }
        } else {
            content.clone()
        };

        // Try to parse as JSON, fall back to simple structure
        match serde_json::from_str::<AgentPlan>(&json_text) {
            Ok(plan) => {
                info!("LLM generated plan with {} actions", plan.actions.len());
                Ok(plan)
            }
            Err(e) => {
                warn!("Failed to parse LLM response as JSON: {}", e);
                // Fallback: treat entire response as the response text
                Ok(AgentPlan {
                    understanding: "Direct response".to_string(),
                    actions: vec![],
                    response: content,
                })
            }
        }
    }

    /// Process using Anthropic API
    async fn process_with_anthropic(
        &self,
        message: &str,
        _context: &[String],
        capabilities: &[String],
    ) -> anyhow::Result<AgentPlan> {
        let url = "https://api.anthropic.com/v1/messages";

        let caps_list = if capabilities.is_empty() {
            "- ccos.chat.egress.* - Send outbound messages\n- ccos.skill.* - Load and execute skills".to_string()
        } else {
            capabilities.join("\n")
        };

        let system_prompt = format!(
            r#"You are a CCOS agent. Your job is to:
1. Understand the user's message
2. Plan which capabilities to execute
3. Provide a helpful response

IMPORTANT: You receive the ACTUAL message content directly, not UUID pointers. The Gateway has already resolved any quarantine references. Work with the message content provided.

When working with skills:
- Use ccos.skill.load with: {{ "url": "..." }} to load skill definitions.
- Once loaded, the skill_definition will contain onboarding steps and available operations.
- Execute each onboarding step using ccos.skill.execute with: {{ "skill": "skill_id", "operation": "operation_name", "params": {{...}} }}.
- PLAN AND EXECUTE ALL onboarding steps until the skill is fully operational.

You have access to these capabilities:
{}

Respond in JSON format:
{{
  "understanding": "brief description of what user wants",
  "actions": [
    {{
      "capability_id": "capability.name",
      "reasoning": "why this capability",
      "inputs": {{ "param": "value" }}
    }}
  ],
  "response": "natural language response to user"
}}"#,
            caps_list
        );

        let request_body = json!({
            "model": self.config.model,
            "max_tokens": 1000,
            "system": system_prompt,
            "messages": [
                {
                    "role": "user",
                    "content": message
                }
            ]
        });

        // Debug: log system prompt length
        info!(
            "Sending to Anthropic LLM - System prompt length: {} chars, User message: {}",
            system_prompt.len(),
            message
        );

        let response = self
            .client
            .post(url)
            .header("x-api-key", &self.config.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await?;

        if !response.status().is_success() {
            let error_text = response.text().await?;
            anyhow::bail!("Anthropic API error: {}", error_text);
        }

        let response_json: serde_json::Value = response.json().await?;
        let content = response_json["content"][0]["text"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Invalid response format"))?
            .to_string();

        // Strip markdown code blocks if present
        let json_text = if content.trim().starts_with("```") {
            let lines: Vec<&str> = content.lines().collect();
            if lines.len() >= 2
                && lines.first().unwrap().starts_with("```")
                && lines.last().unwrap().starts_with("```")
            {
                lines[1..lines.len() - 1].join("\n")
            } else {
                content.clone()
            }
        } else {
            content.clone()
        };

        // Try to parse as JSON
        match serde_json::from_str::<AgentPlan>(&json_text) {
            Ok(plan) => {
                info!("LLM generated plan with {} actions", plan.actions.len());
                Ok(plan)
            }
            Err(e) => {
                warn!("Failed to parse LLM response as JSON: {}", e);
                Ok(AgentPlan {
                    understanding: "Direct response".to_string(),
                    actions: vec![],
                    response: content,
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_plan_deserialization() {
        let json_str = r#"{
            "understanding": "User wants weather",
            "actions": [
                {
                    "capability_id": "weather.get_current",
                    "reasoning": "Need current weather",
                    "inputs": {"city": "Paris"}
                }
            ],
            "response": "I'll get the weather for you"
        }"#;

        let plan: AgentPlan = serde_json::from_str(json_str).unwrap();
        assert_eq!(plan.actions.len(), 1);
        assert_eq!(plan.actions[0].capability_id, "weather.get_current");
    }
}
