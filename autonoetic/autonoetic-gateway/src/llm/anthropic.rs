//! Anthropic LLM Driver.
//! 
//! Uses the Messages API.

use super::{CompletionRequest, LlmDriver};
use autonoetic_types::agent::LlmConfig;
use reqwest::Client;
use serde_json::json;

pub struct AnthropicDriver {
    client: Client,
    config: LlmConfig,
}

impl AnthropicDriver {
    pub fn new(client: Client, config: LlmConfig) -> Self {
        Self { client, config }
    }
}

#[async_trait::async_trait]
impl LlmDriver for AnthropicDriver {
    async fn complete(&self, req: &CompletionRequest) -> anyhow::Result<String> {
        let api_key = std::env::var("ANTHROPIC_API_KEY")
            .map_err(|_| anyhow::anyhow!("Missing ANTHROPIC_API_KEY environment variable"))?;

        // Anthropic treats "system" instructions as a top-level parameter, not inside the messages array
        let mut system_text = String::new();
        let mut messages = Vec::new();

        for m in &req.messages {
            if m.role == super::Role::System {
                system_text.push_str(&m.content);
                system_text.push('\n');
            } else {
                messages.push(json!({
                    "role": m.role.as_str(),
                    "content": m.content
                }));
            }
        }

        let mut body = json!({
            "model": self.config.model,
            "max_tokens": req.max_tokens.unwrap_or(4096),
            "messages": messages,
        });

        if !system_text.is_empty() {
            body.as_object_mut().unwrap().insert("system".to_string(), json!(system_text.trim()));
        }

        if self.config.temperature > 0.0 {
            body.as_object_mut().unwrap().insert("temperature".to_string(), json!(self.config.temperature));
        }

        let response = self.client.post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01")
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            anyhow::bail!("Anthropic API error {}: {}", status, text);
        }

        let json_resp: serde_json::Value = response.json().await?;
        
        json_resp["content"][0]["text"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| anyhow::anyhow!("Failed to parse Anthropic response format"))
    }
}
