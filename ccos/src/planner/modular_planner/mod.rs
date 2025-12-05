//! Modular Planner
//!
//! A pluggable planning architecture that properly leverages the CCOS IntentGraph.
//!
//! ## Architecture
//!
//! The modular planner separates concerns into three phases:
//!
//! 1. **Decomposition**: Breaking a goal into sub-intents (stored in IntentGraph)
//! 2. **Resolution**: Mapping intents to concrete capabilities
//! 3. **Plan Generation**: Building executable RTFS from resolved intents
//!
//! Each phase is implemented via pluggable strategies, allowing easy experimentation
//! and customization without changing the core architecture.
//!
//! ## Strategy Types
//!
//! ### Decomposition Strategies
//! - `PatternDecomposition`: Fast regex-based patterns for common cases
//! - `IntentFirstDecomposition`: LLM-based, produces abstract intents (no tool hints)
//! - `GroundedLlmDecomposition`: LLM with pre-filtered tools via embeddings
//! - `HybridDecomposition`: Pattern-first with LLM fallback
//!
//! ### Resolution Strategies  
//! - `SemanticResolution`: Embedding-based capability matching
//! - `McpResolution`: Direct MCP tool discovery and matching
//! - `CatalogResolution`: Search registered capabilities in catalog
//!
//! ## IntentGraph Integration
//!
//! All decomposed sub-intents are stored as real `StorableIntent` nodes in the
//! IntentGraph with proper edges (IsSubgoalOf, DependsOn, etc.). This enables:
//! - Audit trail of planning decisions
//! - Reuse of previously computed decompositions
//! - Visualization of intent hierarchies
//! - Learning from execution outcomes

pub mod decomposition;
pub mod orchestrator;
pub mod safe_executor;
pub mod resolution;
pub mod types;

// Re-exports
pub use decomposition::{
    DecompositionContext, DecompositionError, DecompositionStrategy, EmbeddingProvider,
    GroundedLlmDecomposition, HybridConfig, HybridDecomposition, IntentFirstDecomposition,
    PatternDecomposition,
};

pub use resolution::{
    CatalogResolution, McpResolution, ResolutionContext, ResolutionError, ResolutionStrategy,
    ResolvedCapability, SemanticResolution,
};

pub use orchestrator::{ModularPlanner, PlanResult, PlannerConfig, PlannerError, PlanningTrace};

pub use types::{ApiAction, DomainHint, IntentType, SubIntent, ToolSummary};
