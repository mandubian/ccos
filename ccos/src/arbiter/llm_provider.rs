//! LLM Provider Abstraction
//!
//! This module provides the abstraction layer for different LLM providers,
//! allowing the Arbiter to work with various LLM services while maintaining
//! a consistent interface.

use crate::arbiter::prompt::{FilePromptStore, PromptManager};
use crate::types::{
    GenerationContext, IntentStatus, Plan, PlanBody, PlanLanguage, StorableIntent, TriggerSource,
};
use async_trait::async_trait;
use rtfs::parser;
use rtfs::runtime::error::RuntimeError;
use rtfs::runtime::values::Value;
use serde::{Deserialize, Serialize};
use std::collections::HashMap; // for validating reduced-grammar RTFS plans
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use sha2::{Digest, Sha256};

/// Convert a HashMap<String, Value> into an RTFS map literal string.
/// Example: {:k1 "v" :k2 123}
fn hashmap_to_rtfs_map(m: &std::collections::HashMap<String, Value>) -> String {
    if m.is_empty() {
        return "{}".to_string();
    }
    let mut parts: Vec<String> = Vec::with_capacity(m.len());
    for (k, v) in m {
        let val_str = match v {
            Value::String(s) => format!("\"{}\"", s.replace('"', "\\\"")),
            Value::Integer(i) => format!("{}", i),
            Value::Float(f) => format!("{}", f),
            Value::Boolean(b) => format!(":{}", if *b { "true" } else { "false" }),
            Value::Symbol(sym) => format!(":{}", sym.0),
            Value::Map(map) => {
                // nested map -> render keys (MapKey) and values recursively
                let items: Vec<String> = map
                    .iter()
                    .map(|(mk, mv)| {
                        let key_str = match mk {
                            rtfs::ast::MapKey::String(s) => s.clone(),
                            rtfs::ast::MapKey::Keyword(k) => format!(":{}", k.0),
                            rtfs::ast::MapKey::Integer(i) => format!("{}", i),
                        };
                        // Reuse Display for nested values where appropriate
                        let val = match mv {
                            Value::String(s) => format!("\"{}\"", s.replace('"', "\\\"")),
                            Value::Map(_) | Value::Vector(_) | Value::List(_) => format!("{}", mv),
                            _ => format!("{}", mv),
                        };
                        format!("{} {}", key_str, val)
                    })
                    .collect();
                format!("{{{}}}", items.join(" "))
            }
            Value::Vector(vec) | Value::List(vec) => {
                let elems = vec
                    .iter()
                    .map(|e| match e {
                        Value::String(s) => format!("\"{}\"", s.replace('"', "\\\"")),
                        Value::Integer(i) => format!("{}", i),
                        Value::Float(f) => format!("{}", f),
                        Value::Boolean(b) => format!(":{}", if *b { "true" } else { "false" }),
                        Value::Symbol(sym) => format!(":{}", sym.0),
                        _ => format!("{}", e),
                    })
                    .collect::<Vec<_>>()
                    .join(" ");
                format!("[{}]", elems)
            }
            _ => format!("{}", v),
        };
        parts.push(format!(":{} {}", k.replace('/', "/"), val_str));
    }
    format!("{{{}}}", parts.join(" "))
}

/// Convert a HashMap<String, String> (storable intent representation) into an RTFS map literal string.
/// The stored strings are expected to be RTFS source expressions and will be inlined as-is.
fn hashmap_str_to_rtfs_map(m: &std::collections::HashMap<String, String>) -> String {
    if m.is_empty() {
        return "{}".to_string();
    }
    let mut parts: Vec<String> = Vec::with_capacity(m.len());
    for (k, v) in m {
        // values in StorableIntent are RTFS source snippets; insert directly
        parts.push(format!(":{} {}", k.replace('/', "/"), v));
    }
    format!("{{{}}}", parts.join(" "))
}

/// Result of plan validation by an LLM provider
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationResult {
    pub is_valid: bool,
    pub confidence: f64,
    pub reasoning: String,
    pub suggestions: Vec<String>,
    pub errors: Vec<String>,
}

/// Metrics for tracking retry behavior
#[derive(Debug)]
pub struct RetryMetrics {
    /// Total number of plan generation attempts (including first attempts)
    pub total_attempts: AtomicU64,
    /// Number of successful retries (attempts > 1 that succeeded)
    pub successful_retries: AtomicU64,
    /// Number of failed retries (attempts > 1 that failed)
    pub failed_retries: AtomicU64,
    /// Number of first attempts that succeeded (no retry needed)
    pub first_attempt_successes: AtomicU64,
    /// Number of first attempts that failed (required retry)
    pub first_attempt_failures: AtomicU64,
}

/// Captures summary details about a single LLM completion.
struct LlmCompletion {
    content: String,
    prompt_hash: String,
    response_hash: String,
    prompt_tokens: Option<u32>,
    completion_tokens: Option<u32>,
    total_tokens: Option<u32>,
    latency_ms: u128,
}

fn sha256_hex(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}

fn clamp_latency_to_i64(latency_ms: u128) -> i64 {
    latency_ms.min(i64::MAX as u128) as i64
}

fn attach_completion_metadata_to_intent(
    intent: &mut StorableIntent,
    config: &LlmProviderConfig,
    completion: &LlmCompletion,
) {
    intent.metadata.insert(
        "llm.prompt_hash".to_string(),
        completion.prompt_hash.clone(),
    );
    intent.metadata.insert(
        "llm.response_hash".to_string(),
        completion.response_hash.clone(),
    );
    intent
        .metadata
        .insert("llm.model".to_string(), config.model.clone());
    intent.metadata.insert(
        "llm.provider".to_string(),
        format!("{:?}", config.provider_type),
    );
    intent.metadata.insert(
        "llm.latency_ms".to_string(),
        completion.latency_ms.to_string(),
    );
    if let Some(tokens) = completion.prompt_tokens {
        intent
            .metadata
            .insert("llm.prompt_tokens".to_string(), tokens.to_string());
    }
    if let Some(tokens) = completion.completion_tokens {
        intent
            .metadata
            .insert("llm.completion_tokens".to_string(), tokens.to_string());
    }
    if let Some(tokens) = completion.total_tokens {
        intent
            .metadata
            .insert("llm.total_tokens".to_string(), tokens.to_string());
    }
}

fn attach_completion_metadata_to_plan(
    plan: &mut Plan,
    config: &LlmProviderConfig,
    completion: &LlmCompletion,
) {
    plan.metadata.insert(
        "llm.prompt_hash".to_string(),
        Value::String(completion.prompt_hash.clone()),
    );
    plan.metadata.insert(
        "llm.response_hash".to_string(),
        Value::String(completion.response_hash.clone()),
    );
    plan.metadata
        .insert("llm.model".to_string(), Value::String(config.model.clone()));
    plan.metadata.insert(
        "llm.provider".to_string(),
        Value::String(format!("{:?}", config.provider_type)),
    );
    plan.metadata.insert(
        "llm.latency_ms".to_string(),
        Value::Integer(clamp_latency_to_i64(completion.latency_ms)),
    );
    if let Some(tokens) = completion.prompt_tokens {
        plan.metadata.insert(
            "llm.prompt_tokens".to_string(),
            Value::Integer(tokens as i64),
        );
    }
    if let Some(tokens) = completion.completion_tokens {
        plan.metadata.insert(
            "llm.completion_tokens".to_string(),
            Value::Integer(tokens as i64),
        );
    }
    if let Some(tokens) = completion.total_tokens {
        plan.metadata.insert(
            "llm.total_tokens".to_string(),
            Value::Integer(tokens as i64),
        );
    }
}

impl RetryMetrics {
    pub fn new() -> Self {
        Self {
            total_attempts: AtomicU64::new(0),
            successful_retries: AtomicU64::new(0),
            failed_retries: AtomicU64::new(0),
            first_attempt_successes: AtomicU64::new(0),
            first_attempt_failures: AtomicU64::new(0),
        }
    }

    /// Record a successful plan generation
    pub fn record_success(&self, attempt_number: u32) {
        self.total_attempts.fetch_add(1, Ordering::Relaxed);
        if attempt_number == 1 {
            self.first_attempt_successes.fetch_add(1, Ordering::Relaxed);
        } else {
            self.successful_retries.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Record a failed plan generation
    pub fn record_failure(&self, attempt_number: u32) {
        self.total_attempts.fetch_add(1, Ordering::Relaxed);
        if attempt_number == 1 {
            self.first_attempt_failures.fetch_add(1, Ordering::Relaxed);
        } else {
            self.failed_retries.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Get current metrics as a summary
    pub fn get_summary(&self) -> RetryMetricsSummary {
        RetryMetricsSummary {
            total_attempts: self.total_attempts.load(Ordering::Relaxed),
            successful_retries: self.successful_retries.load(Ordering::Relaxed),
            failed_retries: self.failed_retries.load(Ordering::Relaxed),
            first_attempt_successes: self.first_attempt_successes.load(Ordering::Relaxed),
            first_attempt_failures: self.first_attempt_failures.load(Ordering::Relaxed),
        }
    }
}

/// Summary of retry metrics for reporting
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryMetricsSummary {
    pub total_attempts: u64,
    pub successful_retries: u64,
    pub failed_retries: u64,
    pub first_attempt_successes: u64,
    pub first_attempt_failures: u64,
}

impl RetryMetricsSummary {
    /// Calculate retry success rate (successful retries / total retries)
    pub fn retry_success_rate(&self) -> f64 {
        let total_retries = self.successful_retries + self.failed_retries;
        if total_retries == 0 {
            0.0
        } else {
            self.successful_retries as f64 / total_retries as f64
        }
    }

    /// Calculate overall success rate (all successes / all attempts)
    pub fn overall_success_rate(&self) -> f64 {
        if self.total_attempts == 0 {
            0.0
        } else {
            (self.first_attempt_successes + self.successful_retries) as f64
                / self.total_attempts as f64
        }
    }

    /// Calculate first attempt success rate
    pub fn first_attempt_success_rate(&self) -> f64 {
        let first_attempts = self.first_attempt_successes + self.first_attempt_failures;
        if first_attempts == 0 {
            0.0
        } else {
            self.first_attempt_successes as f64 / first_attempts as f64
        }
    }
}

/// Configuration for LLM providers
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmProviderConfig {
    pub provider_type: LlmProviderType,
    pub model: String,
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f64>,
    pub timeout_seconds: Option<u64>,
    pub retry_config: crate::arbiter::arbiter_config::RetryConfig,
}

/// Supported LLM provider types
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum LlmProviderType {
    Stub,      // For testing - deterministic responses
    OpenAI,    // OpenAI GPT models
    Anthropic, // Anthropic Claude models
    Local,     // Local models (Ollama, etc.)
}

/// Abstract interface for LLM providers
#[async_trait]
pub trait LlmProvider: Send + Sync {
    /// Generate an Intent from natural language
    async fn generate_intent(
        &self,
        prompt: &str,
        context: Option<HashMap<String, String>>,
    ) -> Result<StorableIntent, RuntimeError>;

    /// Generate a Plan from an Intent
    async fn generate_plan(
        &self,
        intent: &StorableIntent,
        context: Option<HashMap<String, String>>,
    ) -> Result<Plan, RuntimeError>;

    /// Generate a Plan from an Intent with retry logic
    async fn generate_plan_with_retry(
        &self,
        intent: &StorableIntent,
        context: Option<HashMap<String, String>>,
    ) -> Result<Plan, RuntimeError> {
        // Default implementation just calls generate_plan
        // Individual providers can override this for custom retry logic
        self.generate_plan(intent, context).await
    }

    /// Get retry metrics summary for monitoring and debugging
    fn get_retry_metrics(&self) -> Option<RetryMetricsSummary> {
        // Default implementation returns None
        // Individual providers can override this to provide metrics
        None
    }

    /// Validate a generated Plan (using string representation to avoid Send/Sync issues)
    async fn validate_plan(&self, plan_content: &str) -> Result<ValidationResult, RuntimeError>;

    /// Generate text from a prompt (generic text generation)
    async fn generate_text(&self, prompt: &str) -> Result<String, RuntimeError>;

    /// Get provider information
    fn get_info(&self) -> LlmProviderInfo;
}

/// Information about an LLM provider
#[derive(Debug, Clone)]
pub struct LlmProviderInfo {
    pub name: String,
    pub version: String,
    pub model: String,
    pub capabilities: Vec<String>,
}

/// OpenAI-compatible provider (works with OpenAI and OpenRouter)
pub struct OpenAILlmProvider {
    config: LlmProviderConfig,
    client: reqwest::Client,
    metrics: RetryMetrics,
    prompt_manager: PromptManager<FilePromptStore>,
}

impl OpenAILlmProvider {
    pub fn new(config: LlmProviderConfig) -> Result<Self, RuntimeError> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(
                config.timeout_seconds.unwrap_or(30),
            ))
            .build()
            .map_err(|e| RuntimeError::Generic(format!("Failed to create HTTP client: {}", e)))?;

        // Assets are at workspace root, so try ../assets first, then assets (for when run from workspace root)
        let prompt_path = if std::path::Path::new("../assets/prompts/arbiter").exists() {
            "../assets/prompts/arbiter"
        } else {
            "assets/prompts/arbiter"
        };
        let prompt_store = FilePromptStore::new(prompt_path);
        let prompt_manager = PromptManager::new(prompt_store);

        Ok(Self {
            config,
            client,
            metrics: RetryMetrics::new(),
            prompt_manager,
        })
    }

    /// Get current retry metrics summary
    pub fn get_retry_metrics(&self) -> RetryMetricsSummary {
        self.metrics.get_summary()
    }

    /// Extracts the first top-level (do ...) s-expression from a text blob.
    fn extract_do_block(text: &str) -> Option<String> {
        let start = text.find("(do");
        let start = match start {
            Some(s) => s,
            None => return None,
        };
        let mut depth = 0usize;
        for (idx, ch) in text[start..].char_indices() {
            match ch {
                '(' => depth += 1,
                ')' => {
                    if depth == 0 {
                        return None;
                    }
                    depth -= 1;
                    if depth == 0 {
                        let end = start + idx + 1;
                        return Some(text[start..end].to_string());
                    }
                }
                _ => {}
            }
        }
        None
    }

    /// Extracts the first top-level (plan ...) s-expression from a text blob.
    fn extract_plan_block(text: &str) -> Option<String> {
        let start = text.find("(plan");
        let start = match start {
            Some(s) => s,
            None => return None,
        };
        let mut depth = 0usize;
        for (idx, ch) in text[start..].char_indices() {
            match ch {
                '(' => depth += 1,
                ')' => {
                    if depth == 0 {
                        return None;
                    }
                    depth -= 1;
                    if depth == 0 {
                        let end = start + idx + 1;
                        return Some(text[start..end].to_string());
                    }
                }
                _ => {}
            }
        }
        None
    }

    /// Very small helper to extract a quoted string value following a given keyword in a plan block.
    /// Example: for key ":name" extracts the first "..." after it.
    fn extract_quoted_value_after_key(plan_block: &str, key: &str) -> Option<String> {
        if let Some(kpos) = plan_block.find(key) {
            let after = &plan_block[kpos + key.len()..];
            if let Some(q1) = after.find('"') {
                let rest = &after[q1 + 1..];
                if let Some(q2) = rest.find('"') {
                    return Some(rest[..q2].to_string());
                }
            }
        }
        None
    }

    /// Extracts the first top-level s-expression immediately following a given keyword key.
    /// Example: for key ":body", extracts the (do ...) s-expression right after it, skipping quoted text.
    fn extract_s_expr_after_key(text: &str, key: &str) -> Option<String> {
        let kpos = text.find(key)?;
        let after = &text[kpos + key.len()..];
        // Find the first unquoted '(' after the key
        let mut in_string = false;
        let mut prev: Option<char> = None;
        let mut rel_start: Option<usize> = None;
        for (i, ch) in after.char_indices() {
            match ch {
                '"' => {
                    if prev != Some('\\') {
                        in_string = !in_string;
                    }
                }
                '(' if !in_string => {
                    rel_start = Some(i);
                    break;
                }
                _ => {}
            }
            prev = Some(ch);
        }
        let rel_start = rel_start?;
        let start = kpos + key.len() + rel_start;

        // Extract balanced s-expression starting at start
        let mut depth = 0usize;
        for (idx, ch) in text[start..].char_indices() {
            match ch {
                '(' => depth += 1,
                ')' => {
                    if depth == 0 {
                        return None;
                    }
                    depth -= 1;
                    if depth == 0 {
                        let end = start + idx + 1;
                        return Some(text[start..end].to_string());
                    }
                }
                _ => {}
            }
        }
        None
    }

    async fn make_request(
        &self,
        messages: Vec<OpenAIMessage>,
    ) -> Result<LlmCompletion, RuntimeError> {
        let api_key = self.config.api_key.as_ref().ok_or_else(|| {
            RuntimeError::Generic("API key required for OpenAI provider".to_string())
        })?;

        let base_url = self
            .config
            .base_url
            .as_deref()
            .unwrap_or("https://api.openai.com/v1");
        let url = format!("{}/chat/completions", base_url);

        let request_body = OpenAIRequest {
            model: self.config.model.clone(),
            messages,
            max_tokens: self.config.max_tokens,
            temperature: self.config.temperature,
        };
        let payload_bytes = serde_json::to_vec(&request_body).map_err(|e| {
            RuntimeError::Generic(format!("Failed to serialize request body: {}", e))
        })?;
        let prompt_hash = sha256_hex(&payload_bytes);

        let mut request_builder = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json");

        if base_url.contains("openrouter.ai") {
            let referer = std::env::var("OPENROUTER_HTTP_REFERER")
                .unwrap_or_else(|_| "https://github.com/mandubian/ccos".to_string());
            let title = std::env::var("OPENROUTER_TITLE")
                .unwrap_or_else(|_| "CCOS Smart Assistant Demo".to_string());
            request_builder = request_builder
                .header("HTTP-Referer", referer)
                .header("X-Title", title);
        }

        let start = Instant::now();
        let response = request_builder
            .body(payload_bytes)
            .send()
            .await
            .map_err(|e| RuntimeError::Generic(format!("HTTP request failed: {}", e)))?;
        let status = response.status();
        // Read bytes first so we can show them even if UTF-8 conversion fails
        let bytes = match response.bytes().await {
            Ok(b) => b,
            Err(e) => {
                return Err(RuntimeError::Generic(format!(
                    "❌ Failed to read LLM API response body\n\n\
                    🔍 Error reading response bytes: {}\n\n\
                    📊 HTTP Status: {}\n\n\
                    💡 This could be due to:\n\
                    • Network connection was interrupted\n\
                    • Response was too large to read\n\
                    • API endpoint is unreachable\n\n\
                    🔧 Try checking:\n\
                    • Network connection stability\n\
                    • API endpoint configuration\n\
                    • Firewall or proxy settings",
                    e,
                    status.as_u16()
                )));
            }
        };

        // Try to convert bytes to text, but show bytes if conversion fails
        let raw_body = match String::from_utf8(bytes.to_vec()) {
            Ok(text) => text,
            Err(e) => {
                // Show the raw bytes (lossy conversion) so user can see what was actually received
                let body_preview = if bytes.len() > 500 {
                    format!(
                        "{}...\n[truncated, total length: {} bytes]",
                        String::from_utf8_lossy(&bytes[..500]),
                        bytes.len()
                    )
                } else {
                    String::from_utf8_lossy(&bytes).to_string()
                };

                return Err(RuntimeError::Generic(format!(
                    "❌ LLM API response is not valid UTF-8 text\n\n\
                    🔍 UTF-8 conversion error: {}\n\n\
                    📊 HTTP Status: {}\n\n\
                    📥 Response body (raw bytes, lossy UTF-8 conversion):\n\
                    ┌─────────────────────────────────────────────────────────\n\
                    {}\n\
                    └─────────────────────────────────────────────────────────\n\n\
                    💡 This could be due to:\n\
                    • API returned binary data instead of text\n\
                    • Response encoding is not UTF-8\n\
                    • Response contains invalid UTF-8 sequences\n\
                    • API error response in unexpected format\n\n\
                    🔧 Try checking:\n\
                    • API logs for the actual response\n\
                    • Response content-type header\n\
                    • API documentation for expected response format",
                    e,
                    status.as_u16(),
                    body_preview
                )));
            }
        };

        if !status.is_success() {
            // Enhanced error message for HTTP errors
            let response_preview = if raw_body.len() > 1000 {
                format!(
                    "{}...\n[truncated, total length: {} chars]",
                    &raw_body[..1000],
                    raw_body.len()
                )
            } else {
                raw_body.clone()
            };

            return Err(RuntimeError::Generic(format!(
                "❌ LLM API request failed (HTTP {})\n\n\
                📥 API response:\n\
                ┌─────────────────────────────────────────────────────────\n\
                {}\n\
                └─────────────────────────────────────────────────────────\n\n\
                💡 Common causes:\n\
                • Invalid API key or authentication failure\n\
                • Rate limiting (too many requests)\n\
                • Quota exceeded\n\
                • Invalid model name\n\
                • Network connectivity issues\n\
                • API endpoint unavailable\n\n\
                🔧 Check the response above for specific error details.",
                status.as_u16(),
                response_preview
            )));
        }

        let response_hash = sha256_hex(raw_body.as_bytes());

        let response_body: OpenAIResponse = serde_json::from_str(&raw_body).map_err(|e| {
            // Enhanced error message with full response for debugging
            let response_preview = if raw_body.len() > 1000 {
                format!(
                    "{}...\n[truncated, total length: {} chars]",
                    &raw_body[..1000],
                    raw_body.len()
                )
            } else {
                raw_body.clone()
            };

            RuntimeError::Generic(format!(
                "❌ Failed to parse LLM API response as JSON\n\n\
                    📥 Raw API response:\n\
                    ┌─────────────────────────────────────────────────────────\n\
                    {}\n\
                    └─────────────────────────────────────────────────────────\n\n\
                    🔍 JSON parsing error: {}\n\n\
                    💡 This could be due to:\n\
                    • API returned an error message instead of a valid response\n\
                    • Response format changed or is unexpected\n\
                    • Network issue causing incomplete response\n\
                    • API rate limiting or authentication error\n\n\
                    🔧 Check the raw response above to see what the API actually returned.",
                response_preview, e
            ))
        })?;

        let choice = response_body
            .choices
            .first()
            .ok_or_else(|| RuntimeError::Generic("LLM response missing choices".to_string()))?;

        let content = choice.message.content.clone();
        let finish_reason = choice.finish_reason.as_deref();

        // Handle different finish_reason values
        match finish_reason {
            Some("length") => {
                // Response was truncated due to token limit
                let max_tokens = self.config.max_tokens.unwrap_or(0);
                eprintln!(
                    "⚠️  WARNING: LLM response was truncated (finish_reason: length). \
                    Current max_tokens: {}. \
                    Consider increasing CCOS_LLM_MAX_TOKENS environment variable or max_tokens in config.",
                    max_tokens
                );
            }
            Some("content_filter") => {
                // Response was stopped due to content filter
                eprintln!(
                    "⚠️  WARNING: LLM response was stopped by content filter (finish_reason: content_filter). \
                    The response may be incomplete or filtered."
                );
            }
            Some("function_call") => {
                // Response stopped for function call (this is normal for function-calling models)
                // Don't warn, this is expected behavior
            }
            Some("stop") | None => {
                // Normal completion or null (both are fine)
                // No warning needed
            }
            Some(other) => {
                // Unknown finish_reason value
                eprintln!(
                    "ℹ️  LLM response finished with reason: {} (unexpected value)",
                    other
                );
            }
        }

        let usage = response_body.usage.unwrap_or_default();
        let elapsed_ms = start.elapsed().as_millis();

        Ok(LlmCompletion {
            content,
            prompt_hash,
            response_hash,
            prompt_tokens: usage.prompt_tokens,
            completion_tokens: usage.completion_tokens,
            total_tokens: usage.total_tokens,
            latency_ms: elapsed_ms,
        })
    }

    fn parse_intent_from_json(&self, json_str: &str) -> Result<StorableIntent, RuntimeError> {
        // Try to extract JSON from the response (it might be wrapped in markdown)
        let json_start = json_str.find('{').unwrap_or(0);
        let json_end = json_str.rfind('}').map(|i| i + 1).unwrap_or(json_str.len());
        let json_content = &json_str[json_start..json_end];

        #[derive(Deserialize)]
        struct IntentJson {
            name: Option<String>,
            goal: String,
            constraints: Option<HashMap<String, String>>,
            preferences: Option<HashMap<String, String>>,
            success_criteria: Option<String>,
        }

        let intent_json: IntentJson = serde_json::from_str(json_content)
            .map_err(|e| RuntimeError::Generic(format!("Failed to parse intent JSON: {}", e)))?;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        Ok(StorableIntent {
            intent_id: format!("openai_intent_{}", uuid::Uuid::new_v4()),
            name: intent_json.name,
            original_request: "".to_string(), // Will be set by caller
            rtfs_intent_source: "".to_string(),
            goal: intent_json.goal,
            constraints: intent_json.constraints.unwrap_or_default(),
            preferences: intent_json.preferences.unwrap_or_default(),
            success_criteria: intent_json.success_criteria,
            parent_intent: None,
            child_intents: vec![],
            triggered_by: TriggerSource::HumanRequest,
            generation_context: GenerationContext {
                arbiter_version: "openai-provider-1.0".to_string(),
                generation_timestamp: now,
                input_context: HashMap::new(),
                reasoning_trace: None,
            },
            status: IntentStatus::Active,
            priority: 0,
            created_at: now,
            updated_at: now,
            metadata: HashMap::new(),
        })
    }

    fn parse_plan_from_json(&self, json_str: &str, intent_id: &str) -> Result<Plan, RuntimeError> {
        // Try to extract JSON from the response
        let json_start = json_str.find('{').unwrap_or(0);
        let json_end = json_str.rfind('}').map(|i| i + 1).unwrap_or(json_str.len());
        let json_content = &json_str[json_start..json_end];

        #[derive(Deserialize)]
        struct PlanJson {
            name: Option<String>,
            steps: Vec<String>,
        }

        let plan_json: PlanJson = serde_json::from_str(json_content)
            .map_err(|e| RuntimeError::Generic(format!("Failed to parse plan JSON: {}", e)))?;

        let rtfs_body = format!("(do\n  {}\n)", plan_json.steps.join("\n  "));

        Ok(Plan {
            plan_id: format!("openai_plan_{}", uuid::Uuid::new_v4()),
            name: plan_json.name,
            intent_ids: vec![intent_id.to_string()],
            language: PlanLanguage::Rtfs20,
            body: PlanBody::Rtfs(rtfs_body),
            status: crate::types::PlanStatus::Draft,
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            metadata: HashMap::new(),
            input_schema: None,
            output_schema: None,
            policies: HashMap::new(),
            capabilities_required: vec![],
            annotations: HashMap::new(),
        })
    }
}

#[async_trait]
impl LlmProvider for OpenAILlmProvider {
    async fn generate_intent(
        &self,
        prompt: &str,
        _context: Option<HashMap<String, String>>,
    ) -> Result<StorableIntent, RuntimeError> {
        // Load prompt from assets with fallback
        let vars = HashMap::from([("user_request".to_string(), prompt.to_string())]);

        let system_message = self.prompt_manager
            .render("intent_generation", "v1", &vars)
            .unwrap_or_else(|e| {
                eprintln!("Warning: Failed to load intent_generation prompt from assets: {}. Using fallback.", e);
                r#"You are an AI assistant that converts natural language requests into structured intents for a cognitive computing system.

Generate a JSON response with the following structure:
{
  "name": "descriptive_name_for_intent",
  "goal": "clear_description_of_what_should_be_achieved",
  "constraints": {
    "constraint_name": "constraint_value_as_string"
  },
  "preferences": {
    "preference_name": "preference_value_as_string"
  },
  "success_criteria": "how_to_determine_if_intent_was_successful"
}

IMPORTANT: All values in constraints and preferences must be strings, not numbers or arrays.
Examples:
- "max_cost": "100" (not 100)
- "priority": "high" (not ["high"])
- "timeout": "30_seconds" (not 30)

Only respond with valid JSON."#.to_string()
            });

        // Optional: display prompts during live runtime when enabled
        let show_prompts = std::env::var("RTFS_SHOW_PROMPTS")
            .map(|v| v == "1")
            .unwrap_or(false)
            || std::env::var("CCOS_DEBUG")
                .map(|v| v == "1")
                .unwrap_or(false);

        if show_prompts {
            println!(
                "\n=== LLM Intent Generation Prompt ===\n[system]\n{}\n\n[user]\n{}\n=== END PROMPT ===\n",
                system_message,
                prompt
            );
        }

        let messages = vec![
            OpenAIMessage {
                role: "system".to_string(),
                content: system_message.to_string(),
            },
            OpenAIMessage {
                role: "user".to_string(),
                content: prompt.to_string(),
            },
        ];

        let completion = self.make_request(messages).await?;

        if show_prompts {
            println!(
                "\n=== LLM Raw Response (Intent Generation) ===\n{}\n=== END RESPONSE ===\n",
                completion.content
            );
        }
        let mut intent = self.parse_intent_from_json(&completion.content)?;
        attach_completion_metadata_to_intent(&mut intent, &self.config, &completion);
        Ok(intent)
    }

    async fn generate_plan(
        &self,
        intent: &StorableIntent,
        _context: Option<HashMap<String, String>>,
    ) -> Result<Plan, RuntimeError> {
        // Use consolidated plan_generation prompts by default
        // Legacy modes can be enabled via RTFS_LEGACY_PLAN_FULL or RTFS_LEGACY_PLAN_REDUCED
        let use_legacy_full = std::env::var("RTFS_LEGACY_PLAN_FULL")
            .map(|v| v == "1")
            .unwrap_or(false);
        let use_legacy_reduced = std::env::var("RTFS_LEGACY_PLAN_REDUCED")
            .map(|v| v == "1")
            .unwrap_or(false);

        // Prepare variables for prompt rendering
        // Render constraints/preferences (stored as strings) as RTFS maps for insertion into RTFS prompts
        let constraints_str = hashmap_str_to_rtfs_map(&intent.constraints);
        let preferences_str = hashmap_str_to_rtfs_map(&intent.preferences);

        let mut vars = HashMap::from([
            ("goal".to_string(), intent.goal.clone()),
            ("constraints".to_string(), constraints_str.clone()),
            ("preferences".to_string(), preferences_str.clone()),
        ]);

        // Add context variables from previous plan executions
        if let Some(ref context) = _context {
            for (key, value) in context {
                vars.insert(format!("context_{}", key), value.clone());
            }
        }

        // Select prompt: consolidated by default, legacy modes if explicitly requested
        let prompt_id = if use_legacy_full {
            "plan_generation_full"
        } else if use_legacy_reduced {
            "plan_generation_reduced"
        } else {
            "plan_generation" // Consolidated unified prompts
        };

        let system_message = self.prompt_manager
            .render(prompt_id, "v1", &vars)
            .unwrap_or_else(|e| {
                eprintln!("Warning: Failed to load {} prompt from assets: {}. Using fallback.", prompt_id, e);
                // Fallback to consolidated prompt format
                r#"You translate an RTFS intent into a concrete RTFS plan.

Output format: ONLY a single well-formed RTFS s-expression starting with (plan ...). No prose, no JSON, no fences.

Plan structure:
(plan
  :name "descriptive_name"
  :language rtfs20
  :body (do
    (step "Step Name" <expr>)
    ...
  )
  :annotations {:key "value"}
)

CRITICAL: let bindings are LOCAL to a single step. Variables CANNOT cross step boundaries.
Final step should return a structured map with keyword keys for downstream reuse."#.to_string()
            });

        let mut user_message = format!(
            "Intent goal: {}\nConstraints: {}\nPreferences: {}",
            intent.goal, constraints_str, preferences_str
        );

        // Add context information if available
        if let Some(context) = _context {
            if !context.is_empty() {
                user_message.push_str("\n\nAvailable context from previous executions:");
                for (key, value) in context {
                    user_message.push_str(&format!("\n- {}: {}", key, value));
                }
                user_message.push_str("\n\nYou can use these context values directly in your plan. For example, if context shows 'trip/destination: Paris', you can use 'Paris' directly in your plan instead of '<trip/destination>'.");
            }
        }

        user_message
            .push_str("\n\nGenerate the (plan ...) now, following the grammar and constraints:");

        // Optional: display prompts during live runtime when enabled
        // Enable by setting RTFS_SHOW_PROMPTS=1 or CCOS_DEBUG=1
        let show_prompts = std::env::var("RTFS_SHOW_PROMPTS")
            .map(|v| v == "1")
            .unwrap_or(false)
            || std::env::var("CCOS_DEBUG")
                .map(|v| v == "1")
                .unwrap_or(false);
        if show_prompts {
            println!(
                "\n=== LLM Plan Generation Prompt ===\n[system]\n{}\n\n[user]\n{}\n=== END PROMPT ===\n",
                system_message,
                user_message
            );
        }

        let messages = vec![
            OpenAIMessage {
                role: "system".to_string(),
                content: system_message,
            },
            OpenAIMessage {
                role: "user".to_string(),
                content: user_message,
            },
        ];

        let completion = self.make_request(messages).await?;
        let response = completion.content.clone();
        if show_prompts {
            println!(
                "\n=== LLM Raw Response (Plan Generation) ===\n{}\n=== END RESPONSE ===\n",
                response
            );
        }
        // Extract plan: consolidated format always expects (plan ...) wrapper
        // Legacy modes may return different formats
        let expect_plan_wrapper = !use_legacy_reduced;

        if expect_plan_wrapper {
            if let Some(plan_block) = Self::extract_plan_block(&response) {
                // Prefer extracting the (do ...) right after :body; fallback to generic do search
                if let Some(do_block) = Self::extract_s_expr_after_key(&plan_block, ":body")
                    .or_else(|| Self::extract_do_block(&plan_block))
                {
                    // If we extracted a do block from the plan, use it
                    // Parser validation is skipped because LLM may generate function calls
                    // that aren't yet defined in the parser's symbol table
                    let mut plan_name: Option<String> = None;
                    if let Some(name) = Self::extract_quoted_value_after_key(&plan_block, ":name") {
                        plan_name = Some(name);
                    }
                    let mut plan = Plan {
                        plan_id: format!("openai_plan_{}", uuid::Uuid::new_v4()),
                        name: plan_name,
                        intent_ids: vec![intent.intent_id.clone()],
                        language: PlanLanguage::Rtfs20,
                        body: PlanBody::Rtfs(do_block),
                        status: crate::types::PlanStatus::Draft,
                        created_at: std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap()
                            .as_secs(),
                        metadata: HashMap::new(),
                        input_schema: None,
                        output_schema: None,
                        policies: HashMap::new(),
                        capabilities_required: vec![],
                        annotations: HashMap::new(),
                    };
                    attach_completion_metadata_to_plan(&mut plan, &self.config, &completion);
                    return Ok(plan);
                }
            }
        }

        // Fallback: direct RTFS (do ...) body
        if let Some(do_block) = Self::extract_do_block(&response) {
            // If we successfully extracted a (do ...) block, use it
            // Parser validation is skipped because the LLM may generate function calls
            // that aren't yet defined in the parser's symbol table
            let mut plan = Plan {
                plan_id: format!("openai_plan_{}", uuid::Uuid::new_v4()),
                name: None,
                intent_ids: vec![intent.intent_id.clone()],
                language: PlanLanguage::Rtfs20,
                body: PlanBody::Rtfs(do_block),
                status: crate::types::PlanStatus::Draft,
                created_at: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
                metadata: HashMap::new(),
                input_schema: None,
                output_schema: None,
                policies: HashMap::new(),
                capabilities_required: vec![],
                annotations: HashMap::new(),
            };
            attach_completion_metadata_to_plan(&mut plan, &self.config, &completion);
            return Ok(plan);
        }

        // Fallback: previous JSON-wrapped steps contract
        let mut plan = self.parse_plan_from_json(&response, &intent.intent_id)?;
        attach_completion_metadata_to_plan(&mut plan, &self.config, &completion);
        Ok(plan)
    }

    async fn generate_plan_with_retry(
        &self,
        intent: &StorableIntent,
        _context: Option<HashMap<String, String>>,
    ) -> Result<Plan, RuntimeError> {
        let mut last_error = None;
        let mut last_plan_text = None;

        for attempt in 1..=self.config.retry_config.max_retries {
            // First: try to render a retry prompt asset into complete OpenAI messages.
            // If rendering succeeds we will use those messages directly; otherwise fall back to legacy inline prompts below.
            let vars = HashMap::from([
                ("goal".to_string(), intent.goal.clone()),
                (
                    "constraints".to_string(),
                    hashmap_str_to_rtfs_map(&intent.constraints),
                ),
                (
                    "preferences".to_string(),
                    hashmap_str_to_rtfs_map(&intent.preferences),
                ),
                ("attempt".to_string(), format!("{}", attempt)),
                (
                    "max_retries".to_string(),
                    format!("{}", self.config.retry_config.max_retries),
                ),
                (
                    "variant".to_string(),
                    if self.config.retry_config.send_error_feedback {
                        "feedback".to_string()
                    } else {
                        "simple".to_string()
                    },
                ),
                (
                    "last_plan_text".to_string(),
                    last_plan_text.clone().unwrap_or_default(),
                ),
                (
                    "last_error".to_string(),
                    last_error.clone().unwrap_or_default(),
                ),
            ]);

            if let Ok(text) = self
                .prompt_manager
                .render("plan_generation_retry", "v1", &vars)
            {
                // If the prompt asset contains '---' treat left as system and right as user
                let messages = if let Some(idx) = text.find("---") {
                    let system = text[..idx].trim().to_string();
                    let user = text[idx + 3..].trim().to_string();
                    vec![
                        OpenAIMessage {
                            role: "system".to_string(),
                            content: system,
                        },
                        OpenAIMessage {
                            role: "user".to_string(),
                            content: user,
                        },
                    ]
                } else {
                    let system_msg = text;
                    let user_message = if attempt == 1 {
                        format!(
                            "Intent goal: {}\nConstraints: {}\nPreferences: {}\n\nGenerate the (do ...) body now:",
                            intent.goal,
                            hashmap_str_to_rtfs_map(&intent.constraints),
                            hashmap_str_to_rtfs_map(&intent.preferences),
                        )
                    } else if self.config.retry_config.send_error_feedback {
                        format!(
                            "Intent goal: {}\nConstraints: {}\nPreferences: {}\n\nPrevious attempt that failed:\n{}\n\nError: {}\n\nPlease generate a corrected (do ...) body:",
                            intent.goal,
                            hashmap_str_to_rtfs_map(&intent.constraints),
                            hashmap_str_to_rtfs_map(&intent.preferences),
                            last_plan_text.as_ref().unwrap_or(&"".to_string()),
                            last_error.as_ref().unwrap_or(&"".to_string())
                        )
                    } else {
                        format!(
                            "Intent goal: {}\nConstraints: {}\nPreferences: {}\n\nGenerate the (do ...) body now:",
                            intent.goal,
                            hashmap_str_to_rtfs_map(&intent.constraints),
                            hashmap_str_to_rtfs_map(&intent.preferences),
                        )
                    };
                    vec![
                        OpenAIMessage {
                            role: "system".to_string(),
                            content: system_msg,
                        },
                        OpenAIMessage {
                            role: "user".to_string(),
                            content: user_message,
                        },
                    ]
                };

                // Make request with rendered messages
                let completion = self.make_request(messages).await?;
                let response = completion.content.clone();

                // Validate and parse the plan just like the legacy path
                let plan_result = if let Some(do_block) =
                    OpenAILlmProvider::extract_do_block(&response)
                {
                    if parser::parse(&do_block).is_ok() {
                        let mut plan = Plan {
                            plan_id: format!("openai_plan_{}", uuid::Uuid::new_v4()),
                            name: None,
                            intent_ids: vec![intent.intent_id.clone()],
                            language: PlanLanguage::Rtfs20,
                            body: PlanBody::Rtfs(do_block.to_string()),
                            status: crate::types::PlanStatus::Draft,
                            created_at: std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap()
                                .as_secs(),
                            metadata: HashMap::new(),
                            input_schema: None,
                            output_schema: None,
                            policies: HashMap::new(),
                            capabilities_required: vec![],
                            annotations: HashMap::new(),
                        };
                        attach_completion_metadata_to_plan(&mut plan, &self.config, &completion);
                        Ok(plan)
                    } else {
                        Err(RuntimeError::Generic(format!(
                            "Failed to parse RTFS plan: {}",
                            do_block
                        )))
                    }
                } else {
                    // Fallback to JSON parsing
                    let mut plan = self.parse_plan_from_json(&response, &intent.intent_id)?;
                    attach_completion_metadata_to_plan(&mut plan, &self.config, &completion);
                    Ok(plan)
                };

                match plan_result {
                    Ok(plan) => {
                        self.metrics.record_success(attempt);
                        if attempt > 1 {
                            log::info!("✅ Plan retry succeeded on attempt {}", attempt);
                        }
                        return Ok(plan);
                    }
                    Err(e) => {
                        self.metrics.record_failure(attempt);
                        let error_context = if attempt == 1 {
                            format!("Initial attempt failed: {}", e)
                        } else {
                            format!(
                                "Retry attempt {}/{} failed: {}",
                                attempt, self.config.retry_config.max_retries, e
                            )
                        };
                        log::warn!("❌ {}", error_context);
                        let enhanced_error = format!(
                            "Attempt {}: {} (Response: {})",
                            attempt,
                            e,
                            if response.len() > 200 {
                                format!("{}...", &response[..200])
                            } else {
                                response.clone()
                            }
                        );
                        last_error = Some(enhanced_error);
                        last_plan_text = Some(response.clone());
                        if attempt < self.config.retry_config.max_retries {
                            continue; // retry
                        }
                    }
                }
            }

            // If we reach here, prompt asset rendering failed; fall back to legacy inline prompt construction
            // Create prompt based on attempt
            let messages = if attempt == 1 {
                // Initial prompt
                let system_message = r#"You translate an RTFS intent into a concrete RTFS execution body using a reduced grammar.

Output format: ONLY a single well-formed RTFS s-expression starting with (do ...). No prose, no JSON, no fences.

Allowed forms (reduced grammar):
- (do <step> <step> ...)
- (step "Descriptive Name" (<expr>)) ; name must be a double-quoted string
- (call :cap.namespace.op <args...>)   ; capability ids MUST be RTFS keywords starting with a colon
- (if <condition> <then> <else>)  ; conditional execution (use for binary yes/no)
- (match <value> <pattern1> <result1> <pattern2> <result2> ...)  ; pattern matching (use for multiple choices)
- (let [var1 expr1 var2 expr2] <body>)  ; local bindings
- (str <arg1> <arg2> ...)  ; string concatenation
- (= <arg1> <arg2>)  ; equality comparison

Arguments allowed:
- strings: "..."
- numbers: 1 2 3
- simple maps with keyword keys: {:key "value" :a 1 :b 2}
- lists: [1 2 3] or ["a" "b" "c"]

Available capabilities (use exact names with colons):
- :ccos.echo - print message to output
- :ccos.user.ask - prompt user for input, returns their response
- :ccos.math.add - add numbers
- :ccos.math.subtract - subtract numbers
- :ccos.math.multiply - multiply numbers
- :ccos.math.divide - divide numbers

CRITICAL: let bindings are LOCAL to a single step. You CANNOT use variables across step boundaries.

CORRECT - capture and reuse within single step:
  (step "Greet User"
    (let [name (call :ccos.user.ask "What is your name?")]
      (call :ccos.echo {:message (str "Hello, " name "!")})))

CORRECT - multiple prompts with summary in one step:
  (step "Survey"
    (let [name (call :ccos.user.ask "What is your name?")
          age (call :ccos.user.ask "How old are you?")
          hobby (call :ccos.user.ask "What is your hobby?")]
      (call :ccos.echo {:message (str "Summary: " name ", age " age ", enjoys " hobby)})))

WRONG - let has no body:
  (step "Bad" (let [name (call :ccos.user.ask "Name?")])  ; Missing body expression!

WRONG - variables out of scope across steps:
  (step "Get" (let [n (call :ccos.user.ask "Name?")] n))
  (step "Use" (call :ccos.echo {:message n}))  ; n not in scope here!

Conditional branching (CORRECT - if for yes/no):
  (step "Pizza Check" 
    (let [likes (call :ccos.user.ask "Do you like pizza? (yes/no)")]
      (if (= likes "yes")
        (call :ccos.echo {:message "Great! Pizza is delicious!"})
        (call :ccos.echo {:message "Maybe try it sometime!"}))))

Multiple choice (CORRECT - match for many options):
  (step "Language Hello World" 
    (let [lang (call :ccos.user.ask "Choose: rust, python, or javascript")]
      (match lang
        "rust" (call :ccos.echo {:message "println!(\"Hello\")"})
        "python" (call :ccos.echo {:message "print('Hello')"})
        "javascript" (call :ccos.echo {:message "console.log('Hello')"})
        _ (call :ccos.echo {:message "Unknown language"}))))

Return exactly one (plan ...) with these constraints.
"#;
                let user_message = format!(
                    "Intent goal: {}\nConstraints: {}\nPreferences: {}\n\nGenerate the (do ...) body now:",
                    intent.goal,
                    hashmap_str_to_rtfs_map(&intent.constraints),
                    hashmap_str_to_rtfs_map(&intent.preferences),
                );
                vec![
                    OpenAIMessage {
                        role: "system".to_string(),
                        content: system_message.to_string(),
                    },
                    OpenAIMessage {
                        role: "user".to_string(),
                        content: user_message,
                    },
                ]
            } else if self.config.retry_config.send_error_feedback {
                // Retry prompt with error feedback
                let system_message = if attempt == self.config.retry_config.max_retries
                    && self.config.retry_config.simplify_on_final_attempt
                {
                    r#"You translate an RTFS intent into a concrete RTFS execution body using a SIMPLIFIED grammar.

This is your final attempt. Keep it simple and basic.

Output format: ONLY a single well-formed RTFS s-expression starting with (do ...). No prose, no JSON, no fences.

SIMPLIFIED forms only:
- (do <step> <step> ...)
- (step "Name" (call :cap.op <args>))
- (call :ccos.echo {:message "text"})
- (call :ccos.user.ask "question")

Available capabilities:
- :ccos.echo - print message
- :ccos.user.ask - ask user question

Keep it simple. No complex logic, no let bindings, no conditionals.
"#
                } else {
                    r#"You translate an RTFS intent into a concrete RTFS execution body using a reduced grammar.

The previous attempt failed. Please fix the error and try again.

Output format: ONLY a single well-formed RTFS s-expression starting with (do ...). No prose, no JSON, no fences.

Allowed forms (reduced grammar):
- (do <step> <step> ...)
- (step "Descriptive Name" (<expr>)) ; name must be a double-quoted string
- (call :cap.namespace.op <args...>)   ; capability ids MUST be RTFS keywords starting with a colon
- (if <condition> <then> <else>)  ; conditional execution (use for binary yes/no)
- (match <value> <pattern1> <result1> <pattern2> <result2> ...)  ; pattern matching (use for multiple choices)
- (let [var1 expr1 var2 expr2] <body>)  ; local bindings
- (str <arg1> <arg2> ...)  ; string concatenation
- (= <arg1> <arg2>)  ; equality comparison

Arguments allowed:
- strings: "..."
- numbers: 1 2 3
- simple maps with keyword keys: {:key "value" :a 1 :b 2}
- lists: [1 2 3] or ["a" "b" "c"]

Available capabilities (use exact names with colons):
- :ccos.echo - print message to output
- :ccos.user.ask - prompt user for input, returns their response
- :ccos.math.add - add numbers
- :ccos.math.subtract - subtract numbers
- :ccos.math.multiply - multiply numbers
- :ccos.math.divide - divide numbers

CRITICAL: let bindings are LOCAL to a single step. You CANNOT use variables across step boundaries.

CORRECT - capture and reuse within single step:
  (step "Greet User"
    (let [name (call :ccos.user.ask "What is your name?")]
      (call :ccos.echo {:message (str "Hello, " name "!")})))

CORRECT - multiple prompts with summary in one step:
  (step "Survey"
    (let [name (call :ccos.user.ask "What is your name?")
          age (call :ccos.user.ask "How old are you?")
          hobby (call :ccos.user.ask "What is your hobby?")]
      (call :ccos.echo {:message (str "Summary: " name ", age " age ", enjoys " hobby)})))

WRONG - let has no body:
  (step "Bad" (let [name (call :ccos.user.ask "Name?")])  ; Missing body expression!

WRONG - variables out of scope across steps:
  (step "Get" (let [n (call :ccos.user.ask "Name?")] n))
  (step "Use" (call :ccos.echo {:message n}))  ; n not in scope here!

Return exactly one (plan ...) with these constraints.
"#
                };
                let user_message = format!(
                    "Intent goal: {}\nConstraints: {}\nPreferences: {}\n\nPrevious attempt that failed:\n{}\n\nError: {}\n\nPlease generate a corrected (do ...) body:",
                    intent.goal,
                    hashmap_str_to_rtfs_map(&intent.constraints),
                    hashmap_str_to_rtfs_map(&intent.preferences),
                    last_plan_text.as_ref().unwrap(),
                    last_error.as_ref().unwrap()
                );
                vec![
                    OpenAIMessage {
                        role: "system".to_string(),
                        content: system_message.to_string(),
                    },
                    OpenAIMessage {
                        role: "user".to_string(),
                        content: user_message,
                    },
                ]
            } else {
                // Simple retry without feedback
                let system_message = r#"You translate an RTFS intent into a concrete RTFS execution body using a reduced grammar.

Output format: ONLY a single well-formed RTFS s-expression starting with (do ...). No prose, no JSON, no fences.

Allowed forms (reduced grammar):
- (do <step> <step> ...)
- (step "Descriptive Name" (<expr>)) ; name must be a double-quoted string
- (call :cap.namespace.op <args...>)   ; capability ids MUST be RTFS keywords starting with a colon
- (if <condition> <then> <else>)  ; conditional execution (use for binary yes/no)
- (match <value> <pattern1> <result1> <pattern2> <result2> ...)  ; pattern matching (use for multiple choices)
- (let [var1 expr1 var2 expr2] <body>)  ; local bindings
- (str <arg1> <arg2> ...)  ; string concatenation
- (= <arg1> <arg2>)  ; equality comparison

Arguments allowed:
- strings: "..."
- numbers: 1 2 3
- simple maps with keyword keys: {:key "value" :a 1 :b 2}
- lists: [1 2 3] or ["a" "b" "c"]

Available capabilities (use exact names with colons):
- :ccos.echo - print message to output
- :ccos.user.ask - prompt user for input, returns their response
- :ccos.math.add - add numbers
- :ccos.math.subtract - subtract numbers
- :ccos.math.multiply - multiply numbers
- :ccos.math.divide - divide numbers

CRITICAL: let bindings are LOCAL to a single step. You CANNOT use variables across step boundaries.

CORRECT - capture and reuse within single step:
  (step "Greet User"
    (let [name (call :ccos.user.ask "What is your name?")]
      (call :ccos.echo {:message (str "Hello, " name "!")})))

CORRECT - multiple prompts with summary in one step:
  (step "Survey"
    (let [name (call :ccos.user.ask "What is your name?")
          age (call :ccos.user.ask "How old are you?")
          hobby (call :ccos.user.ask "What is your hobby?")]
      (call :ccos.echo {:message (str "Summary: " name ", age " age ", enjoys " hobby)})))

WRONG - let has no body:
  (step "Bad" (let [name (call :ccos.user.ask "Name?")])  ; Missing body expression!

WRONG - variables out of scope across steps:
  (step "Get" (let [n (call :ccos.user.ask "Name?")] n))
  (step "Use" (call :ccos.echo {:message n}))  ; n not in scope here!

Conditional branching (CORRECT - if for yes/no):
  (step "Pizza Check" 
    (let [likes (call :ccos.user.ask "Do you like pizza? (yes/no)")]
      (if (= likes "yes")
        (call :ccos.echo {:message "Great! Pizza is delicious!"})
        (call :ccos.echo {:message "Maybe try it sometime!"}))))

Multiple choice (CORRECT - match for many options):
  (step "Language Hello World" 
    (let [lang (call :ccos.user.ask "Choose: rust, python, or javascript")]
      (match lang
        "rust" (call :ccos.echo {:message "println!(\"Hello\")"})
        "python" (call :ccos.echo {:message "print('Hello')"})
        "javascript" (call :ccos.echo {:message "console.log('Hello')"})
        _ (call :ccos.echo {:message "Unknown language"}))))

Return exactly one (plan ...) with these constraints.
"#;
                let user_message = format!(
                    "Intent goal: {}\nConstraints: {}\nPreferences: {}\n\nGenerate the (do ...) body now:",
                    intent.goal,
                    hashmap_str_to_rtfs_map(&intent.constraints),
                    hashmap_str_to_rtfs_map(&intent.preferences),
                );
                vec![
                    OpenAIMessage {
                        role: "system".to_string(),
                        content: system_message.to_string(),
                    },
                    OpenAIMessage {
                        role: "user".to_string(),
                        content: user_message,
                    },
                ]
            };

            let completion = self.make_request(messages).await?;
            let response = completion.content.clone();

            // Validate and parse the plan
            let plan_result = if let Some(do_block) = OpenAILlmProvider::extract_do_block(&response)
            {
                if parser::parse(&do_block).is_ok() {
                    let mut plan = Plan {
                        plan_id: format!("openai_plan_{}", uuid::Uuid::new_v4()),
                        name: None,
                        intent_ids: vec![intent.intent_id.clone()],
                        language: PlanLanguage::Rtfs20,
                        body: PlanBody::Rtfs(do_block.to_string()),
                        status: crate::types::PlanStatus::Draft,
                        created_at: std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap()
                            .as_secs(),
                        metadata: HashMap::new(),
                        input_schema: None,
                        output_schema: None,
                        policies: HashMap::new(),
                        capabilities_required: vec![],
                        annotations: HashMap::new(),
                    };
                    attach_completion_metadata_to_plan(&mut plan, &self.config, &completion);
                    Ok(plan)
                } else {
                    Err(RuntimeError::Generic(format!(
                        "Failed to parse RTFS plan: {}",
                        do_block
                    )))
                }
            } else {
                // Fallback to JSON parsing
                let mut plan = self.parse_plan_from_json(&response, &intent.intent_id)?;
                attach_completion_metadata_to_plan(&mut plan, &self.config, &completion);
                Ok(plan)
            };

            match plan_result {
                Ok(plan) => {
                    // Record successful attempt
                    self.metrics.record_success(attempt);
                    if attempt > 1 {
                        log::info!("✅ Plan retry succeeded on attempt {}", attempt);
                    }
                    return Ok(plan);
                }
                Err(e) => {
                    // Record failed attempt
                    self.metrics.record_failure(attempt);

                    // Create detailed error message for logging
                    let error_context = if attempt == 1 {
                        format!("Initial attempt failed: {}", e)
                    } else {
                        format!(
                            "Retry attempt {}/{} failed: {}",
                            attempt, self.config.retry_config.max_retries, e
                        )
                    };

                    log::warn!("❌ {}", error_context);

                    // Store enhanced error message for final error reporting
                    let enhanced_error = format!(
                        "Attempt {}: {} (Response: {})",
                        attempt,
                        e,
                        if response.len() > 200 {
                            format!("{}...", &response[..200])
                        } else {
                            response.clone()
                        }
                    );
                    last_error = Some(enhanced_error);
                    last_plan_text = Some(response.clone());

                    if attempt < self.config.retry_config.max_retries {
                        continue; // Retry
                    }
                }
            }
        }

        // All retries exhausted
        if self.config.retry_config.use_stub_fallback {
            log::warn!(
                "⚠️  Using stub fallback after {} failed attempts",
                self.config.retry_config.max_retries
            );
            // Record stub fallback as a success (since we're providing a working plan)
            self.metrics
                .record_success(self.config.retry_config.max_retries + 1);
            let safe_goal = intent.goal.replace('"', r#"\""#);
            let stub_body = format!(
                r#"(do
    (step "Report Fallback" (call :ccos.echo {{:message "Plan retry attempts exhausted; returning safe fallback."}}))
    (step "Restate Goal" (call :ccos.echo {{:message "Original goal: {}"}}))
    (step "Next Actions" (call :ccos.echo {{:message "Please refine the intent or consult logs for details."}}))
)"#,
                safe_goal
            );
            return Ok(Plan {
                plan_id: format!("stub_plan_{}", uuid::Uuid::new_v4()),
                name: Some("Stub Plan".to_string()),
                intent_ids: vec![intent.intent_id.clone()],
                language: PlanLanguage::Rtfs20,
                body: PlanBody::Rtfs(stub_body),
                status: crate::types::PlanStatus::Draft,
                created_at: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
                metadata: HashMap::new(),
                input_schema: None,
                output_schema: None,
                policies: HashMap::new(),
                capabilities_required: vec![],
                annotations: HashMap::new(),
            });
        }

        // Record final failure (all retries exhausted, no stub fallback)
        self.metrics
            .record_failure(self.config.retry_config.max_retries);

        // Create detailed error message with helpful suggestions
        let detailed_error = format!(
            "❌ Plan generation failed after {} attempts.\n\n\
            🔍 **What went wrong:**\n\
            The LLM was unable to generate a valid RTFS plan for your request: \"{}\"\n\
            Last error: {}\n\n\
            💡 **Suggestions to try:**\n\
            1. **Simplify your request** - Break complex tasks into smaller, simpler steps\n\
            2. **Use clearer language** - Be more specific about what you want to accomplish\n\
            3. **Try basic patterns** - Start with simple tasks like:\n\
               - \"Echo a message\"\n\
               - \"Ask the user for their name\"\n\
               - \"Add two numbers together\"\n\n\
            📚 **Working examples:**\n\
            - \"Greet the user and ask for their name\"\n\
            - \"Ask the user if they like pizza and respond accordingly\"\n\
            - \"Ask the user to choose between options and show the result\"\n\n\
            🔧 **Technical details:**\n\
            - Total attempts: {}\n\
            - Retry configuration: max_retries={}, feedback={}, stub_fallback={}\n\
            - Intent constraints: {}
            - Intent preferences: {}",
            self.config.retry_config.max_retries,
            intent.goal,
            last_error.unwrap_or_else(|| "Unknown error".to_string()),
            self.config.retry_config.max_retries,
            self.config.retry_config.max_retries,
            self.config.retry_config.send_error_feedback,
            self.config.retry_config.use_stub_fallback,
            serde_json::to_string(&intent.constraints)
                .unwrap_or_else(|_| format!("{:?}", intent.constraints)),
            serde_json::to_string(&intent.preferences)
                .unwrap_or_else(|_| format!("{:?}", intent.preferences))
        );

        Err(RuntimeError::Generic(detailed_error))
    }

    async fn validate_plan(&self, plan_content: &str) -> Result<ValidationResult, RuntimeError> {
        let system_message = r#"You are an AI assistant that validates RTFS plans.

Analyze the plan and respond with JSON:
{
  "is_valid": true/false,
  "confidence": 0.0-1.0,
  "reasoning": "explanation",
  "suggestions": ["suggestion1", "suggestion2"],
  "errors": ["error1", "error2"]
}

Check for:
- Valid RTFS syntax
- Appropriate step usage
- Logical flow
- Error handling

Only respond with valid JSON."#;

        let user_message = format!("Validate this RTFS plan:\n{}", plan_content);

        let messages = vec![
            OpenAIMessage {
                role: "system".to_string(),
                content: system_message.to_string(),
            },
            OpenAIMessage {
                role: "user".to_string(),
                content: user_message,
            },
        ];

        let completion = self.make_request(messages).await?;
        let response = completion.content;

        // Parse validation result
        let json_start = response.find('{').unwrap_or(0);
        let json_end = response.rfind('}').map(|i| i + 1).unwrap_or(response.len());
        let json_content = &response[json_start..json_end];

        #[derive(Deserialize)]
        struct ValidationJson {
            is_valid: bool,
            confidence: f64,
            reasoning: String,
            suggestions: Vec<String>,
            errors: Vec<String>,
        }

        let validation: ValidationJson = serde_json::from_str(json_content).map_err(|e| {
            RuntimeError::Generic(format!("Failed to parse validation JSON: {}", e))
        })?;

        Ok(ValidationResult {
            is_valid: validation.is_valid,
            confidence: validation.confidence,
            reasoning: validation.reasoning,
            suggestions: validation.suggestions,
            errors: validation.errors,
        })
    }

    async fn generate_text(&self, prompt: &str) -> Result<String, RuntimeError> {
        let messages = vec![OpenAIMessage {
            role: "user".to_string(),
            content: prompt.to_string(),
        }];
        let completion = self.make_request(messages).await?;
        Ok(completion.content)
    }

    fn get_info(&self) -> LlmProviderInfo {
        LlmProviderInfo {
            name: "OpenAI LLM Provider".to_string(),
            version: "1.0.0".to_string(),
            model: self.config.model.clone(),
            capabilities: vec![
                "intent_generation".to_string(),
                "plan_generation".to_string(),
                "plan_validation".to_string(),
            ],
        }
    }
}

// OpenAI API types
#[derive(Serialize)]
struct OpenAIRequest {
    model: String,
    messages: Vec<OpenAIMessage>,
    max_tokens: Option<u32>,
    temperature: Option<f64>,
}

#[derive(Serialize, Deserialize)]
struct OpenAIMessage {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct OpenAIResponse {
    choices: Vec<OpenAIChoice>,
    #[serde(default)]
    usage: Option<OpenAIUsage>,
}

#[derive(Deserialize)]
struct OpenAIChoice {
    message: OpenAIMessage,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Default, Deserialize)]
struct OpenAIUsage {
    #[serde(default)]
    prompt_tokens: Option<u32>,
    #[serde(default)]
    completion_tokens: Option<u32>,
    #[serde(default)]
    total_tokens: Option<u32>,
}

/// Anthropic Claude provider
pub struct AnthropicLlmProvider {
    config: LlmProviderConfig,
    client: reqwest::Client,
    metrics: RetryMetrics,
    prompt_manager: PromptManager<FilePromptStore>,
}

impl AnthropicLlmProvider {
    pub fn new(config: LlmProviderConfig) -> Result<Self, RuntimeError> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(
                config.timeout_seconds.unwrap_or(30),
            ))
            .build()
            .map_err(|e| RuntimeError::Generic(format!("Failed to create HTTP client: {}", e)))?;

        // Assets are at workspace root, so try ../assets first, then assets (for when run from workspace root)
        let prompt_path = if std::path::Path::new("../assets/prompts/arbiter").exists() {
            "../assets/prompts/arbiter"
        } else {
            "assets/prompts/arbiter"
        };
        let prompt_store = FilePromptStore::new(prompt_path);
        let prompt_manager = PromptManager::new(prompt_store);

        Ok(Self {
            config,
            client,
            metrics: RetryMetrics::new(),
            prompt_manager,
        })
    }

    /// Get current retry metrics summary
    pub fn get_retry_metrics(&self) -> RetryMetricsSummary {
        self.metrics.get_summary()
    }

    async fn make_request(
        &self,
        messages: Vec<AnthropicMessage>,
    ) -> Result<LlmCompletion, RuntimeError> {
        let api_key = self.config.api_key.as_ref().ok_or_else(|| {
            RuntimeError::Generic("API key required for Anthropic provider".to_string())
        })?;

        let base_url = self
            .config
            .base_url
            .as_deref()
            .unwrap_or("https://api.anthropic.com/v1");
        let url = format!("{}/messages", base_url);

        let request_body = AnthropicRequest {
            model: self.config.model.clone(),
            messages,
            max_tokens: self.config.max_tokens,
            temperature: self.config.temperature,
        };

        let payload_bytes = serde_json::to_vec(&request_body).map_err(|e| {
            RuntimeError::Generic(format!("Failed to serialize request body: {}", e))
        })?;
        let prompt_hash = sha256_hex(&payload_bytes);

        let start = Instant::now();
        let response = self
            .client
            .post(&url)
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01")
            .header("Content-Type", "application/json")
            .body(payload_bytes)
            .send()
            .await
            .map_err(|e| RuntimeError::Generic(format!("HTTP request failed: {}", e)))?;

        let status = response.status();
        let raw_body = response
            .text()
            .await
            .unwrap_or_else(|_| "Unknown error".to_string());

        if !status.is_success() {
            return Err(RuntimeError::Generic(format!(
                "API request failed: {}",
                raw_body
            )));
        }

        let response_hash = sha256_hex(raw_body.as_bytes());

        let response_body: AnthropicResponse = serde_json::from_str(&raw_body)
            .map_err(|e| RuntimeError::Generic(format!("Failed to parse response: {}", e)))?;

        let content = response_body
            .content
            .first()
            .map(|item| item.text.clone())
            .ok_or_else(|| RuntimeError::Generic("LLM response missing content".to_string()))?;

        let usage = response_body.usage.unwrap_or_default();
        let total_tokens = match (usage.input_tokens, usage.output_tokens) {
            (Some(input), Some(output)) => Some(input + output),
            _ => None,
        };
        let elapsed_ms = start.elapsed().as_millis();

        Ok(LlmCompletion {
            content,
            prompt_hash,
            response_hash,
            prompt_tokens: usage.input_tokens,
            completion_tokens: usage.output_tokens,
            total_tokens,
            latency_ms: elapsed_ms,
        })
    }

    fn parse_intent_from_json(&self, json_str: &str) -> Result<StorableIntent, RuntimeError> {
        // Try to extract JSON from the response (it might be wrapped in markdown)
        let json_start = json_str.find('{').unwrap_or(0);
        let json_end = json_str.rfind('}').map(|i| i + 1).unwrap_or(json_str.len());
        let json_content = &json_str[json_start..json_end];

        #[derive(Deserialize)]
        struct IntentJson {
            name: Option<String>,
            goal: String,
            constraints: Option<HashMap<String, String>>,
            preferences: Option<HashMap<String, String>>,
            success_criteria: Option<String>,
        }

        let intent_json: IntentJson = serde_json::from_str(json_content)
            .map_err(|e| RuntimeError::Generic(format!("Failed to parse intent JSON: {}", e)))?;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        Ok(StorableIntent {
            intent_id: format!("anthropic_intent_{}", uuid::Uuid::new_v4()),
            name: intent_json.name,
            original_request: "".to_string(), // Will be set by caller
            rtfs_intent_source: "".to_string(),
            goal: intent_json.goal,
            constraints: intent_json.constraints.unwrap_or_default(),
            preferences: intent_json.preferences.unwrap_or_default(),
            success_criteria: intent_json.success_criteria,
            parent_intent: None,
            child_intents: vec![],
            triggered_by: TriggerSource::HumanRequest,
            generation_context: GenerationContext {
                arbiter_version: "anthropic-provider-1.0".to_string(),
                generation_timestamp: now,
                input_context: HashMap::new(),
                reasoning_trace: None,
            },
            status: IntentStatus::Active,
            priority: 0,
            created_at: now,
            updated_at: now,
            metadata: HashMap::new(),
        })
    }

    fn parse_plan_from_json(&self, json_str: &str, intent_id: &str) -> Result<Plan, RuntimeError> {
        // Try to extract JSON from the response
        let json_start = json_str.find('{').unwrap_or(0);
        let json_end = json_str.rfind('}').map(|i| i + 1).unwrap_or(json_str.len());
        let json_content = &json_str[json_start..json_end];

        #[derive(Deserialize)]
        struct PlanJson {
            name: Option<String>,
            steps: Vec<String>,
        }

        let plan_json: PlanJson = serde_json::from_str(json_content)
            .map_err(|e| RuntimeError::Generic(format!("Failed to parse plan JSON: {}", e)))?;

        let rtfs_body = format!("(do\n  {}\n)", plan_json.steps.join("\n  "));

        Ok(Plan {
            plan_id: format!("anthropic_plan_{}", uuid::Uuid::new_v4()),
            name: plan_json.name,
            intent_ids: vec![intent_id.to_string()],
            language: PlanLanguage::Rtfs20,
            body: PlanBody::Rtfs(rtfs_body),
            status: crate::types::PlanStatus::Draft,
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            metadata: HashMap::new(),
            input_schema: None,
            output_schema: None,
            policies: HashMap::new(),
            capabilities_required: vec![],
            annotations: HashMap::new(),
        })
    }
}

#[async_trait]
impl LlmProvider for AnthropicLlmProvider {
    async fn generate_intent(
        &self,
        prompt: &str,
        context: Option<HashMap<String, String>>,
    ) -> Result<StorableIntent, RuntimeError> {
        // Load prompt from assets with fallback
        let vars = HashMap::from([("user_request".to_string(), prompt.to_string())]);

        let system_message = self.prompt_manager
            .render("intent_generation", "v1", &vars)
            .unwrap_or_else(|e| {
                eprintln!("Warning: Failed to load intent_generation prompt from assets: {}. Using fallback.", e);
                r#"You are an AI assistant that converts natural language requests into structured intents for a cognitive computing system.

Generate a JSON response with the following structure:
{
  "name": "descriptive_name_for_intent",
  "goal": "clear_description_of_what_should_be_achieved",
  "constraints": {
    "constraint_name": "constraint_value_as_string"
  },
  "preferences": {
    "preference_name": "preference_value_as_string"
  },
  "success_criteria": "how_to_determine_if_intent_was_successful"
}

IMPORTANT: All values in constraints and preferences must be strings, not numbers or arrays.
Examples:
- "max_cost": "100" (not 100)
- "priority": "high" (not ["high"])
- "timeout": "30_seconds" (not 30)

Only respond with valid JSON."#.to_string()
            });

        let user_message = if let Some(ctx) = context {
            let context_str = ctx
                .iter()
                .map(|(k, v)| format!("{}: {}", k, v))
                .collect::<Vec<_>>()
                .join("\n");
            format!("Context:\n{}\n\nRequest: {}", context_str, prompt)
        } else {
            prompt.to_string()
        };

        // Optional: display prompts during live runtime when enabled
        let show_prompts = std::env::var("RTFS_SHOW_PROMPTS")
            .map(|v| v == "1")
            .unwrap_or(false)
            || std::env::var("CCOS_DEBUG")
                .map(|v| v == "1")
                .unwrap_or(false);

        if show_prompts {
            println!(
                "\n=== LLM Intent Generation Prompt (Anthropic) ===\n[system]\n{}\n\n[user]\n{}\n=== END PROMPT ===\n",
                system_message,
                user_message
            );
        }

        let messages = vec![AnthropicMessage {
            role: "user".to_string(),
            content: format!("{}\n\n{}", system_message, user_message),
        }];

        let completion = self.make_request(messages).await?;

        if show_prompts {
            println!(
                "\n=== LLM Raw Response (Intent Generation - Anthropic) ===\n{}\n=== END RESPONSE ===\n",
                completion.content
            );
        }

        let mut intent = self.parse_intent_from_json(&completion.content)?;
        intent.original_request = prompt.to_string();
        attach_completion_metadata_to_intent(&mut intent, &self.config, &completion);

        Ok(intent)
    }

    async fn generate_plan(
        &self,
        intent: &StorableIntent,
        _context: Option<HashMap<String, String>>,
    ) -> Result<Plan, RuntimeError> {
        let system_message = r#"You are an AI assistant that generates executable plans from structured intents.

Generate a JSON response with the following structure:
{
  "name": "descriptive_plan_name",
  "steps": [
    "step 1 description",
    "step 2 description",
    "step 3 description"
  ]
}

Each step should be a clear, actionable instruction that can be executed by the system.
Only respond with valid JSON."#;

        let user_message = format!(
            "Intent: {}\nGoal: {}\nConstraints: {:?}\nPreferences: {:?}\nSuccess Criteria: {:?}",
            intent.name.as_deref().unwrap_or("unnamed"),
            intent.goal,
            intent.constraints,
            intent.preferences,
            intent.success_criteria.as_deref().unwrap_or("none")
        );

        let messages = vec![AnthropicMessage {
            role: "user".to_string(),
            content: format!("{}\n\n{}", system_message, user_message),
        }];

        let completion = self.make_request(messages).await?;
        let mut plan = self.parse_plan_from_json(&completion.content, &intent.intent_id)?;
        attach_completion_metadata_to_plan(&mut plan, &self.config, &completion);
        Ok(plan)
    }

    async fn validate_plan(&self, plan_content: &str) -> Result<ValidationResult, RuntimeError> {
        let system_message = r#"You are an AI assistant that validates executable plans.

Analyze the provided plan and return a JSON response with the following structure:
{
  "is_valid": true/false,
  "confidence": 0.0-1.0,
  "reasoning": "explanation of validation decision",
  "suggestions": ["suggestion1", "suggestion2"],
  "errors": ["error1", "error2"]
}

Only respond with valid JSON."#;

        let user_message = format!("Plan to validate:\n{}", plan_content);

        let messages = vec![AnthropicMessage {
            role: "user".to_string(),
            content: format!("{}\n\n{}", system_message, user_message),
        }];

        let completion = self.make_request(messages).await?;
        let response = completion.content;

        // Try to extract JSON from the response
        let json_start = response.find('{').unwrap_or(0);
        let json_end = response.rfind('}').map(|i| i + 1).unwrap_or(response.len());
        let json_content = &response[json_start..json_end];

        serde_json::from_str(json_content)
            .map_err(|e| RuntimeError::Generic(format!("Failed to parse validation JSON: {}", e)))
    }

    async fn generate_text(&self, prompt: &str) -> Result<String, RuntimeError> {
        let messages = vec![AnthropicMessage {
            role: "user".to_string(),
            content: prompt.to_string(),
        }];

        let completion = self.make_request(messages).await?;
        Ok(completion.content)
    }

    fn get_info(&self) -> LlmProviderInfo {
        LlmProviderInfo {
            name: "Anthropic Claude".to_string(),
            version: "1.0".to_string(),
            model: self.config.model.clone(),
            capabilities: vec![
                "intent_generation".to_string(),
                "plan_generation".to_string(),
                "plan_validation".to_string(),
            ],
        }
    }
}

// Anthropic API types
#[derive(Serialize)]
struct AnthropicRequest {
    model: String,
    messages: Vec<AnthropicMessage>,
    max_tokens: Option<u32>,
    temperature: Option<f64>,
}

#[derive(Serialize)]
struct AnthropicMessage {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct AnthropicResponse {
    content: Vec<AnthropicContent>,
    #[serde(default)]
    usage: Option<AnthropicUsage>,
}

#[derive(Deserialize)]
struct AnthropicContent {
    text: String,
}

#[derive(Default, Deserialize)]
struct AnthropicUsage {
    #[serde(default)]
    input_tokens: Option<u32>,
    #[serde(default)]
    output_tokens: Option<u32>,
}

/// Stub LLM provider for testing and development
pub struct StubLlmProvider {
    config: LlmProviderConfig,
}

impl StubLlmProvider {
    pub fn new(config: LlmProviderConfig) -> Self {
        Self { config }
    }

    /// Generate a deterministic storable intent based on natural language
    fn generate_stub_intent(&self, nl: &str) -> StorableIntent {
        let lower_nl = nl.to_lowercase();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        if lower_nl.contains("sentiment") || lower_nl.contains("analyze") {
            StorableIntent {
                intent_id: format!("stub_sentiment_{}", uuid::Uuid::new_v4()),
                name: Some("analyze_user_sentiment".to_string()),
                original_request: nl.to_string(),
                rtfs_intent_source: "".to_string(),
                goal: "Analyze user sentiment from interactions".to_string(),
                constraints: HashMap::from([("accuracy".to_string(), "\"high\"".to_string())]),
                preferences: HashMap::from([("speed".to_string(), "\"medium\"".to_string())]),
                success_criteria: Some("\"sentiment_analyzed\"".to_string()),
                parent_intent: None,
                child_intents: vec![],
                triggered_by: TriggerSource::HumanRequest,
                generation_context: GenerationContext {
                    arbiter_version: "stub-1.0".to_string(),
                    generation_timestamp: now,
                    input_context: HashMap::new(),
                    reasoning_trace: None,
                },
                status: IntentStatus::Active,
                priority: 0,
                created_at: now,
                updated_at: now,
                metadata: HashMap::new(),
            }
        } else if lower_nl.contains("optimize") || lower_nl.contains("improve") {
            StorableIntent {
                intent_id: format!("stub_optimize_{}", uuid::Uuid::new_v4()),
                name: Some("optimize_system_performance".to_string()),
                original_request: nl.to_string(),
                rtfs_intent_source: "".to_string(),
                goal: "Optimize system performance".to_string(),
                constraints: HashMap::from([("budget".to_string(), "\"low\"".to_string())]),
                preferences: HashMap::from([("speed".to_string(), "\"high\"".to_string())]),
                success_criteria: Some("\"performance_optimized\"".to_string()),
                parent_intent: None,
                child_intents: vec![],
                triggered_by: TriggerSource::HumanRequest,
                generation_context: GenerationContext {
                    arbiter_version: "stub-1.0".to_string(),
                    generation_timestamp: now,
                    input_context: HashMap::new(),
                    reasoning_trace: None,
                },
                status: IntentStatus::Active,
                priority: 0,
                created_at: now,
                updated_at: now,
                metadata: HashMap::new(),
            }
        } else {
            // Default intent
            StorableIntent {
                intent_id: format!("stub_general_{}", uuid::Uuid::new_v4()),
                name: Some("general_assistance".to_string()),
                original_request: nl.to_string(),
                rtfs_intent_source: "".to_string(),
                goal: "Perform a small delegated task".to_string(),
                constraints: HashMap::new(),
                preferences: HashMap::from([("helpfulness".to_string(), "\"high\"".to_string())]),
                success_criteria: Some("\"assistance_provided\"".to_string()),
                parent_intent: None,
                child_intents: vec![],
                triggered_by: TriggerSource::HumanRequest,
                generation_context: GenerationContext {
                    arbiter_version: "stub-1.0".to_string(),
                    generation_timestamp: now,
                    input_context: HashMap::new(),
                    reasoning_trace: None,
                },
                status: IntentStatus::Active,
                priority: 0,
                created_at: now,
                updated_at: now,
                metadata: HashMap::new(),
            }
        }
    }

    /// Generate a deterministic plan based on intent
    fn generate_stub_plan(&self, intent: &StorableIntent) -> Plan {
        let plan_body = match intent.name.as_deref() {
            Some("analyze_user_sentiment") => {
                r#"
(do
    (step "Fetch User Data" (call :ccos.echo "fetched user interactions"))
    (step "Analyze Sentiment" (call :ccos.echo "sentiment analysis completed"))
    (step "Generate Report" (call :ccos.echo "sentiment report generated"))
)
"#
            }
            Some("optimize_system_performance") => {
                r#"
(do
    (step "Collect Metrics" (call :ccos.echo "system metrics collected"))
    (step "Identify Bottlenecks" (call :ccos.echo "bottlenecks identified"))
    (step "Apply Optimizations" (call :ccos.echo "optimizations applied"))
    (step "Verify Improvements" (call :ccos.echo "performance improvements verified"))
)
"#
            }
            _ => {
                // If the intent mentions planning a trip (e.g., Paris), return a more
                // detailed multi-step RTFS plan to make examples and demos more useful.
                let goal_lower = intent.goal.to_lowercase();
                if goal_lower.contains("trip") || goal_lower.contains("paris") {
                    r#"
(do
    (step "Greet" (call :ccos.echo {:message "Let's plan your trip to Paris."}))
    (step "Collect Dates and Duration"
      (let [dates (call :ccos.user.ask "What dates will you travel to Paris?")
            duration (call :ccos.user.ask "How many days will you stay?")]
        (call :ccos.echo {:message (str "Dates: " dates ", duration: " duration)})))
    (step "Collect Preferences"
      (let [interests (call :ccos.user.ask "What activities are you interested in (museums, food, walks)?")
            budget (call :ccos.user.ask "Any budget constraints (low/medium/high)?")]
        (call :ccos.echo {:message (str "Prefs: " interests ", budget: " budget)})))
    (step "Assemble Itinerary" (call :ccos.echo {:message "Assembling a sample itinerary based on your preferences..."}))
    (step "Return Structured Summary"
      (let [dates (call :ccos.user.ask "Confirm travel dates (or type 'same')")
            duration (call :ccos.user.ask "Confirm duration in days (or type 'same')")
            interests (call :ccos.user.ask "Confirm interests (or type 'same')")]
        {:trip/destination "Paris"
         :trip/dates dates
         :trip/duration duration
         :trip/interests interests}))
)
"#
                } else {
                    r#"
(do
    (step "Process Request" (call :ccos.echo "processing your request"))
    (step "Complete Task" (call :ccos.echo "stub done"))
)
"#
                }
            }
        };

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        Plan {
            plan_id: format!("stub_plan_{}", uuid::Uuid::new_v4()),
            name: Some(format!(
                "stub_plan_for_{}",
                intent.name.as_deref().unwrap_or("general")
            )),
            intent_ids: vec![intent.intent_id.clone()],
            language: PlanLanguage::Rtfs20,
            body: PlanBody::Rtfs(plan_body.trim().to_string()),
            status: crate::types::PlanStatus::Draft,
            created_at: now,
            metadata: HashMap::new(),
            input_schema: None,
            output_schema: None,
            policies: HashMap::new(),
            capabilities_required: vec!["ccos.echo".to_string()],
            annotations: HashMap::new(),
        }
    }
}

#[async_trait]
impl LlmProvider for StubLlmProvider {
    async fn generate_intent(
        &self,
        prompt: &str,
        _context: Option<HashMap<String, String>>,
    ) -> Result<StorableIntent, RuntimeError> {
        // Optional: display prompts during live runtime when enabled
        let show_prompts = std::env::var("RTFS_SHOW_PROMPTS")
            .map(|v| v == "1")
            .unwrap_or(false)
            || std::env::var("CCOS_DEBUG")
                .map(|v| v == "1")
                .unwrap_or(false);

        if show_prompts {
            println!(
                "\n=== Stub Intent Generation ===\n[prompt]\n{}\n=== END PROMPT ===\n",
                prompt
            );
        }

        // For stub provider, we'll use a simple pattern matching approach
        // In a real implementation, this would parse the prompt and context
        let mut intent = self.generate_stub_intent(prompt);

        let response_text = format!("Stub intent synthesized for request: {}", prompt);
        let completion = LlmCompletion {
            content: response_text.clone(),
            prompt_hash: sha256_hex(prompt.as_bytes()),
            response_hash: sha256_hex(response_text.as_bytes()),
            prompt_tokens: None,
            completion_tokens: None,
            total_tokens: None,
            latency_ms: 0,
        };

        attach_completion_metadata_to_intent(&mut intent, &self.config, &completion);

        if show_prompts {
            println!(
                "\n=== Stub Intent Result ===\nIntent ID: {}\nGoal: {}\n=== END RESULT ===\n",
                intent.intent_id, intent.goal
            );
        }

        Ok(intent)
    }

    async fn generate_plan(
        &self,
        intent: &StorableIntent,
        _context: Option<HashMap<String, String>>,
    ) -> Result<Plan, RuntimeError> {
        // Optional: display prompts during live runtime when enabled
        let show_prompts = std::env::var("RTFS_SHOW_PROMPTS")
            .map(|v| v == "1")
            .unwrap_or(false)
            || std::env::var("CCOS_DEBUG")
                .map(|v| v == "1")
                .unwrap_or(false);

        if show_prompts {
            println!(
                "\n=== Stub Plan Generation ===\n[intent]\nGoal: {}\nConstraints: {:?}\nPreferences: {:?}\n=== END INPUT ===\n",
                intent.goal,
                intent.constraints,
                intent.preferences
            );
        }

        let mut plan = self.generate_stub_plan(intent);

        let synthetic_prompt = format!("Stub plan synthesis for intent: {}", intent.intent_id);
        let plan_body_text = match &plan.body {
            PlanBody::Source(body) | PlanBody::Rtfs(body) => body.clone(),
            PlanBody::Binary(_) | PlanBody::Wasm(_) => "(binary/wasm plan body)".to_string(),
        };
        let completion = LlmCompletion {
            content: plan_body_text.clone(),
            prompt_hash: sha256_hex(synthetic_prompt.as_bytes()),
            response_hash: sha256_hex(plan_body_text.as_bytes()),
            prompt_tokens: None,
            completion_tokens: None,
            total_tokens: None,
            latency_ms: 0,
        };

        attach_completion_metadata_to_plan(&mut plan, &self.config, &completion);

        if show_prompts {
            if let PlanBody::Rtfs(ref body) = plan.body {
                println!("\n=== Stub Plan Result ===\n{}\n=== END RESULT ===\n", body);
            }
        }

        Ok(plan)
    }

    async fn validate_plan(&self, _plan_content: &str) -> Result<ValidationResult, RuntimeError> {
        // Stub validation - always returns valid
        Ok(ValidationResult {
            is_valid: true,
            confidence: 0.95,
            reasoning: "Stub provider validation - always valid".to_string(),
            suggestions: vec!["Consider adding more specific steps".to_string()],
            errors: vec![],
        })
    }

    async fn generate_text(&self, prompt: &str) -> Result<String, RuntimeError> {
        // Check if this is a delegation analysis prompt
        let lower_prompt = prompt.to_lowercase();
        // Optional gate for demo-specific RTFS hint handling; default enabled, can disable with 0/false/off
        let demo_hints_enabled = std::env::var("CCOS_STUB_DEMO_HINTS")
            .map(|v| {
                let v = v.to_lowercase();
                !(v == "0" || v == "false" || v == "off")
            })
            .unwrap_or(true);

        // Handle demo prompt hints for strict RTFS vector/map outputs used by examples
        // 1) RTFS vector of clarifying questions
        if demo_hints_enabled
            && (lower_prompt.contains("rtfs vector of strings")
                || (lower_prompt.contains("rtfs vector") && lower_prompt.contains("questions"))
                || lower_prompt.contains("respond only with an rtfs vector"))
        {
            // Return 4-5 generic but sensible questions tailored for planning demos
            return Ok(
                "[\"What is your budget or cost sensitivity?\" \"What timeframe or deadline should we target?\" \"What specific subdomains or interests matter most?\" \"Any constraints on data sources or tools to use/avoid?\" \"What output format do you prefer (summary, report, code)?\"]"
                    .to_string(),
            );
        }
        // 2) RTFS map-only structured preferences extraction
        if demo_hints_enabled
            && (lower_prompt.contains("output only a single rtfs map")
                || lower_prompt.contains("strict rtfs format"))
        {
            // Try to extract a goal from the prompt for nicer output; fallback if not found
            let goal = prompt
                .splitn(2, ":goal \"")
                .nth(1)
                .and_then(|rest| rest.split('"').next())
                .unwrap_or("User goal");
            // Produce a minimal, well-formed RTFS map with a few parameters inferred from typical Q/A
            let rtfs = format!(
                "{{\n  :goal \"{}\"\n  :parameters {{\n    :budget {{:type :string :value \"medium\" :question \"What is your budget or cost sensitivity?\"}}\n    :duration {{:type :duration :value \"7 days\" :question \"What timeframe or deadline should we target?\"}}\n    :interests {{:type :list :value \"sightseeing, food, culture\" :question \"What specific subdomains or interests matter most?\"}}\n  }}\n}}",
                goal.replace('"', "\\\"")
            );
            return Ok(rtfs);
        }
        // Shortcut: detect arbiter graph-generation marker and return RTFS (do ...) intent graph
        if lower_prompt.contains("generate_intent_graph") || lower_prompt.contains("intent graph") {
            return Ok(r#"(do
  {:type "intent" :name "root" :goal "Say hi and add numbers"}
  {:type "intent" :name "greet" :goal "Greet the user"}
  {:type "intent" :name "compute" :goal "Add two numbers"}
  (edge :IsSubgoalOf "greet" "root")
  (edge :IsSubgoalOf "compute" "root")
  (edge :DependsOn "compute" "greet")
)"#
            .to_string());
        }

        if lower_prompt.contains("delegation analysis") || lower_prompt.contains("should_delegate")
        {
            // This is a delegation analysis request - return JSON
            if lower_prompt.contains("sentiment") || lower_prompt.contains("analyze") {
                Ok(r#"{
  "should_delegate": true,
  "reasoning": "Sentiment analysis requires specialized NLP capabilities available in sentiment_agent",
  "required_capabilities": ["sentiment_analysis", "text_processing"],
  "delegation_confidence": 0.92
}"#.to_string())
            } else if lower_prompt.contains("optimize") || lower_prompt.contains("performance") {
                Ok(r#"{
  "should_delegate": true,
  "reasoning": "Performance optimization requires specialized capabilities available in optimization_agent",
  "required_capabilities": ["performance_optimization", "system_analysis"],
  "delegation_confidence": 0.88
}"#.to_string())
            } else if lower_prompt.contains("backup") || lower_prompt.contains("database") {
                Ok(r#"{
  "should_delegate": true,
  "reasoning": "Database backup requires specialized backup and encryption capabilities available in backup_agent",
  "required_capabilities": ["backup", "encryption"],
  "delegation_confidence": 0.95
}"#.to_string())
            } else {
                // Default delegation analysis response
                Ok(r#"{
  "should_delegate": false,
  "reasoning": "Task can be handled directly without specialized agent delegation",
  "required_capabilities": ["general_processing"],
  "delegation_confidence": 0.75
}"#
                .to_string())
            }
        } else if lower_prompt
            .contains("delegating arbiter llm tasked with synthesizing a new rtfs capability")
        {
            Ok(r#"(do
    (capability "stub.generated.capability.v1"
        :description "Stub capability generated by delegating arbiter"
        :parameters {:context "map"}
        :implementation (do {:status "ready_for_execution" :context context}))
    (plan
        :plan-id "stub.generated.plan.v1"
        :language "rtfs20"
        :body "(do\n  (let [ctx context]\n    {:status \"completed\"\n     :context ctx\n     :result {:message \"stub capability executed\" :context ctx}}))"
        :needs_capabilities [:stub.generated.capability.v1])
)"#.to_string())
        } else {
            // Regular intent generation - returns RTFS intent
            if lower_prompt.contains("sentiment") || lower_prompt.contains("analyze") {
                Ok(r#"(intent "analyze_user_sentiment"
  :goal "Analyze user sentiment from interactions and provide insights"
  :constraints {
    :accuracy (> confidence 0.85)
    :privacy :maintain-user-privacy
  }
  :preferences {
    :speed :medium
    :detail :comprehensive
  }
  :success-criteria (and (sentiment-analyzed? data) (> confidence 0.85)))"#
                    .to_string())
            } else if lower_prompt.contains("optimize")
                || lower_prompt.contains("improve")
                || lower_prompt.contains("performance")
            {
                Ok(r#"(intent "optimize_system_performance"
  :goal "Optimize system performance and efficiency"
  :constraints {
    :budget (< cost 1000)
    :downtime (< downtime 0.01)
  }
  :preferences {
    :speed :high
    :method :automated
  }
  :success-criteria (and (> performance 0.2) (< latency 100)))"#
                    .to_string())
            } else if lower_prompt.contains("backup") || lower_prompt.contains("database") {
                Ok(r#"(intent "create_database_backup"
  :goal "Create a comprehensive backup of the database"
  :constraints {
    :integrity :maintain-data-integrity
    :availability (> uptime 0.99)
  }
  :preferences {
    :compression :high
    :encryption :enabled
  }
  :success-criteria (and (backup-created? db) (backup-verified? db)))"#
                    .to_string())
            } else if lower_prompt.contains("machine learning")
                || lower_prompt.contains("ml")
                || lower_prompt.contains("pipeline")
            {
                Ok(r#"(intent "create_ml_pipeline"
  :goal "Create a machine learning pipeline for data processing"
  :constraints {
    :accuracy (> model-accuracy 0.9)
    :scalability :handle-large-datasets
  }
  :preferences {
    :framework :tensorflow
    :deployment :cloud
  }
  :success-criteria (and (pipeline-deployed? ml) (> accuracy 0.9)))"#
                    .to_string())
            } else if lower_prompt.contains("microservices")
                || lower_prompt.contains("architecture")
            {
                Ok(r#"(intent "design_microservices_architecture"
  :goal "Design a scalable microservices architecture"
  :constraints {
    :scalability :horizontal-scaling
    :reliability (> uptime 0.999)
  }
  :preferences {
    :technology :kubernetes
    :communication :rest-api
  }
  :success-criteria (and (architecture-designed? ms) (deployment-ready? ms)))"#
                    .to_string())
            } else if lower_prompt.contains("real-time") || lower_prompt.contains("streaming") {
                Ok(r#"(intent "implement_realtime_processing"
  :goal "Implement real-time data processing with streaming analytics"
  :constraints {
    :latency (< processing-time 100)
    :throughput (> events-per-second 10000)
  }
  :preferences {
    :technology :apache-kafka
    :processing :streaming
  }
  :success-criteria (and (streaming-active? rt) (< latency 100)))"#
                    .to_string())
            } else {
                Err(RuntimeError::Generic(
                    "Stub LLM provider fallback triggered; configure a real provider or supply explicit stub hints"
                        .to_string(),
                ))
            }
        }
    }

    fn get_info(&self) -> LlmProviderInfo {
        LlmProviderInfo {
            name: "Stub LLM Provider".to_string(),
            version: "1.0.0".to_string(),
            model: self.config.model.clone(),
            capabilities: vec![
                "intent_generation".to_string(),
                "plan_generation".to_string(),
                "plan_validation".to_string(),
            ],
        }
    }
}

/// Factory for creating LLM providers
pub struct LlmProviderFactory;

impl LlmProviderFactory {
    /// Create an LLM provider based on configuration
    pub async fn create_provider(
        config: LlmProviderConfig,
    ) -> Result<Box<dyn LlmProvider>, RuntimeError> {
        match config.provider_type {
            LlmProviderType::Stub => {
                // Only allow Stub provider in test mode or when explicitly enabled
                let allow_stub = std::env::var("CCOS_ALLOW_STUB_PROVIDER")
                    .map(|v| v == "1" || v == "true")
                    .unwrap_or(false)
                    || cfg!(test);

                if !allow_stub {
                    return Err(RuntimeError::Generic(
                        "Stub LLM provider is not allowed in production. Set CCOS_ALLOW_STUB_PROVIDER=1 to enable (for testing only), or use a real provider (openai, anthropic, openrouter).".to_string()
                    ));
                }

                eprintln!("⚠️  WARNING: Using Stub LLM Provider (testing only - not realistic)");
                eprintln!("   Set CCOS_LLM_PROVIDER=openai or CCOS_LLM_PROVIDER=anthropic for real LLM calls");
                Ok(Box::new(StubLlmProvider::new(config)))
            }
            LlmProviderType::OpenAI => {
                let provider = OpenAILlmProvider::new(config)?;
                Ok(Box::new(provider))
            }
            LlmProviderType::Anthropic => {
                let provider = AnthropicLlmProvider::new(config)?;
                Ok(Box::new(provider))
            }
            LlmProviderType::Local => {
                // TODO: Implement Local provider
                Err(RuntimeError::Generic(
                    "Local provider not yet implemented".to_string(),
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_stub_provider_intent_generation() {
        let config = LlmProviderConfig {
            provider_type: LlmProviderType::Stub,
            model: "stub-model".to_string(),
            api_key: None,
            base_url: None,
            max_tokens: None,
            temperature: None,
            timeout_seconds: None,
            retry_config: crate::arbiter::arbiter_config::RetryConfig::default(),
        };

        let provider = StubLlmProvider::new(config);
        let intent = provider
            .generate_intent("analyze sentiment", None)
            .await
            .unwrap();

        // The stub provider responds based on prompt content
        assert_eq!(intent.name, Some("analyze_user_sentiment".to_string()));
        assert!(intent.goal.contains("Analyze user sentiment"));
    }

    #[tokio::test]
    async fn test_stub_provider_plan_generation() {
        let config = LlmProviderConfig {
            provider_type: LlmProviderType::Stub,
            model: "stub-model".to_string(),
            api_key: None,
            base_url: None,
            max_tokens: None,
            temperature: None,
            timeout_seconds: None,
            retry_config: crate::arbiter::arbiter_config::RetryConfig::default(),
        };

        let provider = StubLlmProvider::new(config);
        let intent = provider
            .generate_intent("optimize performance", None)
            .await
            .unwrap();
        let plan = provider.generate_plan(&intent, None).await.unwrap();

        // The stub provider responds based on intent content
        assert_eq!(
            plan.name,
            Some("stub_plan_for_optimize_system_performance".to_string())
        );
        assert!(matches!(plan.body, PlanBody::Rtfs(_)));
    }

    #[tokio::test]
    async fn test_stub_provider_validation() {
        let config = LlmProviderConfig {
            provider_type: LlmProviderType::Stub,
            model: "stub-model".to_string(),
            api_key: None,
            base_url: None,
            max_tokens: None,
            temperature: None,
            timeout_seconds: None,
            retry_config: crate::arbiter::arbiter_config::RetryConfig::default(),
        };

        let provider = StubLlmProvider::new(config);
        let intent = provider.generate_intent("test", None).await.unwrap();
        let plan = provider.generate_plan(&intent, None).await.unwrap();

        // Extract plan content for validation
        let plan_content = match &plan.body {
            PlanBody::Source(content) | PlanBody::Rtfs(content) => content.as_str(),
            PlanBody::Binary(_) | PlanBody::Wasm(_) => "(binary/wasm plan)",
        };

        let validation = provider.validate_plan(plan_content).await.unwrap();

        assert!(validation.is_valid);
        assert!(validation.confidence > 0.9);
        assert!(!validation.reasoning.is_empty());
    }

    #[tokio::test]
    async fn test_stub_provider_intent_metadata() {
        let config = LlmProviderConfig {
            provider_type: LlmProviderType::Stub,
            model: "stub-metadata-model".to_string(),
            api_key: None,
            base_url: None,
            max_tokens: None,
            temperature: None,
            timeout_seconds: None,
            retry_config: crate::arbiter::arbiter_config::RetryConfig::default(),
        };

        let model = config.model.clone();
        let provider = StubLlmProvider::new(config);
        let prompt = "Plan a trip to Paris";
        let intent = provider.generate_intent(prompt, None).await.unwrap();

        assert_eq!(intent.metadata.get("llm.model"), Some(&model));
        assert_eq!(
            intent.metadata.get("llm.provider"),
            Some(&format!("{:?}", LlmProviderType::Stub)),
        );
        assert_eq!(
            intent.metadata.get("llm.latency_ms"),
            Some(&"0".to_string())
        );

        let expected_prompt_hash = sha256_hex(prompt.as_bytes());
        assert_eq!(
            intent.metadata.get("llm.prompt_hash"),
            Some(&expected_prompt_hash),
        );

        let response_text = format!("Stub intent synthesized for request: {}", prompt);
        let expected_response_hash = sha256_hex(response_text.as_bytes());
        assert_eq!(
            intent.metadata.get("llm.response_hash"),
            Some(&expected_response_hash),
        );

        assert!(intent.metadata.get("llm.total_tokens").is_none());
        assert!(intent.metadata.get("llm.prompt_tokens").is_none());
        assert!(intent.metadata.get("llm.completion_tokens").is_none());
    }

    #[tokio::test]
    async fn test_stub_provider_plan_metadata() {
        let config = LlmProviderConfig {
            provider_type: LlmProviderType::Stub,
            model: "stub-plan-model".to_string(),
            api_key: None,
            base_url: None,
            max_tokens: None,
            temperature: None,
            timeout_seconds: None,
            retry_config: crate::arbiter::arbiter_config::RetryConfig::default(),
        };

        let model = config.model.clone();
        let provider = StubLlmProvider::new(config);
        let intent = provider
            .generate_intent("optimize performance", None)
            .await
            .unwrap();
        let plan = provider.generate_plan(&intent, None).await.unwrap();

        let expected_prompt = format!("Stub plan synthesis for intent: {}", intent.intent_id);
        let expected_prompt_hash = sha256_hex(expected_prompt.as_bytes());

        let plan_body_text = match &plan.body {
            PlanBody::Source(body) | PlanBody::Rtfs(body) => body.clone(),
            PlanBody::Binary(_) | PlanBody::Wasm(_) => "(binary/wasm plan body)".to_string(),
        };
        let expected_response_hash = sha256_hex(plan_body_text.as_bytes());

        assert_eq!(
            plan.metadata.get("llm.model"),
            Some(&Value::String(model.clone())),
        );
        assert_eq!(
            plan.metadata.get("llm.provider"),
            Some(&Value::String(format!("{:?}", LlmProviderType::Stub))),
        );
        assert_eq!(
            plan.metadata.get("llm.latency_ms"),
            Some(&Value::Integer(0)),
        );
        assert_eq!(
            plan.metadata.get("llm.prompt_hash"),
            Some(&Value::String(expected_prompt_hash)),
        );
        assert_eq!(
            plan.metadata.get("llm.response_hash"),
            Some(&Value::String(expected_response_hash)),
        );

        assert!(plan.metadata.get("llm.total_tokens").is_none());
        assert!(plan.metadata.get("llm.prompt_tokens").is_none());
        assert!(plan.metadata.get("llm.completion_tokens").is_none());
    }

    #[tokio::test]
    async fn test_anthropic_provider_creation() {
        let config = LlmProviderConfig {
            provider_type: LlmProviderType::Anthropic,
            model: "claude-3-sonnet-20240229".to_string(),
            api_key: Some("test-key".to_string()),
            base_url: None,
            max_tokens: Some(1000),
            temperature: Some(0.7),
            timeout_seconds: Some(30),
            retry_config: crate::arbiter::arbiter_config::RetryConfig::default(),
        };

        // Test that provider can be created (even without valid API key)
        let provider = AnthropicLlmProvider::new(config);
        assert!(provider.is_ok());

        let provider = provider.unwrap();
        let info = provider.get_info();
        assert_eq!(info.name, "Anthropic Claude");
        assert_eq!(info.version, "1.0");
        assert!(info.capabilities.contains(&"intent_generation".to_string()));
        assert!(info.capabilities.contains(&"plan_generation".to_string()));
        assert!(info.capabilities.contains(&"plan_validation".to_string()));
    }

    #[tokio::test]
    async fn test_anthropic_provider_factory() {
        let config = LlmProviderConfig {
            provider_type: LlmProviderType::Anthropic,
            model: "claude-3-sonnet-20240229".to_string(),
            api_key: Some("test-key".to_string()),
            base_url: None,
            max_tokens: Some(1000),
            temperature: Some(0.7),
            timeout_seconds: Some(30),
            retry_config: crate::arbiter::arbiter_config::RetryConfig::default(),
        };

        // Test that factory can create Anthropic provider
        let provider = LlmProviderFactory::create_provider(config).await;
        assert!(provider.is_ok());

        let provider = provider.unwrap();
        let info = provider.get_info();
        assert_eq!(info.name, "Anthropic Claude");
    }

    #[test]
    fn test_extract_do_block_simple() {
        let text = r#"
Some header text
(do
    (step \"A\" (call :ccos.echo {:message \"hi\"}))
    (step \"B\" (call :ccos.math.add 2 3))
)
Trailing
"#;
        let do_block = OpenAILlmProvider::extract_do_block(text).expect("should find do block");
        assert!(do_block.starts_with("(do"));
        assert!(do_block.contains(":ccos.echo"));
        assert!(do_block.ends_with(")"));
    }

    #[test]
    fn test_extract_plan_block_and_name_and_body() {
        let text = r#"
Intro
(plan
    :name "Sample Plan"
    :language rtfs20
    :body (do
                     (step "Greet" (call :ccos.echo {:message "hi"}))
                     (step "Add" (call :ccos.math.add 2 3)))
    :annotations {:source "unit"}
)
Footer
"#;

        let plan_block =
            OpenAILlmProvider::extract_plan_block(text).expect("should find plan block");
        assert!(plan_block.starts_with("(plan"));
        let name = OpenAILlmProvider::extract_quoted_value_after_key(&plan_block, ":name")
            .expect("should extract name");
        assert_eq!(name, "Sample Plan");
        let do_block =
            OpenAILlmProvider::extract_do_block(&plan_block).expect("should find nested do block");
        assert!(do_block.contains(":ccos.math.add 2 3"));
    }

    #[test]
    fn test_extract_plan_block_with_fences_and_prose() {
        let text = r#"
Here is your plan. I've ensured it follows the schema:

```rtfs
(plan
  :name "Fenced Plan"
  :language rtfs20
  :body (do
       (step "Say" (call :ccos.echo {:message "yo"}))
       (step "Sum" (call :ccos.math.add 1 2)))
)
```

Some trailing commentary that should be ignored.
"#;

        let plan_block =
            OpenAILlmProvider::extract_plan_block(text).expect("should find plan inside fences");
        assert!(plan_block.starts_with("(plan"));
        let do_block =
            OpenAILlmProvider::extract_do_block(&plan_block).expect("nested do should be found");
        assert!(do_block.contains(":ccos.echo"));
    }

    #[test]
    fn test_extract_do_block_with_fences_and_prefix() {
        let text = r#"
Model: Here's the body you requested:

```lisp
(do
  (step "One" (call :ccos.echo {:message "a"}))
  (step "Two" (call :ccos.math.add 3 4))
)
```
"#;

        let do_block =
            OpenAILlmProvider::extract_do_block(text).expect("should find do inside fences");
        assert!(do_block.starts_with("(do"));
        assert!(parser::parse(&do_block).is_ok());
    }

    #[test]
    fn test_extract_quoted_value_after_key_multiple_occurrences() {
        let text = r#"
(plan
  :name "First"
  :annotations {:name "not this one"}
  :body (do (step "n" (call :ccos.echo {:message "m"})))
)
"#;
        let plan_block = OpenAILlmProvider::extract_plan_block(text).unwrap();
        let name = OpenAILlmProvider::extract_quoted_value_after_key(&plan_block, ":name").unwrap();
        assert_eq!(name, "First");
    }

    #[test]
    fn test_extract_do_after_body_key_normal() {
        let text = r#"
(plan
  :name "X"
  :language rtfs20
  :body (do
      (step "A" (call :ccos.echo {:message "m"}))
      (step "B" (call :ccos.math.add 5 6)))
)
"#;
        let plan_block = OpenAILlmProvider::extract_plan_block(text).unwrap();
        let do_block = OpenAILlmProvider::extract_s_expr_after_key(&plan_block, ":body").unwrap();
        assert!(do_block.starts_with("(do"));
        assert!(do_block.contains(":ccos.math.add 5 6"));
    }

    #[test]
    fn test_extract_do_after_body_key_missing_returns_none() {
        let text = r#"
(plan
  :name "No Body"
  :language rtfs20
  :annotations {:note "no body key"}
)
"#;
        let plan_block = OpenAILlmProvider::extract_plan_block(text).unwrap();
        assert!(OpenAILlmProvider::extract_s_expr_after_key(&plan_block, ":body").is_none());
    }

    #[test]
    fn test_extract_do_after_body_skips_quoted_parens() {
        let text = r#"
(plan
  :name "Quoted"
  :body "not this (do wrong)"
  :body (do (step "Only" (call :ccos.echo {:message "ok"})))
)
"#;
        let plan_block = OpenAILlmProvider::extract_plan_block(text).unwrap();
        let do_block = OpenAILlmProvider::extract_s_expr_after_key(&plan_block, ":body").unwrap();
        assert!(do_block.contains(":ccos.echo"));
    }
}
