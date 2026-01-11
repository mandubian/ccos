//! LLM Operations Module
//!
//! Provides LLM-based text generation with governance controls.
//! All prompts are sanitized through GovernanceKernel before execution.

use crate::cognitive_engine::DelegatingCognitiveEngine;
use once_cell::sync::OnceCell;
use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Global arbiter reference for LLM operations
/// Set during CCOS initialization via set_global_arbiter()
static GLOBAL_ARBITER: OnceCell<Arc<DelegatingCognitiveEngine>> = OnceCell::new();

/// Set the global arbiter for LLM operations.
/// Called during CCOS initialization.
pub fn set_global_arbiter(arbiter: Arc<DelegatingCognitiveEngine>) {
    let _ = GLOBAL_ARBITER.set(arbiter);
}

/// Get the global arbiter for LLM operations.
pub fn get_global_arbiter() -> Option<Arc<DelegatingCognitiveEngine>> {
    GLOBAL_ARBITER.get().cloned()
}

/// LLM generate request parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmGenerateRequest {
    /// The prompt to send to the LLM
    pub prompt: String,
    /// Optional context data to include (serialized as JSON)
    pub context: Option<String>,
    /// Maximum tokens to generate (default: 4096)
    pub max_tokens: Option<u32>,
    /// Temperature for sampling (default: 0.3)
    pub temperature: Option<f32>,
}

/// LLM generate response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmGenerateResponse {
    /// The generated text
    pub text: String,
    /// Number of tokens used (input + output)
    pub tokens_used: Option<u32>,
    /// Whether approval was required
    pub approval_required: bool,
    /// Approval reason if required
    pub approval_reason: Option<String>,
}

/// Default max tokens - should be large enough for grammar.md
const DEFAULT_MAX_TOKENS: u32 = 4096;

/// Default temperature for more deterministic output
const DEFAULT_TEMPERATURE: f32 = 0.3;

/// Generate text using the LLM with prompt sanitization.
///
/// This function:
/// 1. Sanitizes the prompt through GovernanceKernel
/// 2. If safe, sends to arbiter for generation
/// 3. If requires approval, returns with approval_required flag
/// 4. If blocked, returns error
pub async fn llm_generate(request: LlmGenerateRequest) -> RuntimeResult<LlmGenerateResponse> {
    // Get context size for sanitization check
    let context_size = request.context.as_ref().map(|c| c.len()).unwrap_or(0);

    // Sanitize the prompt through governance kernel
    let sanitization_result = sanitize_prompt_inline(&request.prompt, context_size)?;

    match sanitization_result {
        SanitizationResult::Safe => {
            // Prompt is safe - call the arbiter
            let response_text = call_arbiter_generate(
                &request.prompt,
                request.context.as_deref(),
                request.max_tokens.unwrap_or(DEFAULT_MAX_TOKENS),
                request.temperature.unwrap_or(DEFAULT_TEMPERATURE),
            )
            .await?;

            Ok(LlmGenerateResponse {
                text: response_text,
                tokens_used: None, // TODO: Track from arbiter response
                approval_required: false,
                approval_reason: None,
            })
        }
        SanitizationResult::RequiresApproval(reason) => {
            // Return with approval flag - execution will be paused
            Ok(LlmGenerateResponse {
                text: String::new(),
                tokens_used: None,
                approval_required: true,
                approval_reason: Some(reason),
            })
        }
    }
}

enum SanitizationResult {
    Safe,
    RequiresApproval(String),
}

/// Inline sanitization matching GovernanceKernel::sanitize_llm_prompt
fn sanitize_prompt_inline(prompt: &str, context_size: usize) -> RuntimeResult<SanitizationResult> {
    let prompt_lower = prompt.to_lowercase();

    // Injection patterns - BLOCK these completely
    const INJECTION_PATTERNS: &[&str] = &[
        "ignore all previous instructions",
        "ignore previous instructions",
        "forget your instructions",
        "disregard your instructions",
        "you are now",
        "pretend you are",
        "act as if you are",
        "roleplay as",
        "your new role is",
        "from now on you will",
        "system prompt",
        "jailbreak",
        "dan mode",
        "developer mode",
        "bypass",
        "override your",
        "ignore safety",
    ];

    for pattern in INJECTION_PATTERNS {
        if prompt_lower.contains(pattern) {
            return Err(RuntimeError::Generic(format!(
                "LLM prompt injection blocked: pattern '{}' detected",
                pattern
            )));
        }
    }

    // Dangerous patterns - require approval
    const DANGEROUS_PATTERNS: &[&str] = &[
        "password",
        "api key",
        "secret key",
        "private key",
        "access token",
        "credentials",
        "execute code",
        "run command",
        "shell command",
        "rm -rf",
        "drop table",
    ];

    for pattern in DANGEROUS_PATTERNS {
        if prompt_lower.contains(pattern) {
            return Ok(SanitizationResult::RequiresApproval(format!(
                "Prompt mentions sensitive topic: '{}'",
                pattern
            )));
        }
    }

    // Size check
    if prompt.len() > 2000 && context_size == 0 {
        return Ok(SanitizationResult::RequiresApproval(
            "Long prompt without context data".to_string(),
        ));
    }

    Ok(SanitizationResult::Safe)
}

/// Call the arbiter to generate text
async fn call_arbiter_generate(
    prompt: &str,
    context: Option<&str>,
    _max_tokens: u32,
    _temperature: f32,
) -> RuntimeResult<String> {
    // Build the full prompt with context
    let full_prompt = if let Some(ctx) = context {
        format!("{}\n\nContext:\n{}", prompt, ctx)
    } else {
        prompt.to_string()
    };

    // Get the global arbiter
    let arbiter = get_global_arbiter().ok_or_else(|| {
        RuntimeError::Generic(
            "LLM arbiter not initialized. Call ops::llm::set_global_arbiter() during CCOS init."
                .to_string(),
        )
    })?;

    // Call the arbiter's generate_raw_text method
    log::info!(
        "[ccos.llm.generate] Calling arbiter with prompt ({} chars)",
        full_prompt.len()
    );

    let response = arbiter
        .generate_raw_text(&full_prompt)
        .await
        .map_err(|e| RuntimeError::Generic(format!("LLM generation failed: {}", e)))?;

    log::info!(
        "[ccos.llm.generate] Generated {} chars response",
        response.len()
    );

    Ok(response)
}
