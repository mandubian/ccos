//! RTFS Delegation System
//!
//! This module defines the core delegation concepts for RTFS execution.
//! Delegation determines where RTFS function calls should be executed:
//! locally (pure evaluator), via models, or remotely.
//!
//! This is a core RTFS concept that CCOS can extend and implement.

use std::collections::HashMap;
use std::sync::Arc;

/// Where the RTFS evaluator should send the execution.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ExecTarget {
    /// Run directly in the deterministic RTFS evaluator.
    LocalPure,
    /// Call an on-device model that implements the model interface.
    LocalModel(String),
    /// Delegate to a remote provider via external orchestration.
    RemoteModel(String),
    /// Execute a pre-compiled RTFS module from cache.
    CacheHit {
        /// Pointer to the cached bytecode.
        storage_pointer: String,
        /// Cryptographic signature of the bytecode for verification.
        signature: String,
    },
}

/// Context for delegation decisions.
#[derive(Debug, Clone)]
pub struct CallContext<'a> {
    /// Fully-qualified RTFS symbol name being invoked.
    pub fn_symbol: &'a str,
    /// Cheap structural hash of argument type information.
    pub arg_type_fingerprint: u64,
    /// Hash representing ambient runtime context (permissions, task, etc.).
    pub runtime_context_hash: u64,
    /// Optional semantic embedding of the original task description.
    pub semantic_hash: Option<Vec<f32>>,
    /// Optional delegation metadata from external components
    pub metadata: Option<DelegationMetadata>,
}

/// Delegation metadata provided by external components.
#[derive(Debug, Clone, Default)]
pub struct DelegationMetadata {
    /// Confidence score from the component that provided this metadata (0.0 - 1.0)
    pub confidence: Option<f64>,
    /// Human-readable reasoning from the component
    pub reasoning: Option<String>,
    /// Additional context from external components
    pub context: HashMap<String, String>,
    /// Component that provided this metadata
    pub source: Option<String>,
}

impl DelegationMetadata {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_confidence(mut self, confidence: f64) -> Self {
        self.confidence = Some(confidence);
        self
    }

    pub fn with_reasoning(mut self, reasoning: String) -> Self {
        self.reasoning = Some(reasoning);
        self
    }

    pub fn with_context(mut self, key: String, value: String) -> Self {
        self.context.insert(key, value);
        self
    }

    pub fn with_source(mut self, source: String) -> Self {
        self.source = Some(source);
        self
    }
}

/// Core delegation engine trait for RTFS.
/// 
/// This trait defines how RTFS decides where to execute function calls.
/// CCOS can provide implementations that integrate with external systems.
pub trait DelegationEngine: Send + Sync + std::fmt::Debug {
    /// Decide where to execute a function call based on the context.
    fn decide(&self, ctx: &CallContext) -> ExecTarget;
}

/// Simple static mapping implementation for basic delegation.
/// 
/// This is a minimal implementation that uses a static mapping
/// of function names to execution targets.
#[derive(Debug)]
pub struct StaticDelegationEngine {
    /// Fast lookup for explicit per-symbol policies.
    static_map: HashMap<String, ExecTarget>,
}

impl StaticDelegationEngine {
    /// Create a new static delegation engine with the given mapping.
    pub fn new(static_map: HashMap<String, ExecTarget>) -> Self {
        Self { static_map }
    }

    /// Create a new static delegation engine with no mappings (all local pure).
    pub fn new_empty() -> Self {
        Self {
            static_map: HashMap::new(),
        }
    }
}

impl DelegationEngine for StaticDelegationEngine {
    fn decide(&self, ctx: &CallContext) -> ExecTarget {
        // Check if we have an explicit mapping for this function
        if let Some(target) = self.static_map.get(ctx.fn_symbol) {
            target.clone()
        } else {
            // Default to local pure execution
            ExecTarget::LocalPure
        }
    }
}

/// Model registry for managing available models.
/// 
/// This is a placeholder for model management functionality.
/// CCOS can provide more sophisticated implementations.
#[derive(Debug, Default)]
pub struct ModelRegistry {
    // Placeholder for model management
    // CCOS implementations can extend this with actual model registry logic
}

impl ModelRegistry {
    /// Create a new model registry with default settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a new model registry with default model configurations.
    pub fn with_defaults() -> Self {
        Self::default()
    }

    /// Get a model provider by ID (placeholder implementation).
    /// 
    /// This is a placeholder method. CCOS implementations can provide
    /// more sophisticated model registry functionality.
    pub fn get(&self, _model_id: &str) -> Option<()> {
        // Placeholder - always returns None for now
        // CCOS implementations can provide actual model providers
        None
    }
}
