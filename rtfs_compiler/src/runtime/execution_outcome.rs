//! Execution Outcome Types for RTFS-CCOS Control Flow Inversion
//!
//! This module defines the types used for RTFS to yield control back to CCOS
//! when it encounters non-pure operations that require delegation decisions.

use crate::runtime::values::Value;

/// The outcome of executing an RTFS node.
/// RTFS execution can either complete locally or require host intervention.
#[derive(Debug, Clone)]
pub enum ExecutionOutcome {
    /// Execution completed successfully with a value.
    Complete(Value),
    /// Execution requires host intervention for a non-pure operation.
    RequiresHost(HostCall),
}

/// A call to the host (CCOS) for handling non-pure operations.
/// This represents the information CCOS needs to make delegation decisions.
#[derive(Debug, Clone)]
pub struct HostCall {
    /// The fully-qualified RTFS symbol name being invoked.
    pub fn_symbol: String,
    /// The arguments to pass to the function.
    pub args: Vec<Value>,
    /// Optional metadata about the call context.
    pub metadata: Option<CallMetadata>,
}

/// Additional metadata about a host call for delegation decisions.
#[derive(Debug, Clone, Default)]
pub struct CallMetadata {
    /// Cheap structural hash of argument type information.
    pub arg_type_fingerprint: u64,
    /// Hash representing ambient runtime context (permissions, task, etc.).
    pub runtime_context_hash: u64,
    /// Optional semantic embedding of the original task description.
    pub semantic_hash: Option<Vec<f32>>,
    /// Additional context from external components.
    pub context: std::collections::HashMap<String, String>,
}

impl CallMetadata {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_arg_type_fingerprint(mut self, fingerprint: u64) -> Self {
        self.arg_type_fingerprint = fingerprint;
        self
    }

    pub fn with_runtime_context_hash(mut self, hash: u64) -> Self {
        self.runtime_context_hash = hash;
        self
    }

    pub fn with_semantic_hash(mut self, hash: Vec<f32>) -> Self {
        self.semantic_hash = Some(hash);
        self
    }

    pub fn with_context(mut self, key: String, value: String) -> Self {
        self.context.insert(key, value);
        self
    }
}