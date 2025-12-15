//! Adapter for CCOS LLM Provider to Modular Planner LLM Provider
//!
//! This module bridges the gap between the core CCOS LLM infrastructure
//! and the Modular Planner's trait requirements.

use async_trait::async_trait;
use std::sync::{Arc, Mutex};
use std::time::Instant;

// Use the trait from the modular planner
use crate::planner::modular_planner::decomposition::intent_first::LlmProvider as PlannerLlmProvider;
// Use the trait from the arbiter
use crate::arbiter::llm_provider::LlmProvider as CcosLlmProvider;

use crate::planner::modular_planner::decomposition::DecompositionError;

/// Captured LLM interaction for tracing
#[derive(Debug, Clone)]
pub struct LlmInteractionCapture {
    pub model: String,
    pub prompt: String,
    pub response: Option<String>,
    pub duration_ms: u64,
}

/// Adapter that wraps a CCOS LLM provider to implement the Planner LLM provider trait
/// with optional tracing support for TUI
pub struct CcosLlmAdapter {
    provider: Box<dyn CcosLlmProvider>,
    /// Optional callback to emit LLM interactions for tracing
    trace_callback: Option<Arc<dyn Fn(LlmInteractionCapture) + Send + Sync>>,
    /// Last captured prompt (for TUI to retrieve)
    pub last_prompt: Arc<Mutex<Option<String>>>,
    /// Last captured response (for TUI to retrieve)
    pub last_response: Arc<Mutex<Option<String>>>,
}

impl CcosLlmAdapter {
    pub fn new(provider: Box<dyn CcosLlmProvider>) -> Self {
        Self {
            provider,
            trace_callback: None,
            last_prompt: Arc::new(Mutex::new(None)),
            last_response: Arc::new(Mutex::new(None)),
        }
    }

    /// Create adapter with a tracing callback for TUI integration
    pub fn new_with_tracing(
        provider: Box<dyn CcosLlmProvider>,
        callback: Arc<dyn Fn(LlmInteractionCapture) + Send + Sync>,
    ) -> Self {
        Self {
            provider,
            trace_callback: Some(callback),
            last_prompt: Arc::new(Mutex::new(None)),
            last_response: Arc::new(Mutex::new(None)),
        }
    }

    /// Get the last prompt that was sent
    pub fn get_last_prompt(&self) -> Option<String> {
        self.last_prompt.lock().ok()?.clone()
    }

    /// Get the last response received
    pub fn get_last_response(&self) -> Option<String> {
        self.last_response.lock().ok()?.clone()
    }
}

#[async_trait(?Send)]
impl PlannerLlmProvider for CcosLlmAdapter {
    async fn generate_text(&self, prompt: &str) -> Result<String, DecompositionError> {
        let start = Instant::now();

        // Store prompt before calling
        if let Ok(mut guard) = self.last_prompt.lock() {
            *guard = Some(prompt.to_string());
        }

        let result = self
            .provider
            .generate_text(prompt)
            .await
            .map_err(|e| DecompositionError::LlmError(e.to_string()))?;

        let duration_ms = start.elapsed().as_millis() as u64;

        // Store response
        if let Ok(mut guard) = self.last_response.lock() {
            *guard = Some(result.clone());
        }

        // Emit trace callback if available
        if let Some(ref callback) = self.trace_callback {
            callback(LlmInteractionCapture {
                model: "openrouter".to_string(), // TODO: get actual model name from provider
                prompt: prompt.to_string(),
                response: Some(result.clone()),
                duration_ms,
            });
        }

        Ok(result)
    }
}
