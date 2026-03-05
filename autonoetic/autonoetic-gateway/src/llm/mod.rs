//! LLM Driver Abstraction and Types.
//! 
//! Provides a thin, unified interface (`LlmDriver`) for interacting with
//! various remote model providers (OpenAI, Anthropic, Gemini, etc.).

use autonoetic_types::agent::LlmConfig;
use std::sync::Arc;

pub mod openai;
pub mod anthropic;
pub mod gemini;

#[cfg(test)]
mod tests;

/// A conversation message role.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Role {
    System,
    User,
    Assistant,
}

impl Role {
    pub fn as_str(&self) -> &'static str {
        match self {
            Role::System => "system",
            Role::User => "user",
            Role::Assistant => "assistant",
        }
    }
}

/// A single message in a completion request.
#[derive(Debug, Clone)]
pub struct Message {
    pub role: Role,
    pub content: String,
}

/// A request to an LLM provider.
#[derive(Debug, Clone)]
pub struct CompletionRequest {
    /// Model identifier (e.g., "gpt-4o", "claude-3-5-sonnet-20241022").
    pub model: String,
    /// Conversation messages.
    pub messages: Vec<Message>,
    /// Maximum tokens to generate (optional).
    pub max_tokens: Option<u32>,
    /// Sampling temperature (optional).
    pub temperature: Option<f32>,
}

/// Stop reason for an LLM generation.
#[derive(Debug, Clone)]
pub enum StopReason {
    EndTurn,
    MaxTokens,
    ToolCall,
    StopSequence,
    Other(String),
}

/// Events emitted during streaming LLM completion.
#[derive(Debug, Clone)]
pub enum StreamEvent {
    /// Incremental text content.
    TextDelta(String),
    /// Content generation complete.
    Complete(StopReason),
}

/// The unified LLM driver interface.
#[async_trait::async_trait]
pub trait LlmDriver: Send + Sync {
    /// Send a completion request and get a full string response.
    async fn complete(&self, request: &CompletionRequest) -> anyhow::Result<String>;

    /// Stream a completion request, pushing events to the provided channel.
    /// The default implementation just calls `complete()` and sends one chunk.
    async fn stream(
        &self,
        request: &CompletionRequest,
        tx: tokio::sync::mpsc::Sender<StreamEvent>,
    ) -> anyhow::Result<()> {
        let text = self.complete(request).await?;
        if !text.is_empty() {
            let _ = tx.send(StreamEvent::TextDelta(text)).await;
        }
        let _ = tx.send(StreamEvent::Complete(StopReason::EndTurn)).await;
        Ok(())
    }
}

/// Factory to build the appropriate driver for a given config.
pub fn build_driver(config: LlmConfig, reqwest_client: reqwest::Client) -> Arc<dyn LlmDriver> {
    let provider = config.provider.to_lowercase();
    match provider.as_str() {
        "openai" => Arc::new(openai::OpenAiDriver::new(reqwest_client, config)),
        "openrouter" => {
            let mut conf = config.clone();
            // Point the generic OpenAI driver at OpenRouter's URL
            conf.provider = "openai".to_string(); // we'll use this to conditionally add headers in the driver if needed, wait actually the URL is all we need
            // Let's rely on the environment variables mapped inside the driver module later
            Arc::new(openai::OpenAiDriver::new(reqwest_client, conf))
        }
        _ => {
            tracing::warn!("Unknown provider '{}', falling back to OpenAI driver", provider);
            Arc::new(openai::OpenAiDriver::new(reqwest_client, config))
        }
    }
}
