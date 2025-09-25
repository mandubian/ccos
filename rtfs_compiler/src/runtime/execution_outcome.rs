//! Execution Outcome Types for RTFS-CCOS Control Flow Inversion
//!
//! This module defines the types used for RTFS to yield control back to CCOS
//! when it encounters non-pure operations that require delegation decisions.

use crate::runtime::security::RuntimeContext;
use crate::runtime::values::Value;

/// The outcome of executing an RTFS node.
/// RTFS execution can either complete locally or require host intervention.
#[derive(Debug, Clone)]
pub enum ExecutionOutcome {
    /// Execution completed successfully with a value.
    Complete(Value),
    /// Execution requires host intervention for a non-pure operation.
    /// Unified structure handles all host calls with mandatory security/audit fields.
    RequiresHost(HostCall),
}

/// A call to the host (CCOS) for handling non-pure operations.
/// This represents the information CCOS needs to make delegation decisions.
#[derive(Debug, Clone)]
pub struct HostCall {
    // MANDATORY - Core execution data
    /// The fully-qualified capability identifier (e.g., "ccos.state.kv.get")
    pub capability_id: String,
    /// The arguments to pass to the capability.
    pub args: Vec<Value>,

    // MANDATORY - CCOS security & audit (required for Causal Chain)
    /// Security context for the call - MANDATORY for CCOS security model
    pub security_context: RuntimeContext,
    /// Causal context tracking the origin of this call - MANDATORY for audit trail
    pub causal_context: Option<CausalContext>,

    // OPTIONAL - Performance, reliability, and compatibility metadata
    pub metadata: Option<CallMetadata>,
}

/// Additional metadata about a host call for delegation decisions.
/// This contains optional performance, reliability, and compatibility features.
#[derive(Debug, Clone, Default)]
pub struct CallMetadata {
    /// OPTIONAL - Performance & reliability
    pub timeout_ms: Option<u64>,
    pub idempotency_key: Option<String>,

    /// OPTIONAL - Legacy compatibility
    pub arg_type_fingerprint: u64,
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

    pub fn with_timeout_ms(mut self, timeout: u64) -> Self {
        self.timeout_ms = Some(timeout);
        self
    }

    pub fn with_idempotency_key(mut self, key: String) -> Self {
        self.idempotency_key = Some(key);
        self
    }
}

/// Causal context for tracking the origin and hierarchy of host calls.
/// This is MANDATORY for CCOS audit trail and Causal Chain integration.
#[derive(Debug, Clone)]
pub struct CausalContext {
    /// The parent action ID in the Causal Chain hierarchy
    pub parent_action_id: Option<String>,
    /// The intent ID this call is serving
    pub intent_id: Option<String>,
    /// The step ID within the intent
    pub step_id: Option<String>,
    /// The plan ID if this is part of a plan execution
    pub plan_id: Option<String>,
}

impl CausalContext {
    pub fn new() -> Self {
        Self {
            parent_action_id: None,
            intent_id: None,
            step_id: None,
            plan_id: None,
        }
    }

    pub fn with_parent_action_id(mut self, action_id: String) -> Self {
        self.parent_action_id = Some(action_id);
        self
    }

    pub fn with_intent_id(mut self, intent_id: String) -> Self {
        self.intent_id = Some(intent_id);
        self
    }

    pub fn with_step_id(mut self, step_id: String) -> Self {
        self.step_id = Some(step_id);
        self
    }

    pub fn with_plan_id(mut self, plan_id: String) -> Self {
        self.plan_id = Some(plan_id);
        self
    }
}

impl Default for CausalContext {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_host_call_creation() {
        let security_context = RuntimeContext::full();
        let causal_context = CausalContext::new()
            .with_plan_id("test-plan".to_string())
            .with_intent_id("test-intent".to_string());

        let metadata = CallMetadata::new()
            .with_timeout_ms(5000)
            .with_idempotency_key("test-key".to_string());

        let host_call = HostCall {
            capability_id: "ccos.test.capability".to_string(),
            args: vec![Value::String("test".to_string())],
            security_context,
            causal_context: Some(causal_context),
            metadata: Some(metadata),
        };

        assert_eq!(host_call.capability_id, "ccos.test.capability");
        assert_eq!(host_call.args.len(), 1);
        assert!(host_call.causal_context.is_some());
        assert!(host_call.metadata.is_some());
    }

    #[test]
    fn test_execution_outcome_requires_host() {
        let security_context = RuntimeContext::full();
        let host_call = HostCall {
            capability_id: "ccos.test".to_string(),
            args: vec![],
            security_context,
            causal_context: None,
            metadata: None,
        };

        let outcome = ExecutionOutcome::RequiresHost(host_call);

        match outcome {
            ExecutionOutcome::RequiresHost(call) => {
                assert_eq!(call.capability_id, "ccos.test");
            }
            ExecutionOutcome::Complete(_) => panic!("Expected RequiresHost"),
        }
    }
}
