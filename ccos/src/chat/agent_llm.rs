//! LLM Integration for CCOS Agent
//!
//! Provides intelligent message processing using LLM providers.
//! The Agent uses this to understand user intent and plan capability execution.

use crate::utils::log_redaction::redact_text_for_logs;
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
        agent_context: &str,
    ) -> anyhow::Result<AgentPlan> {
        match self.config.provider.as_str() {
            "openai" | "openrouter" | "google" | "gemini" => {
                self.process_with_openai(message, context, capabilities, agent_context)
                    .await
            }
            "anthropic" => {
                self.process_with_anthropic(message, context, capabilities, agent_context)
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
        context: &[String],
        capabilities: &[String],
        agent_context: &str,
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

        let context_block = if agent_context.trim().is_empty() {
            String::new()
        } else {
            format!("\nAgent context (safe metadata):\n{}\n", agent_context)
        };

        let recent_context_block = format_recent_context_block(context);

        let system_prompt = format!(
            r#"You are a CCOS agent. Your job is to:
1. Understand the user's message
2. Plan which capabilities to execute
3. Provide a helpful response

IMPORTANT: You receive the ACTUAL message content directly, not UUID pointers. The Gateway has already resolved any quarantine references. Work with the message content provided.

When working with skills:
- Use ccos.skill.load with: {{ "url": "..." }} to load skill definitions (Markdown/YAML/JSON).
- Only use ccos.skill.load when the user is explicitly trying to load/onboard/install a skill OR when the URL clearly points to a skill definition file (e.g. ends with .md/.yaml/.yml/.json or contains /skill.md).
- If the user provides a URL that is clearly NOT a skill definition (e.g. an X/Twitter tweet URL like https://x.com/... or a normal web page), treat it as data for a skill operation instead (usually via ccos.skill.execute), or ask a clarifying question. Do NOT call ccos.skill.load for arbitrary URLs.
- Once loaded, the skill_definition will describe available operations and any setup requirements.
- Use ccos.skill.execute for any required skill operation (onboarding or otherwise) with: {{ "skill": "skill_id", "operation": "operation_name", "params": {{...}} }}.
- Plan and execute the steps required to fulfill the user's request using the available skill operations.
- Do not guess or invent skill URLs. If the user did not provide a URL and none is available in context, ask the user for the correct skill URL.
- Only use operations explicitly listed in Registered capabilities; do not invent operation names (e.g. "skill_definition"). If unsure, ask the user or check the registered capabilities list.
{}
{}

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
            context_block,
            recent_context_block,
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
        context: &[String],
        capabilities: &[String],
        agent_context: &str,
    ) -> anyhow::Result<AgentPlan> {
        let url = "https://api.anthropic.com/v1/messages";

        let caps_list = if capabilities.is_empty() {
            "- ccos.chat.egress.* - Send outbound messages\n- ccos.skill.* - Load and execute skills".to_string()
        } else {
            capabilities.join("\n")
        };

        let context_block = if agent_context.trim().is_empty() {
            String::new()
        } else {
            format!("\nAgent context (safe metadata):\n{}\n", agent_context)
        };

        let recent_context_block = format_recent_context_block(context);

        let system_prompt = format!(
            r#"You are a CCOS agent. Your job is to:
1. Understand the user's message
2. Plan which capabilities to execute
3. Provide a helpful response

IMPORTANT: You receive the ACTUAL message content directly, not UUID pointers. The Gateway has already resolved any quarantine references. Work with the message content provided.

When working with skills:
- Use ccos.skill.load with: {{ "url": "..." }} to load skill definitions (Markdown/YAML/JSON).
- Only use ccos.skill.load when the user is explicitly trying to load/onboard/install a skill OR when the URL clearly points to a skill definition file (e.g. ends with .md/.yaml/.yml/.json or contains /skill.md).
- If the user provides a URL that is clearly NOT a skill definition (e.g. an X/Twitter tweet URL like https://x.com/... or a normal web page), treat it as data for a skill operation instead (usually via ccos.skill.execute), or ask a clarifying question. Do NOT call ccos.skill.load for arbitrary URLs.
- Once loaded, the skill_definition will describe available operations and any setup requirements.
- Use ccos.skill.execute for any required skill operation (onboarding or otherwise) with: {{ "skill": "skill_id", "operation": "operation_name", "params": {{...}} }}.
- Plan and execute the steps required to fulfill the user's request using the available skill operations.
- Do not guess or invent skill URLs. If the user did not provide a URL and none is available in context, ask the user for the correct skill URL.
- Only use operations explicitly listed in Registered capabilities; do not invent operation names (e.g. "skill_definition"). If unsure, ask the user or check the registered capabilities list.
{}
{}

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
            context_block,
            recent_context_block,
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

fn format_recent_context_block(context: &[String]) -> String {
    if context.is_empty() {
        return String::new();
    }

    // Keep the prompt stable and small: include only a few recent turns, redacted.
    let max_lines = 8usize;
    let max_line_len = 240usize;
    let start = context.len().saturating_sub(max_lines);

    let lines = context[start..]
        .iter()
        .map(|line| {
            let redacted = redact_text_for_logs(line);
            if redacted.len() > max_line_len {
                format!("{}...", &redacted[..max_line_len])
            } else {
                redacted
            }
        })
        .collect::<Vec<_>>()
        .join("\n");

    format!("\nRecent conversation (redacted):\n{}\n", lines)
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
