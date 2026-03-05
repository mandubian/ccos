//! LLM Driver Abstraction.

use autonoetic_types::agent::LlmConfig;
use reqwest::Client;
use std::sync::Arc;

#[async_trait::async_trait]
pub trait LlmDriver: Send + Sync {
    async fn complete(&self, prompt: &str) -> anyhow::Result<String>;
}

/// A generic HTTP-based LLM driver using `reqwest`.
pub struct HttpLlmDriver {
    client: Client,
    _config: LlmConfig,
}

impl HttpLlmDriver {
    pub fn new(config: LlmConfig) -> Self {
        Self {
            client: Client::new(),
            _config: config,
        }
    }
}

#[async_trait::async_trait]
impl LlmDriver for HttpLlmDriver {
    async fn complete(&self, _prompt: &str) -> anyhow::Result<String> {
        // Stub implementation — this would make a real reqwest call to OpenAI/Anthropic
        // based on self._config
        tracing::debug!("HttpLlmDriver::complete stub called");
        Ok("Stubbed LLM response".to_string())
    }
}

/// Factory to build the appropriate driver for a given config.
pub fn build_driver(config: LlmConfig) -> Arc<dyn LlmDriver> {
    Arc::new(HttpLlmDriver::new(config))
}
