//! Decomposition strategies for breaking goals into sub-intents
//!
//! Each strategy implements the `DecompositionStrategy` trait and can be
//! plugged into the `ModularPlanner`.

mod pattern;
mod intent_first;
pub mod grounded_llm;
pub mod hybrid;

pub use pattern::PatternDecomposition;
pub use intent_first::IntentFirstDecomposition;
pub use grounded_llm::{GroundedLlmDecomposition, EmbeddingProvider, cosine_similarity};
pub use hybrid::HybridDecomposition;

use std::collections::HashMap;
use async_trait::async_trait;
use thiserror::Error;

use super::types::{SubIntent, ToolSummary};

/// Error type for decomposition failures
#[derive(Debug, Error)]
pub enum DecompositionError {
    #[error("Failed to parse goal: {0}")]
    ParseError(String),
    
    #[error("LLM generation failed: {0}")]
    LlmError(String),
    
    #[error("Pattern matching failed: {0}")]
    PatternError(String),
    
    #[error("Goal is too complex for this strategy: {0}")]
    TooComplex(String),
    
    #[error("No decomposition strategy could handle the goal")]
    NoStrategyMatched,
    
    #[error("Internal error: {0}")]
    Internal(String),
}

/// Context provided to decomposition strategies
#[derive(Debug, Clone, Default)]
pub struct DecompositionContext {
    /// Previously seen goals (for learning/caching)
    pub history: Vec<String>,
    
    /// User preferences that may affect decomposition
    pub preferences: HashMap<String, String>,
    
    /// Maximum depth for recursive decomposition
    pub max_depth: usize,
    
    /// Current recursion depth
    pub current_depth: usize,
    
    /// Pre-extracted parameters from the goal
    pub pre_extracted_params: HashMap<String, String>,
    
    /// Whether to print verbose LLM debug info (prompts/responses)
    pub verbose_llm: bool,
}

impl DecompositionContext {
    pub fn new() -> Self {
        Self {
            history: vec![],
            preferences: HashMap::new(),
            max_depth: 3,
            current_depth: 0,
            pre_extracted_params: HashMap::new(),
            verbose_llm: false,
        }
    }
    
    pub fn with_verbose_llm(mut self, verbose: bool) -> Self {
        self.verbose_llm = verbose;
        self
    }
    
    pub fn with_max_depth(mut self, depth: usize) -> Self {
        self.max_depth = depth;
        self
    }
    
    pub fn with_param(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.pre_extracted_params.insert(key.into(), value.into());
        self
    }
    
    pub fn deeper(&self) -> Self {
        Self {
            history: self.history.clone(),
            preferences: self.preferences.clone(),
            max_depth: self.max_depth,
            current_depth: self.current_depth + 1,
            pre_extracted_params: self.pre_extracted_params.clone(),
            verbose_llm: self.verbose_llm,
        }
    }
    
    pub fn is_at_max_depth(&self) -> bool {
        self.current_depth >= self.max_depth
    }
}

/// Result of a decomposition operation
#[derive(Debug, Clone)]
pub struct DecompositionResult {
    /// The sub-intents produced
    pub sub_intents: Vec<SubIntent>,
    
    /// Which strategy produced this result
    pub strategy_name: String,
    
    /// Confidence in the decomposition (0.0 - 1.0)
    pub confidence: f64,
    
    /// Whether this decomposition is "atomic" (no further decomposition needed)
    pub is_atomic: bool,
    
    /// Optional reasoning/explanation
    pub reasoning: Option<String>,
}

impl DecompositionResult {
    pub fn atomic(sub_intents: Vec<SubIntent>, strategy: impl Into<String>) -> Self {
        Self {
            sub_intents,
            strategy_name: strategy.into(),
            confidence: 1.0,
            is_atomic: true,
            reasoning: None,
        }
    }
    
    pub fn with_confidence(mut self, confidence: f64) -> Self {
        self.confidence = confidence;
        self
    }
    
    pub fn with_reasoning(mut self, reasoning: impl Into<String>) -> Self {
        self.reasoning = Some(reasoning.into());
        self
    }
}

/// Trait for goal decomposition strategies.
/// 
/// Implementations break a natural language goal into a sequence of sub-intents
/// that can be stored in the IntentGraph and resolved to capabilities.
#[async_trait(?Send)]
pub trait DecompositionStrategy {
    /// Name of this strategy for logging/debugging
    fn name(&self) -> &str;
    
    /// Check if this strategy can handle the given goal.
    /// Returns a confidence score (0.0 = cannot handle, 1.0 = perfect fit).
    fn can_handle(&self, goal: &str) -> f64;
    
    /// Decompose a goal into sub-intents.
    /// 
    /// # Arguments
    /// * `goal` - The natural language goal to decompose
    /// * `available_tools` - Optional pre-filtered tools (for grounded strategies)
    /// * `context` - Additional context for decomposition
    async fn decompose(
        &self,
        goal: &str,
        available_tools: Option<&[ToolSummary]>,
        context: &DecompositionContext,
    ) -> Result<DecompositionResult, DecompositionError>;
}

pub mod llm_adapter;
