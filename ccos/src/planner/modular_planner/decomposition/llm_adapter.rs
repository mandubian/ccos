//! Adapter for CCOS LLM Provider to Modular Planner LLM Provider
//!
//! This module bridges the gap between the core CCOS LLM infrastructure
//! and the Modular Planner's trait requirements.

use async_trait::async_trait;

// Use the trait from the modular planner
use crate::planner::modular_planner::decomposition::intent_first::LlmProvider as PlannerLlmProvider;
// Use the trait from the arbiter
use crate::arbiter::llm_provider::LlmProvider as CcosLlmProvider;

use crate::planner::modular_planner::decomposition::DecompositionError;

/// Adapter that wraps a CCOS LLM provider to implement the Planner LLM provider trait
pub struct CcosLlmAdapter {
    provider: Box<dyn CcosLlmProvider>,
}

impl CcosLlmAdapter {
    pub fn new(provider: Box<dyn CcosLlmProvider>) -> Self {
        Self { provider }
    }
}

#[async_trait(?Send)]
impl PlannerLlmProvider for CcosLlmAdapter {
    async fn generate_text(&self, prompt: &str) -> Result<String, DecompositionError> {
        self.provider.generate_text(prompt)
            .await
            .map_err(|e| DecompositionError::LlmError(e.to_string()))
    }
}

