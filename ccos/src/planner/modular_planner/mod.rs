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
pub mod resolution;
pub mod orchestrator;
pub mod types;
pub mod verification;

// Re-exports
pub use decomposition::{
    DecompositionStrategy, 
    PatternDecomposition, 
    IntentFirstDecomposition,
    GroundedLlmDecomposition,
    HybridDecomposition,
    DecompositionContext,
    DecompositionError,
    EmbeddingProvider,
};

pub use resolution::{
    ResolutionStrategy,
    SemanticResolution,
    McpResolution,
    CatalogResolution,
    ResolutionContext,
    ResolutionError,
    ResolvedCapability,
};

pub use orchestrator::{
    ModularPlanner,
    PlannerConfig,
    PlannerError,
    PlanResult,
    PlanningTrace,
};

pub use types::{
    SubIntent,
    IntentType,
    DomainHint,
    ToolSummary,
    ApiAction,
};

pub use verification::{
    PlanVerifier,
    RuleBasedVerifier,
    LlmVerifier,
    CompositeVerifier,
    VerificationResult,
    VerificationVerdict,
    VerificationIssue,
    IssueSeverity,
    IssueCategory,
    VerificationError,
};
