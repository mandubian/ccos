//! Hybrid decomposition strategy
//!
//! Combines multiple strategies: tries patterns first (fast), then falls back
//! to LLM-based decomposition if no pattern matches.

use async_trait::async_trait;
use std::sync::Arc;

use super::grounded_llm::{EmbeddingProvider, GroundedLlmDecomposition};
use super::intent_first::{IntentFirstDecomposition, LlmProvider};
use super::pattern::PatternDecomposition;
use super::{DecompositionContext, DecompositionError, DecompositionResult, DecompositionStrategy};
use crate::planner::modular_planner::types::ToolSummary;

/// Hybrid decomposition strategy configuration
#[derive(Debug, Clone)]
pub struct HybridConfig {
    /// Minimum confidence threshold to accept pattern match
    pub pattern_confidence_threshold: f64,
    /// Whether to try grounded LLM (with tools) before intent-first
    pub prefer_grounded: bool,
    /// Maximum tools to consider for grounded decomposition
    pub max_grounded_tools: usize,
    /// Force LLM path (skip patterns)
    pub force_llm: bool,
}

impl Default for HybridConfig {
    fn default() -> Self {
        Self {
            // NOTE: Pattern decomposition is fast but doesn't select specific tools.
            // LLM-based decomposition (grounded_llm) provides much better tool selection.
            // Threshold 2.0 effectively disables patterns, preferring LLM.
            // Pattern code kept for future use cases where speed > accuracy is desired.
            pattern_confidence_threshold: 2.0,
            prefer_grounded: true,
            max_grounded_tools: 0, // 0 = unlimited (like real MCP behavior)
            force_llm: true,
        }
    }
}

/// Hybrid decomposition strategy.
///
/// Tries strategies in order of preference:
/// 1. Pattern matching (fastest, most predictable)
/// 2. Grounded LLM (if tools available)
/// 3. Intent-first LLM (fallback)
pub struct HybridDecomposition {
    pattern: PatternDecomposition,
    grounded: Option<GroundedLlmDecomposition>,
    intent_first: Option<IntentFirstDecomposition>,
    config: HybridConfig,
}

impl HybridDecomposition {
    pub fn new() -> Self {
        Self {
            pattern: PatternDecomposition::new(),
            grounded: None,
            intent_first: None,
            config: HybridConfig::default(),
        }
    }

    pub fn with_llm(mut self, llm_provider: Arc<dyn LlmProvider>) -> Self {
        self.intent_first = Some(IntentFirstDecomposition::new(llm_provider.clone()));
        self.grounded = Some(
            GroundedLlmDecomposition::new(llm_provider)
                .with_max_tools(self.config.max_grounded_tools),
        );
        self
    }

    pub fn with_embedding(mut self, embedding_provider: Arc<dyn EmbeddingProvider>) -> Self {
        if let Some(grounded) = self.grounded.take() {
            self.grounded = Some(grounded.with_embedding_provider(embedding_provider));
        }
        self
    }

    pub fn with_config(mut self, config: HybridConfig) -> Self {
        self.config = config;
        // Update grounded if it exists
        if let Some(grounded) = self.grounded.take() {
            self.grounded = Some(grounded.with_max_tools(self.config.max_grounded_tools));
        }
        self
    }

    /// Pattern-only mode (no LLM)
    pub fn pattern_only() -> Self {
        Self {
            pattern: PatternDecomposition::new(),
            grounded: None,
            intent_first: None,
            config: HybridConfig::default(),
        }
    }
}

impl Default for HybridDecomposition {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait(?Send)]
impl DecompositionStrategy for HybridDecomposition {
    fn name(&self) -> &str {
        "hybrid"
    }

    fn can_handle(&self, goal: &str) -> f64 {
        // Hybrid can handle anything if it has LLM fallback
        let pattern_score = self.pattern.can_handle(goal);

        if pattern_score >= self.config.pattern_confidence_threshold {
            pattern_score
        } else if self.intent_first.is_some() || self.grounded.is_some() {
            0.5 // LLM fallback available
        } else {
            pattern_score // Only patterns available
        }
    }

    async fn decompose(
        &self,
        goal: &str,
        available_tools: Option<&[ToolSummary]>,
        context: &DecompositionContext,
    ) -> Result<DecompositionResult, DecompositionError> {
        // 1. Try pattern matching first
        let pattern_confidence = self.pattern.can_handle(goal);

        if !self.config.force_llm && pattern_confidence >= self.config.pattern_confidence_threshold
        {
            match self.pattern.decompose(goal, available_tools, context).await {
                Ok(result) => {
                    if context.show_prompt || context.verbose_llm {
                        println!(
                            "ℹ️  Using pattern decomposition (no LLM prompt). Confidence {:.2}",
                            result.confidence
                        );
                    }
                    log::debug!(
                        "[hybrid] Pattern matched with confidence {:.2}: {}",
                        result.confidence,
                        result.strategy_name
                    );
                    return Ok(result);
                }
                Err(e) => {
                    log::debug!("[hybrid] Pattern failed, trying LLM: {}", e);
                }
            }
        } else {
            log::debug!(
                "[hybrid] Pattern confidence {:.2} below threshold {:.2}, skipping",
                pattern_confidence,
                self.config.pattern_confidence_threshold
            );
        }

        // 2. Try grounded LLM if tools available and configured
        // NOTE: We do NOT fallback to IntentFirst anymore - GroundedLLM is the single LLM strategy
        // This is simpler to debug and GroundedLLM produces better results with tool context
        if self.config.prefer_grounded && available_tools.is_some() {
            if let Some(ref grounded) = self.grounded {
                match grounded.decompose(goal, available_tools, context).await {
                    Ok(result) => {
                        log::debug!(
                            "[hybrid] Grounded LLM succeeded with confidence {:.2}",
                            result.confidence
                        );
                        return Ok(result);
                    }
                    Err(e) => {
                        log::debug!("[hybrid] Grounded LLM failed: {}", e);
                        // No IntentFirst fallback - return the error
                        return Err(e);
                    }
                }
            }
        }

        // 3. If no tools available, try grounded LLM anyway (it can do abstract decomposition)
        if let Some(ref grounded) = self.grounded {
            return grounded.decompose(goal, available_tools, context).await;
        }

        // 4. If we got here with a partial pattern match, try pattern anyway
        if pattern_confidence > 0.0 {
            return self.pattern.decompose(goal, available_tools, context).await;
        }

        Err(DecompositionError::NoStrategyMatched)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_hybrid_pattern_first() {
        let strategy = HybridDecomposition::pattern_only();
        let context = DecompositionContext::new();

        // Should match pattern
        let result = strategy
            .decompose("list issues but ask me for page size", None, &context)
            .await
            .expect("Should decompose");

        assert!(result.strategy_name.starts_with("pattern:"));
        assert_eq!(result.sub_intents.len(), 2);
    }

    #[tokio::test]
    async fn test_hybrid_no_llm_fallback() {
        let strategy = HybridDecomposition::pattern_only();
        let context = DecompositionContext::new();

        // Should fail for complex goal without LLM
        let result = strategy
            .decompose(
                "do something very complex that patterns don't recognize",
                None,
                &context,
            )
            .await;

        assert!(result.is_err());
    }

    #[test]
    fn test_can_handle() {
        let pattern_only = HybridDecomposition::pattern_only();

        // High confidence for pattern-matching goals
        assert!(pattern_only.can_handle("list issues but ask me for page size") > 0.5);

        // Low confidence for non-pattern goals without LLM
        assert!(pattern_only.can_handle("do something weird") < 0.5);
    }
}
