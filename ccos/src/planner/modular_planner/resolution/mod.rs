//! Resolution strategies for mapping intents to capabilities
//!
//! After decomposition produces SubIntents, resolution maps each intent
//! to a concrete capability that can execute it.

pub mod catalog;
pub mod mcp;
pub mod semantic;

pub use catalog::{CatalogConfig, CatalogResolution, ScoringMethod};
pub use mcp::McpResolution;
pub use semantic::{CapabilityCatalog, CapabilityInfo, SemanticResolution};

use async_trait::async_trait;
use std::collections::HashMap;
use thiserror::Error;

use super::types::{DomainHint, SubIntent, ToolSummary};

/// Error type for resolution failures
#[derive(Debug, Error)]
pub enum ResolutionError {
    #[error("No capability found for intent: {0}")]
    NotFound(String),

    #[error("Multiple ambiguous matches for intent: {0}")]
    Ambiguous(String),

    #[error("MCP discovery failed: {0}")]
    McpError(String),

    #[error("Catalog search failed: {0}")]
    CatalogError(String),

    #[error("Embedding service error: {0}")]
    EmbeddingError(String),

    #[error("Capability requires external referral: {0}")]
    NeedsReferral(String),

    #[error("Cache I/O error: {0}")]
    CacheError(String),

    #[error("Grounded planner explicitly returned no tool: {0}")]
    GroundedNoTool(String),

    #[error("Internal error: {0}")]
    Internal(String),
}

/// Context provided to resolution strategies
#[derive(Debug, Clone, Default)]
pub struct ResolutionContext {
    /// Domain hints to narrow search
    pub domain_hints: Vec<DomainHint>,

    /// Previously resolved capabilities in this plan
    pub resolved_capabilities: Vec<String>,

    /// User preferences for capability selection
    pub preferences: HashMap<String, String>,

    /// Whether to allow synthesized capabilities
    pub allow_synthesis: bool,

    /// Maximum score difference to consider ambiguous
    pub ambiguity_threshold: f64,
}

impl ResolutionContext {
    pub fn new() -> Self {
        Self {
            domain_hints: vec![],
            resolved_capabilities: vec![],
            preferences: HashMap::new(),
            allow_synthesis: true,
            ambiguity_threshold: 0.1,
        }
    }

    pub fn with_domain(mut self, domain: DomainHint) -> Self {
        self.domain_hints.push(domain);
        self
    }

    pub fn with_synthesis(mut self, allow: bool) -> Self {
        self.allow_synthesis = allow;
        self
    }
}

/// Result of resolving a SubIntent to a capability
#[derive(Debug, Clone)]
pub enum ResolvedCapability {
    /// Local capability (already registered)
    Local {
        capability_id: String,
        arguments: HashMap<String, String>,
        input_schema: Option<serde_json::Value>,
        confidence: f64,
    },

    /// Remote MCP capability (discovered and registered)
    Remote {
        capability_id: String,
        server_url: String,
        arguments: HashMap<String, String>,
        input_schema: Option<serde_json::Value>,
        confidence: f64,
    },

    /// Synthesized capability (generated RTFS code)
    Synthesized {
        capability_id: String,
        rtfs_code: String,
        arguments: HashMap<String, String>,
    },

    /// Built-in capability (like user.ask)
    BuiltIn {
        capability_id: String,
        arguments: HashMap<String, String>,
    },

    /// Cannot be resolved - needs external help
    NeedsReferral {
        reason: String,
        suggested_action: String,
    },
}

impl ResolvedCapability {
    pub fn capability_id(&self) -> Option<&str> {
        match self {
            ResolvedCapability::Local { capability_id, .. } => Some(capability_id),
            ResolvedCapability::Remote { capability_id, .. } => Some(capability_id),
            ResolvedCapability::Synthesized { capability_id, .. } => Some(capability_id),
            ResolvedCapability::BuiltIn { capability_id, .. } => Some(capability_id),
            ResolvedCapability::NeedsReferral { .. } => None,
        }
    }

    pub fn arguments(&self) -> Option<&HashMap<String, String>> {
        match self {
            ResolvedCapability::Local { arguments, .. } => Some(arguments),
            ResolvedCapability::Remote { arguments, .. } => Some(arguments),
            ResolvedCapability::Synthesized { arguments, .. } => Some(arguments),
            ResolvedCapability::BuiltIn { arguments, .. } => Some(arguments),
            ResolvedCapability::NeedsReferral { .. } => None,
        }
    }

    pub fn confidence(&self) -> f64 {
        match self {
            ResolvedCapability::Local { confidence, .. } => *confidence,
            ResolvedCapability::Remote { confidence, .. } => *confidence,
            ResolvedCapability::Synthesized { .. } => 0.5, // Synthesized has moderate confidence
            ResolvedCapability::BuiltIn { .. } => 1.0,     // Built-in is always high confidence
            ResolvedCapability::NeedsReferral { .. } => 0.0,
        }
    }

    /// Check if this resolution represents a pending/unresolved capability
    pub fn is_pending(&self) -> bool {
        matches!(self, ResolvedCapability::NeedsReferral { .. })
    }
}

/// Trait for intent resolution strategies.
///
/// Implementations map SubIntents to concrete capabilities that can
/// execute them.
#[async_trait(?Send)]
pub trait ResolutionStrategy: Send + Sync {
    /// Name of this strategy for logging/debugging
    fn name(&self) -> &str;

    /// Check if this strategy can handle the given intent type
    fn can_handle(&self, intent: &SubIntent) -> bool;

    /// Resolve a SubIntent to a capability
    async fn resolve(
        &self,
        intent: &SubIntent,
        context: &ResolutionContext,
    ) -> Result<ResolvedCapability, ResolutionError>;

    /// List available tools/capabilities from this resolution source.
    ///
    /// This enables eager discovery for grounded LLM decomposition.
    /// Accepts optional domain hints to restrict the search.
    /// Returns empty vec if not supported or no tools available.
    async fn list_available_tools(&self, _domain_hints: Option<&[DomainHint]>) -> Vec<ToolSummary> {
        vec![] // Default: no pre-listing
    }
}

/// Combined resolution that tries multiple strategies
pub struct CompositeResolution {
    strategies: Vec<Box<dyn ResolutionStrategy>>,
}

impl CompositeResolution {
    pub fn new() -> Self {
        Self { strategies: vec![] }
    }

    pub fn with_strategy(mut self, strategy: Box<dyn ResolutionStrategy>) -> Self {
        self.strategies.push(strategy);
        self
    }

    pub fn add_strategy(&mut self, strategy: Box<dyn ResolutionStrategy>) {
        self.strategies.push(strategy);
    }
}

impl Default for CompositeResolution {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait(?Send)]
impl ResolutionStrategy for CompositeResolution {
    fn name(&self) -> &str {
        "composite"
    }

    fn can_handle(&self, intent: &SubIntent) -> bool {
        self.strategies.iter().any(|s| s.can_handle(intent))
    }

    async fn resolve(
        &self,
        intent: &SubIntent,
        context: &ResolutionContext,
    ) -> Result<ResolvedCapability, ResolutionError> {
        let mut last_error = None;

        for strategy in &self.strategies {
            if !strategy.can_handle(intent) {
                continue;
            }

            match strategy.resolve(intent, context).await {
                Ok(resolved) => {
                    log::debug!(
                        "[composite] Strategy '{}' resolved intent to {:?}",
                        strategy.name(),
                        resolved.capability_id()
                    );
                    return Ok(resolved);
                }
                Err(ResolutionError::GroundedNoTool(msg)) => {
                    log::debug!(
                        "[composite] Strategy '{}' returned explicit NO TOOL: {}",
                        strategy.name(),
                        msg
                    );
                    return Err(ResolutionError::GroundedNoTool(msg));
                }
                Err(e) => {
                    log::debug!("[composite] Strategy '{}' failed: {}", strategy.name(), e);
                    last_error = Some(e);
                }
            }
        }

        Err(last_error.unwrap_or_else(|| {
            ResolutionError::NotFound(format!("No strategy could resolve: {}", intent.description))
        }))
    }

    async fn list_available_tools(&self, domain_hints: Option<&[DomainHint]>) -> Vec<ToolSummary> {
        let mut all_tools = Vec::new();
        let mut seen_ids = std::collections::HashSet::new();

        for strategy in &self.strategies {
            let tools = strategy.list_available_tools(domain_hints).await;
            for tool in tools {
                // Deduplicate by tool id to avoid showing the same capability multiple times
                // Prefer the version with the more specific id (contains '/' or more dots)
                if seen_ids.contains(&tool.id) {
                    // Check if this is a "better" version (more specific ID)
                    if let Some(existing_idx) =
                        all_tools.iter().position(|t: &ToolSummary| t.id == tool.id)
                    {
                        let existing = &all_tools[existing_idx];
                        // Prefer the one with more path separators (fuller ID)
                        let existing_seps =
                            existing.id.matches('/').count() + existing.id.matches('.').count();
                        let new_seps = tool.id.matches('/').count() + tool.id.matches('.').count();
                        if new_seps > existing_seps {
                            all_tools[existing_idx] = tool;
                        }
                    }
                } else {
                    seen_ids.insert(tool.id.clone());
                    all_tools.push(tool);
                }
            }
        }
        all_tools
    }
}
