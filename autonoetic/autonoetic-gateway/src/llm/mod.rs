//! LLM Driver Abstraction and Types.
//!
//! Provides a thin, unified interface (`LlmDriver`) for interacting with
//! various remote model providers (OpenAI, Anthropic, Gemini, etc.).

use autonoetic_types::agent::LlmConfig;
use std::sync::Arc;

const LLM_BASE_URL_OVERRIDE_ENV: &str = "AUTONOETIC_LLM_BASE_URL";
const LLM_API_KEY_OVERRIDE_ENV: &str = "AUTONOETIC_LLM_API_KEY";

pub mod anthropic;
pub mod gemini;
pub mod openai;
pub mod provider;

#[cfg(test)]
mod tests;

// ---------------------------------------------------------------------------
// Roles
// ---------------------------------------------------------------------------

/// A conversation message role.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Role {
    System,
    User,
    Assistant,
    /// Used for tool-result turns sent back to the model.
    Tool,
}

impl Role {
    pub fn as_str(&self) -> &'static str {
        match self {
            Role::System => "system",
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::Tool => "tool",
        }
    }
}

// ---------------------------------------------------------------------------
// Tool types
// ---------------------------------------------------------------------------

/// A tool the agent can invoke. Sent to the LLM so it knows what's available.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    /// JSON Schema describing the tool's parameters (as a raw serde_json value).
    pub input_schema: serde_json::Value,
}

/// A tool invocation requested by the model in a response.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ToolCall {
    /// Opaque identifier used to match the result back to this call.
    pub id: String,
    pub name: String,
    /// JSON-encoded arguments string (matches what the model returns).
    pub arguments: String,
}

/// A tool result sent back to the model after the agent executes a tool.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ToolResult {
    /// The ID of the ToolCall this is a result for.
    pub tool_call_id: String,
    /// The name of the tool (needed by Anthropic's API for routing).
    pub tool_name: String,
    pub content: String,
    pub is_error: bool,
}

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

/// A single message in a conversation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: String,
    /// Optional tool calls from an assistant turn.
    pub tool_calls: Vec<ToolCall>,
    /// For Role::Tool turns, the matching call ID.
    pub tool_call_id: Option<String>,
}

impl Message {
    /// Convenience: plain text user message.
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: content.into(),
            tool_calls: vec![],
            tool_call_id: None,
        }
    }

    /// Convenience: system message.
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: Role::System,
            content: content.into(),
            tool_calls: vec![],
            tool_call_id: None,
        }
    }

    /// Convenience: assistant text message.
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: content.into(),
            tool_calls: vec![],
            tool_call_id: None,
        }
    }

    /// Convenience: tool result message.
    pub fn tool_result(
        tool_call_id: impl Into<String>,
        tool_name: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        let _ = tool_name; // stored via ToolResult struct; kept in signature for API clarity
        Self {
            role: Role::Tool,
            content: content.into(),
            tool_calls: vec![],
            tool_call_id: Some(tool_call_id.into()),
        }
    }
}

// ---------------------------------------------------------------------------
// Request / Response
// ---------------------------------------------------------------------------

/// A request to an LLM provider.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CompletionRequest {
    /// Model identifier (e.g., "gpt-4o", "claude-3-5-sonnet-20241022").
    pub model: String,
    /// Conversation messages.
    pub messages: Vec<Message>,
    /// Tool definitions available to the model.
    pub tools: Vec<ToolDefinition>,
    /// Maximum tokens to generate (optional).
    pub max_tokens: Option<u32>,
    /// Sampling temperature (optional).
    pub temperature: Option<f32>,
    /// Optional metadata for pipeline hooks (e.g. skip_llm, assistant_reply).
    pub metadata: Option<std::collections::HashMap<String, serde_json::Value>>,
}

impl CompletionRequest {
    pub fn simple(model: impl Into<String>, messages: Vec<Message>) -> Self {
        Self {
            model: model.into(),
            messages,
            tools: vec![],
            max_tokens: None,
            temperature: None,
            metadata: None,
        }
    }
}

/// Why the model stopped generating.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum StopReason {
    EndTurn,
    MaxTokens,
    ToolUse,
    StopSequence,
    Other(String),
}

/// Token usage statistics returned by the provider.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct TokenUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

/// Full response from a completion call.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CompletionResponse {
    /// Text content (may be empty if the model only returned tool calls).
    pub text: String,
    /// Tool calls requested by the model (may be empty).
    pub tool_calls: Vec<ToolCall>,
    pub stop_reason: StopReason,
    pub usage: TokenUsage,
}

impl CompletionResponse {
    pub fn text_only(text: String) -> Self {
        Self {
            text,
            tool_calls: vec![],
            stop_reason: StopReason::EndTurn,
            usage: TokenUsage::default(),
        }
    }

    pub fn has_tool_calls(&self) -> bool {
        !self.tool_calls.is_empty()
    }
}

// ---------------------------------------------------------------------------
// Streaming
// ---------------------------------------------------------------------------

/// Events emitted during SSE streaming.
#[derive(Debug, Clone)]
pub enum StreamEvent {
    TextDelta(String),
    ToolUseStart {
        id: String,
        name: String,
    },
    ToolInputDelta(String),
    ToolUseEnd {
        id: String,
        name: String,
        arguments: String,
    },
    Complete {
        stop_reason: StopReason,
        usage: TokenUsage,
    },
}

// ---------------------------------------------------------------------------
// LlmDriver trait
// ---------------------------------------------------------------------------

/// The unified LLM driver interface.
#[async_trait::async_trait]
pub trait LlmDriver: Send + Sync {
    /// Send a completion request and receive a full structured response.
    async fn complete(&self, request: &CompletionRequest) -> anyhow::Result<CompletionResponse>;

    /// Stream a completion, sending incremental events to the channel.
    /// Default implementation wraps `complete()` with a single text chunk.
    async fn stream(
        &self,
        request: &CompletionRequest,
        tx: tokio::sync::mpsc::Sender<StreamEvent>,
    ) -> anyhow::Result<CompletionResponse> {
        let response = self.complete(request).await?;
        if !response.text.is_empty() {
            let _ = tx.send(StreamEvent::TextDelta(response.text.clone())).await;
        }
        let _ = tx
            .send(StreamEvent::Complete {
                stop_reason: response.stop_reason.clone(),
                usage: response.usage.clone(),
            })
            .await;
        Ok(response)
    }
}

// ---------------------------------------------------------------------------
// Factory
// ---------------------------------------------------------------------------

/// Build the appropriate driver for the given config.
///
/// Credential/endpoint resolution is centralised in `provider::resolve()` —
/// drivers themselves never read environment variables.
pub fn build_driver(
    config: LlmConfig,
    client: reqwest::Client,
) -> anyhow::Result<Arc<dyn LlmDriver>> {
    let base_url_override = std::env::var(LLM_BASE_URL_OVERRIDE_ENV).ok();
    let api_key_override = std::env::var(LLM_API_KEY_OVERRIDE_ENV).ok();
    let resolved = provider::resolve(
        &config.provider,
        &config.model,
        if config.temperature > 0.0 {
            Some(config.temperature as f32)
        } else {
            None
        },
        None, // max_tokens from request, not config
        base_url_override.as_deref(),
        api_key_override.as_deref(),
    )?;

    let driver: Arc<dyn LlmDriver> = match resolved.kind {
        provider::DriverKind::Anthropic => {
            Arc::new(anthropic::AnthropicDriver::new(client, resolved))
        }
        provider::DriverKind::Gemini => Arc::new(gemini::GeminiDriver::new(client, resolved)),
        provider::DriverKind::OpenAi => Arc::new(openai::OpenAiDriver::new(client, resolved)),
    };
    Ok(driver)
}
