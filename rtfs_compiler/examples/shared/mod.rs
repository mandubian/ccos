//! Shared utilities for example demos
use rtfs_compiler::ccos::remote_models::RemoteModelConfig;
use rtfs_compiler::ccos::delegation::ModelProvider;
use reqwest::blocking::Client;
use std::sync::Arc;

/// Minimal blocking OpenRouter provider for various models
#[derive(Debug)]
pub struct CustomOpenRouterModel {
    pub id: &'static str,
    pub config: RemoteModelConfig,
    pub client: Arc<Client>,
}

impl CustomOpenRouterModel {
    pub fn new(id: &'static str, model_name: &str) -> Self {
        let config = RemoteModelConfig::new(
            std::env::var("OPENROUTER_API_KEY").unwrap_or_default(),
            model_name.to_string(),
        );
        let client = Arc::new(Client::builder()
            .timeout(std::time::Duration::from_secs(config.timeout_seconds))
            .build()
            .expect("HTTP client"));

        Self { id, config, client }
    }
}

impl ModelProvider for CustomOpenRouterModel {
    fn id(&self) -> &'static str { self.id }

    fn infer(&self, prompt: &str) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let request = serde_json::json!({
            "model": self.config.model_name,
            "messages": [{ "role": "user", "content": prompt }],
            "max_tokens": self.config.max_tokens,
            "temperature": self.config.temperature,
        });
        let resp: serde_json::Value = self.client
            .post("https://openrouter.ai/api/v1/chat/completions")
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .header("Content-Type", "application/json")
            .header("HTTP-Referer", "https://rtfs-compiler.example.com")
            .header("X-Title", "RTFS Compiler")
            .json(&request)
            .send()?
            .json()?;
        Ok(resp["choices"][0]["message"]["content"].as_str().unwrap_or("").to_string())
    }
}
