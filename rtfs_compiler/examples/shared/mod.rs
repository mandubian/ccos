//! Shared utilities for example demos
use reqwest::blocking::Client;
use rtfs_compiler::ccos::delegation::ModelProvider;
use rtfs_compiler::ccos::remote_models::RemoteModelConfig;

/// Real OpenRouter provider that makes actual API calls
#[derive(Debug)]
pub struct CustomOpenRouterModel {
    pub id: &'static str,
    pub config: RemoteModelConfig,
}

impl CustomOpenRouterModel {
    pub fn new(id: &'static str, model_name: &str) -> Self {
        let config = RemoteModelConfig::new(
            std::env::var("OPENROUTER_API_KEY").unwrap_or_default(),
            model_name.to_string(),
        );

        Self { id, config }
    }
}

impl ModelProvider for CustomOpenRouterModel {
    fn id(&self) -> &'static str {
        self.id
    }

    fn infer(&self, prompt: &str) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        // Check if API key is available
        if self.config.api_key.is_empty() {
            return Err("OPENROUTER_API_KEY not set".into());
        }

        // Use block_in_place to run blocking HTTP call in async context
        tokio::task::block_in_place(|| {
            // Create client inside the blocking task
            let client = Client::new();

            // Make real OpenRouter API call
            let request = serde_json::json!({
                "model": self.config.model_name,
                "messages": [{
                    "role": "user",
                    "content": prompt
                }],
                "max_tokens": 2048,
                "temperature": 0.1,  // Low temperature for consistent RTFS generation
            });

            let response: serde_json::Value = client
                .post("https://openrouter.ai/api/v1/chat/completions")
                .header("Authorization", format!("Bearer {}", self.config.api_key))
                .header("Content-Type", "application/json")
                .header("HTTP-Referer", "https://rtfs-compiler.example.com")
                .header("X-Title", "RTFS Compiler")
                .json(&request)
                .send()?
                .json()?;

            let content = response["choices"][0]["message"]["content"]
                .as_str()
                .ok_or("Invalid response format from OpenRouter API")?;

            Ok(content.to_string())
        })
    }
}
