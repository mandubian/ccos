//! Google Gemini LLM Driver.
//! 
//! Uses the generateContent API.

use super::{CompletionRequest, LlmDriver};
use autonoetic_types::agent::LlmConfig;
use reqwest::Client;
use serde_json::json;

pub struct GeminiDriver {
    client: Client,
    config: LlmConfig,
}

impl GeminiDriver {
    pub fn new(client: Client, config: LlmConfig) -> Self {
        Self { client, config }
    }
}

#[async_trait::async_trait]
impl LlmDriver for GeminiDriver {
    async fn complete(&self, req: &CompletionRequest) -> anyhow::Result<String> {
        let api_key = std::env::var("GEMINI_API_KEY")
            .or_else(|_| std::env::var("GOOGLE_API_KEY"))
            .map_err(|_| anyhow::anyhow!("Missing GEMINI_API_KEY environment variable"))?;

        // Gemini uses `contents` array with `parts`
        let mut contents = Vec::new();
        let mut system_instruction = None;

        for m in &req.messages {
            if m.role == super::Role::System {
                system_instruction = Some(json!({
                    "parts": [{"text": m.content}]
                }));
            } else {
                let role = match m.role {
                    super::Role::User => "user",
                    super::Role::Assistant => "model",
                    _ => "user",
                };
                contents.push(json!({
                    "role": role,
                    "parts": [{"text": m.content}]
                }));
            }
        }

        let mut body = json!({
            "contents": contents,
        });

        if let Some(sys) = system_instruction {
            body.as_object_mut().unwrap().insert("systemInstruction".to_string(), sys);
        }

        let mut generation_config = serde_json::Map::new();
        if let Some(max) = req.max_tokens {
            generation_config.insert("maxOutputTokens".to_string(), json!(max));
        }
        if self.config.temperature > 0.0 {
            generation_config.insert("temperature".to_string(), json!(self.config.temperature));
        }

        if !generation_config.is_empty() {
            body.as_object_mut()
                .unwrap()
                .insert("generationConfig".to_string(), serde_json::Value::Object(generation_config));
        }

        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent",
            self.config.model
        );

        let response = self.client.post(&url)
            .header("x-goog-api-key", api_key)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            anyhow::bail!("Gemini API error {}: {}", status, text);
        }

        let json_resp: serde_json::Value = response.json().await?;
        
        json_resp["candidates"][0]["content"]["parts"][0]["text"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| anyhow::anyhow!("Failed to parse Gemini response format"))
    }
}
