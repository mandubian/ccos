//! Decomposition strategies for breaking goals into sub-intents
//!
//! Each strategy implements the `DecompositionStrategy` trait and can be
//! plugged into the `ModularPlanner`.

pub mod grounded_llm;
pub mod hybrid;
mod intent_first;
mod pattern;

pub use grounded_llm::{cosine_similarity, EmbeddingProvider, GroundedLlmDecomposition};
pub use hybrid::{HybridConfig, HybridDecomposition};
pub use intent_first::IntentFirstDecomposition;
pub use pattern::PatternDecomposition;

use async_trait::async_trait;
use std::collections::HashMap;
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

    /// Whether to print just the prompt sent to LLM
    pub show_prompt: bool,

    /// Whether to confirm before each LLM call
    pub confirm_llm: bool,

    /// Parent intent description (for sub-intent refinement)
    /// When refining a sub-intent, this contains the original parent goal
    pub parent_intent: Option<String>,

    /// Descriptions of sibling intents (other steps at same level in parent plan)
    /// Helps LLM understand what's already being done and avoid regenerating
    pub sibling_intents: Vec<String>,

    /// Indices of sibling steps that provide input data to this intent
    /// e.g., if this intent depends on step 0 for data, data_source_indices = [0]
    pub data_source_indices: Vec<usize>,
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
            show_prompt: false,
            confirm_llm: false,
            parent_intent: None,
            sibling_intents: vec![],
            data_source_indices: vec![],
        }
    }

    pub fn with_verbose_llm(mut self, verbose: bool) -> Self {
        self.verbose_llm = verbose;
        self
    }

    pub fn with_show_prompt(mut self, show: bool) -> Self {
        self.show_prompt = show;
        self
    }

    pub fn with_confirm_llm(mut self, confirm: bool) -> Self {
        self.confirm_llm = confirm;
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
            show_prompt: self.show_prompt,
            confirm_llm: self.confirm_llm,
            parent_intent: self.parent_intent.clone(),
            sibling_intents: self.sibling_intents.clone(),
            data_source_indices: self.data_source_indices.clone(),
        }
    }

    /// Set the parent intent description for sub-intent refinement
    pub fn with_parent_intent(mut self, parent: impl Into<String>) -> Self {
        self.parent_intent = Some(parent.into());
        self
    }

    /// Set sibling intent descriptions (other steps in parent plan)
    pub fn with_siblings(mut self, siblings: Vec<String>) -> Self {
        self.sibling_intents = siblings;
        self
    }

    /// Set indices of sibling steps that provide data to this intent
    pub fn with_data_sources(mut self, sources: Vec<usize>) -> Self {
        self.data_source_indices = sources;
        self
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
