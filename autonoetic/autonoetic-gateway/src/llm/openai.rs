//! OpenAI-compatible LLM Driver.
//! 
//! Serves OpenAI, OpenRouter, Groq, Together, vLLM, Ollama, etc.

use super::{CompletionRequest, LlmDriver};
use autonoetic_types::agent::LlmConfig;
use reqwest::Client;
use serde_json::json;

pub struct OpenAiDriver {
    client: Client,
    config: LlmConfig,
}

impl OpenAiDriver {
    pub fn new(client: Client, config: LlmConfig) -> Self {
        Self { client, config }
    }

    fn resolve_base_url(&self) -> &str {
        match self.config.provider.to_lowercase().as_str() {
            "openai" => "https://api.openai.com/v1/chat/completions",
            "openrouter" => "https://openrouter.ai/api/v1/chat/completions",
            _ => "https://api.openai.com/v1/chat/completions",
        }
    }

    fn resolve_api_key(&self) -> anyhow::Result<String> {
        let env_var = match self.config.provider.to_lowercase().as_str() {
            "openai" => "OPENAI_API_KEY",
            "openrouter" => "OPENROUTER_API_KEY",
            _ => "OPENAI_API_KEY",
        };
        std::env::var(env_var).map_err(|_| anyhow::anyhow!("Missing {} environment variable", env_var))
    }
}

#[async_trait::async_trait]
impl LlmDriver for OpenAiDriver {
    async fn complete(&self, req: &CompletionRequest) -> anyhow::Result<String> {
        let url = self.resolve_base_url();
        let api_key = self.resolve_api_key()?;
        
        let messages = req.messages.iter().map(|m| {
            json!({
                "role": m.role.as_str(),
                "content": m.content
            })
        }).collect::<Vec<_>>();

        // Use json! to stay radically thin
        let mut body = json!({
            "model": self.config.model,
            "messages": messages,
        });

        if self.config.temperature > 0.0 {
            if let Some(obj) = body.as_object_mut() {
                obj.insert("temperature".to_string(), json!(self.config.temperature));
            }
        }

        let mut request = self.client.post(url)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json");

        // OpenRouter optional headers
        if self.config.provider.eq_ignore_ascii_case("openrouter") {
            request = request
                .header("HTTP-Referer", "https://autonoetic.ccos.local")
                .header("X-Title", "Autonoetic Gateway");
        }

        let response = request.json(&body).send().await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            anyhow::bail!("OpenAI API error {}: {}", status, text);
        }

        let json_resp: serde_json::Value = response.json().await?;
        
        json_resp["choices"][0]["message"]["content"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| anyhow::anyhow!("Failed to parse OpenAI response format"))
    }
}
