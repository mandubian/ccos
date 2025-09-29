//! Remote Model Providers
//!
//! This module provides remote model providers for various LLM services including
//! OpenAI, Gemini, Claude, and OpenRouter. All providers implement the ModelProvider
//! trait for seamless integration with the delegation engine.

use crate::ccos::delegation::ModelProvider;
use futures::executor::block_on;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Configuration for remote model providers
#[derive(Debug, Clone)]
pub struct RemoteModelConfig {
    pub api_key: String,
    pub base_url: Option<String>,
    pub model_name: String,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
    pub timeout_seconds: u64,
}

impl RemoteModelConfig {
    pub fn new(api_key: String, model_name: String) -> Self {
        Self {
            api_key,
            base_url: None,
            model_name,
            max_tokens: None,
            temperature: Some(0.7),
            timeout_seconds: 120,
        }
    }

    pub fn with_base_url(mut self, base_url: String) -> Self {
        self.base_url = Some(base_url);
        self
    }

    pub fn with_max_tokens(mut self, max_tokens: u32) -> Self {
        self.max_tokens = Some(max_tokens);
        self
    }

    pub fn with_temperature(mut self, temperature: f32) -> Self {
        self.temperature = Some(temperature);
        self
    }

    pub fn with_timeout(mut self, timeout_seconds: u64) -> Self {
        self.timeout_seconds = timeout_seconds;
        self
    }
}

/// Base remote model provider with common functionality
#[derive(Debug)]
pub struct BaseRemoteModel {
    id: &'static str,
    config: RemoteModelConfig,
    client: Arc<Client>,
}

impl BaseRemoteModel {
    pub fn new(id: &'static str, config: RemoteModelConfig) -> Self {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(config.timeout_seconds))
            .build()
            .expect("Failed to create HTTP client");

        Self {
            id,
            config,
            client: Arc::new(client),
        }
    }

    fn get_base_url(&self) -> String {
        self.config
            .base_url
            .clone()
            .unwrap_or_else(|| match self.id {
                "openai" => "https://api.openai.com/v1".to_string(),
                "gemini" => "https://generativelanguage.googleapis.com/v1beta".to_string(),
                "claude" => "https://api.anthropic.com/v1".to_string(),
                "openrouter" => "https://openrouter.ai/api/v1".to_string(),
                _ => "https://api.openai.com/v1".to_string(),
            })
    }
}

/// OpenAI model provider
#[derive(Debug)]
pub struct OpenAIModel {
    base: BaseRemoteModel,
}

impl OpenAIModel {
    pub fn new(config: RemoteModelConfig) -> Self {
        Self {
            base: BaseRemoteModel::new("openai", config),
        }
    }

    pub fn gpt4() -> Self {
        Self::new(RemoteModelConfig::new(
            std::env::var("OPENAI_API_KEY").unwrap_or_default(),
            "gpt-4".to_string(),
        ))
    }

    pub fn gpt35_turbo() -> Self {
        Self::new(RemoteModelConfig::new(
            std::env::var("OPENAI_API_KEY").unwrap_or_default(),
            "gpt-3.5-turbo".to_string(),
        ))
    }
}

#[derive(Serialize)]
struct OpenAIRequest {
    model: String,
    messages: Vec<OpenAIMessage>,
    max_tokens: Option<u32>,
    temperature: Option<f32>,
}

#[derive(Serialize, Deserialize)]
struct OpenAIMessage {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct OpenAIResponse {
    choices: Vec<OpenAIChoice>,
}

#[derive(Deserialize)]
struct OpenAIChoice {
    message: OpenAIMessage,
}

impl ModelProvider for OpenAIModel {
    fn id(&self) -> &'static str {
        "openai"
    }

    fn infer(&self, prompt: &str) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let request = OpenAIRequest {
            model: self.base.config.model_name.clone(),
            messages: vec![OpenAIMessage {
                role: "user".to_string(),
                content: prompt.to_string(),
            }],
            max_tokens: self.base.config.max_tokens,
            temperature: self.base.config.temperature,
        };

        let response = block_on(async {
            self.base
                .client
                .post(&format!("{}/chat/completions", self.base.get_base_url()))
                .header(
                    "Authorization",
                    format!("Bearer {}", self.base.config.api_key),
                )
                .header("Content-Type", "application/json")
                .json(&request)
                .send()
                .await?
                .json::<OpenAIResponse>()
                .await
        })?;

        Ok(response.choices[0].message.content.clone())
    }
}

/// Google Gemini model provider
#[derive(Debug)]
pub struct GeminiModel {
    base: BaseRemoteModel,
}

impl GeminiModel {
    pub fn new(config: RemoteModelConfig) -> Self {
        Self {
            base: BaseRemoteModel::new("gemini", config),
        }
    }

    pub fn gemini_pro() -> Self {
        Self::new(RemoteModelConfig::new(
            std::env::var("GEMINI_API_KEY").unwrap_or_default(),
            "gemini-pro".to_string(),
        ))
    }

    pub fn gemini_flash() -> Self {
        Self::new(RemoteModelConfig::new(
            std::env::var("GEMINI_API_KEY").unwrap_or_default(),
            "gemini-1.5-flash".to_string(),
        ))
    }
}

#[derive(Serialize)]
struct GeminiRequest {
    contents: Vec<GeminiContent>,
    generation_config: Option<GeminiGenerationConfig>,
}

#[derive(Serialize, Deserialize)]
struct GeminiContent {
    parts: Vec<GeminiPart>,
}

#[derive(Serialize, Deserialize)]
struct GeminiPart {
    text: String,
}

#[derive(Serialize)]
struct GeminiGenerationConfig {
    max_output_tokens: Option<u32>,
    temperature: Option<f32>,
}

#[derive(Deserialize)]
struct GeminiResponse {
    candidates: Vec<GeminiCandidate>,
}

#[derive(Deserialize)]
struct GeminiCandidate {
    content: GeminiContent,
}

impl ModelProvider for GeminiModel {
    fn id(&self) -> &'static str {
        "gemini"
    }

    fn infer(&self, prompt: &str) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let request = GeminiRequest {
            contents: vec![GeminiContent {
                parts: vec![GeminiPart {
                    text: prompt.to_string(),
                }],
            }],
            generation_config: Some(GeminiGenerationConfig {
                max_output_tokens: self.base.config.max_tokens,
                temperature: self.base.config.temperature,
            }),
        };

        let response = block_on(async {
            self.base
                .client
                .post(&format!(
                    "{}/models/{}/generateContent",
                    self.base.get_base_url(),
                    self.base.config.model_name
                ))
                .header("x-goog-api-key", &self.base.config.api_key)
                .header("Content-Type", "application/json")
                .json(&request)
                .send()
                .await?
                .json::<GeminiResponse>()
                .await
        })?;

        Ok(response.candidates[0].content.parts[0].text.clone())
    }
}

/// Anthropic Claude model provider
#[derive(Debug)]
pub struct ClaudeModel {
    base: BaseRemoteModel,
}

impl ClaudeModel {
    pub fn new(config: RemoteModelConfig) -> Self {
        Self {
            base: BaseRemoteModel::new("claude", config),
        }
    }

    pub fn claude_3_opus() -> Self {
        Self::new(RemoteModelConfig::new(
            std::env::var("ANTHROPIC_API_KEY").unwrap_or_default(),
            "claude-3-opus-20240229".to_string(),
        ))
    }

    pub fn claude_3_sonnet() -> Self {
        Self::new(RemoteModelConfig::new(
            std::env::var("ANTHROPIC_API_KEY").unwrap_or_default(),
            "claude-3-sonnet-20240229".to_string(),
        ))
    }

    pub fn claude_3_haiku() -> Self {
        Self::new(RemoteModelConfig::new(
            std::env::var("ANTHROPIC_API_KEY").unwrap_or_default(),
            "claude-3-haiku-20240307".to_string(),
        ))
    }
}

#[derive(Serialize)]
struct ClaudeRequest {
    model: String,
    max_tokens: Option<u32>,
    temperature: Option<f32>,
    messages: Vec<ClaudeMessage>,
}

#[derive(Serialize)]
struct ClaudeMessage {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct ClaudeResponse {
    content: Vec<ClaudeContent>,
}

#[derive(Deserialize)]
struct ClaudeContent {
    text: String,
}

impl ModelProvider for ClaudeModel {
    fn id(&self) -> &'static str {
        "claude"
    }

    fn infer(&self, prompt: &str) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let request = ClaudeRequest {
            model: self.base.config.model_name.clone(),
            max_tokens: self.base.config.max_tokens,
            temperature: self.base.config.temperature,
            messages: vec![ClaudeMessage {
                role: "user".to_string(),
                content: prompt.to_string(),
            }],
        };

        let response = block_on(async {
            self.base
                .client
                .post(&format!("{}/messages", self.base.get_base_url()))
                .header("x-api-key", &self.base.config.api_key)
                .header("anthropic-version", "2023-06-01")
                .header("Content-Type", "application/json")
                .json(&request)
                .send()
                .await?
                .json::<ClaudeResponse>()
                .await
        })?;

        Ok(response.content[0].text.clone())
    }
}

/// OpenRouter model provider (aggregates multiple providers)
#[derive(Debug)]
pub struct OpenRouterModel {
    base: BaseRemoteModel,
}

impl OpenRouterModel {
    pub fn new(config: RemoteModelConfig) -> Self {
        Self {
            base: BaseRemoteModel::new("openrouter", config),
        }
    }

    pub fn gpt4() -> Self {
        Self::new(RemoteModelConfig::new(
            std::env::var("OPENROUTER_API_KEY").unwrap_or_default(),
            "openai/gpt-4".to_string(),
        ))
    }

    pub fn claude_3_opus() -> Self {
        Self::new(RemoteModelConfig::new(
            std::env::var("OPENROUTER_API_KEY").unwrap_or_default(),
            "anthropic/claude-3-opus".to_string(),
        ))
    }

    pub fn gemini_pro() -> Self {
        Self::new(RemoteModelConfig::new(
            std::env::var("OPENROUTER_API_KEY").unwrap_or_default(),
            "google/gemini-pro".to_string(),
        ))
    }

    pub fn llama_3_8b() -> Self {
        Self::new(RemoteModelConfig::new(
            std::env::var("OPENROUTER_API_KEY").unwrap_or_default(),
            "meta-llama/llama-3-8b-instruct".to_string(),
        ))
    }
}

impl ModelProvider for OpenRouterModel {
    fn id(&self) -> &'static str {
        "openrouter"
    }

    fn infer(&self, prompt: &str) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        // OpenRouter uses the same API as OpenAI
        let request = OpenAIRequest {
            model: self.base.config.model_name.clone(),
            messages: vec![OpenAIMessage {
                role: "user".to_string(),
                content: prompt.to_string(),
            }],
            max_tokens: self.base.config.max_tokens,
            temperature: self.base.config.temperature,
        };

        let response = block_on(async {
            let resp = self
                .base
                .client
                .post(&format!("{}/chat/completions", self.base.get_base_url()))
                .header(
                    "Authorization",
                    format!("Bearer {}", self.base.config.api_key),
                )
                .header("Content-Type", "application/json")
                .header("HTTP-Referer", "https://rtfs-compiler.example.com")
                .header("X-Title", "RTFS Compiler")
                .json(&request)
                .send()
                .await
                .map_err(|e| Box::<dyn std::error::Error + Send + Sync>::from(e))?;

            let status = resp.status();
            if !status.is_success() {
                let body_text = resp
                    .text()
                    .await
                    .unwrap_or_else(|_| "<failed to read body>".to_string());
                let lower = body_text.to_lowercase();
                // Detect common policy / endpoint mismatch 404 from OpenRouter
                if status.as_u16() == 404 && lower.contains("no endpoints found") {
                    return Err::<OpenAIResponse, Box<dyn std::error::Error + Send + Sync>>(Box::from(format!(
                            "OpenRouter 404: No endpoints found matching your data policy for model '{}'. Possible causes:\n  1) Model restricted under current privacy/data retention settings.\n  2) Model name incorrect or deprecated.\n  3) Account lacks provider/model access.\nRemediation steps:\n  - Verify slug on https://openrouter.ai/models\n  - Temporarily relax data/privacy filters to confirm\n  - Try a common model: 'openai/gpt-4o-mini', 'meta-llama/llama-3-8b-instruct', 'mistralai/mistral-7b-instruct'\n  - Confirm OPENROUTER_API_KEY validity.\nRaw body: {}",
                            self.base.config.model_name, body_text
                        )));
                }
                return Err::<OpenAIResponse, Box<dyn std::error::Error + Send + Sync>>(Box::from(
                    format!(
                        "OpenRouter request failed: status={} model={} body={}",
                        status, self.base.config.model_name, body_text
                    ),
                ));
            }

            resp.json::<OpenAIResponse>()
                .await
                .map_err(|e| Box::<dyn std::error::Error + Send + Sync>::from(e))
        })?;

        Ok(response.choices[0].message.content.clone())
    }
}

/// Factory for creating remote model providers
pub struct RemoteModelFactory;

impl RemoteModelFactory {
    /// Create an OpenAI model provider
    pub fn openai(model_name: &str) -> OpenAIModel {
        OpenAIModel::new(RemoteModelConfig::new(
            std::env::var("OPENAI_API_KEY").unwrap_or_default(),
            model_name.to_string(),
        ))
    }

    /// Create a Gemini model provider
    pub fn gemini(model_name: &str) -> GeminiModel {
        GeminiModel::new(RemoteModelConfig::new(
            std::env::var("GEMINI_API_KEY").unwrap_or_default(),
            model_name.to_string(),
        ))
    }

    /// Create a Claude model provider
    pub fn claude(model_name: &str) -> ClaudeModel {
        ClaudeModel::new(RemoteModelConfig::new(
            std::env::var("ANTHROPIC_API_KEY").unwrap_or_default(),
            model_name.to_string(),
        ))
    }

    /// Create an OpenRouter model provider
    pub fn openrouter(model_name: &str) -> OpenRouterModel {
        OpenRouterModel::new(RemoteModelConfig::new(
            std::env::var("OPENROUTER_API_KEY").unwrap_or_default(),
            model_name.to_string(),
        ))
    }

    /// Create a provider from a configuration string
    /// Format: "provider:model_name" (e.g., "openai:gpt-4", "claude:claude-3-opus")
    pub fn from_config(
        config: &str,
    ) -> Result<Box<dyn ModelProvider>, Box<dyn std::error::Error + Send + Sync>> {
        let parts: Vec<&str> = config.split(':').collect();
        if parts.len() != 2 {
            return Err("Invalid config format. Expected 'provider:model_name'".into());
        }

        let provider = parts[0];
        let model_name = parts[1];

        match provider {
            "openai" => Ok(Box::new(Self::openai(model_name))),
            "gemini" => Ok(Box::new(Self::gemini(model_name))),
            "claude" => Ok(Box::new(Self::claude(model_name))),
            "openrouter" => Ok(Box::new(Self::openrouter(model_name))),
            _ => Err(format!("Unknown provider: {}", provider).into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_remote_model_factory() {
        // Test factory methods (without actual API calls)
        let _openai = RemoteModelFactory::openai("gpt-4");
        let _gemini = RemoteModelFactory::gemini("gemini-pro");
        let _claude = RemoteModelFactory::claude("claude-3-opus");
        let _openrouter = RemoteModelFactory::openrouter("openai/gpt-4");
    }

    #[test]
    fn test_config_parsing() {
        // Test config string parsing
        let result = RemoteModelFactory::from_config("openai:gpt-4");
        assert!(result.is_ok());

        let result = RemoteModelFactory::from_config("invalid:format");
        assert!(result.is_err());
    }
}
