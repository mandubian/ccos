//! Execution Outcome Types for RTFS-CCOS Control Flow Inversion
//!
//! This module defines the types used for RTFS to yield control back to CCOS
//! when it encounters non-pure operations that require delegation decisions.

use crate::runtime::values::Value;
use crate::runtime::security::RuntimeContext;
use serde_json::{self, Value as JsonValue};
use std::collections::HashMap;

/// Simple demonstration of the effect boundary concept
/// This shows how Phase 3's EffectRequest would work in practice
#[cfg(feature = "effect-boundary")]
pub mod effect_boundary_demo {

    use super::*;
    use serde_json::json;

    #[cfg(feature = "effect-boundary")]
    #[test]
    fn test_host_capabilities_integration() {
        // Test that the state capabilities are registered and working
        let registry = crate::ccos::capabilities::registry::CapabilityRegistry::new();

        // Test that all 5 state capabilities are registered
        assert!(registry.get_capability("ccos.state.kv.get").is_some());
        assert!(registry.get_capability("ccos.state.kv.put").is_some());
        assert!(registry.get_capability("ccos.state.kv.cas-put").is_some());
        assert!(registry.get_capability("ccos.state.counter.inc").is_some());
        assert!(registry.get_capability("ccos.state.event.append").is_some());

        println!("✅ All 5 host-backed state capabilities are registered");
    }

    #[cfg(feature = "effect-boundary")]
    #[test]
    fn test_kv_get_capability_demo() {
        let args = vec![Value::String("test-key".to_string())];
        let result = kv_get_capability(args);

        match result {
            Ok(Value::String(s)) => {
                assert!(s.starts_with("mock-value-for-"));
                println!("✅ kv.get capability works: {}", s);
            }
            Ok(_) => panic!("Expected string result"),
            Err(e) => panic!("kv.get failed: {:?}", e),
        }
    }

    #[cfg(feature = "effect-boundary")]
    #[test]
    fn test_counter_inc_capability_demo() {
        let args = vec![Value::String("test-counter".to_string()), Value::Integer(5)];
        let result = counter_inc_capability(args);

        match result {
            Ok(Value::Integer(n)) => {
                assert_eq!(n, 42); // Mock result
                println!("✅ counter.inc capability works: returned {}", n);
            }
            Ok(_) => panic!("Expected integer result"),
            Err(e) => panic!("counter.inc failed: {:?}", e),
        }
    }

    #[cfg(feature = "effect-boundary")]
    #[test]
    fn test_event_append_capability_demo() {
        let args = vec![Value::String("test-log".to_string())];
        let result = event_append_capability(args);

        match result {
            Ok(Value::Boolean(b)) => {
                assert_eq!(b, true);
                println!("✅ event.append capability works: returned {}", b);
            }
            Ok(_) => panic!("Expected boolean result"),
            Err(e) => panic!("event.append failed: {:?}", e),
        }
    }

    /// Demonstration function showing the effect boundary in action
    #[cfg(feature = "effect-boundary")]
    pub fn demonstrate_effect_boundary() -> Result<(), Box<dyn std::error::Error>> {
        println!("🎯 RTFS Effect Boundary Demonstration");
        println!("====================================");

        // 1. Create a security context
        let security_context = RuntimeContext::pure();
        println!("✅ Created security context: {:?}", security_context);

        // 2. Create causal context with intent and step IDs
        let causal_context = CausalContext {
            intent_id: Some("intent-123".to_string()),
            step_id: Some("step-456".to_string()),
            plan_id: Some("plan-789".to_string()),
        };
        println!("✅ Created causal context: {:?}", causal_context);

        // 3. Create an EffectRequest for a counter increment
        let effect_request = EffectRequest {
            capability_id: "ccos.counter:v1.inc".to_string(),
            input_payload: json!({
                "key": "my-counter",
                "increment": 1,
                "initial_value": 0
            }),
            security_context: security_context.clone(),
            causal_context: Some(causal_context),
            timeout_ms: Some(5000),
            idempotency_key: Some("inc-counter-123".to_string()),
            metadata: None,
        };
        println!("✅ Created EffectRequest: {}", effect_request.capability_id);

        // 4. Show the EffectRequest details
        println!("📋 EffectRequest Details:");
        println!("   - Capability ID: {}", effect_request.capability_id);
        println!("   - Timeout: {:?}", effect_request.timeout_ms);
        println!("   - Idempotency Key: {:?}", effect_request.idempotency_key);
        println!("   - Input Payload: {}", effect_request.input_payload);

        // 5. Demonstrate the ExecutionOutcome with EffectRequest
        let outcome = ExecutionOutcome::RequiresHostEffect(effect_request.clone());
        match outcome {
            ExecutionOutcome::RequiresHostEffect(req) => {
                println!("✅ Created ExecutionOutcome::RequiresHostEffect");
                println!("   - Requesting capability: {}", req.capability_id);
                println!("   - This would normally be sent to CCOS Host for processing");

                // 6. Simulate Host processing and response
                println!("🔄 Simulating Host processing...");
                let mock_response = simulate_host_processing(&req);
                println!("✅ Host would return: {}", mock_response);
            }
            _ => println!("❌ Unexpected outcome type"),
        }

        println!("\n🎉 Effect Boundary demonstration completed successfully!");
        println!("This shows how Phase 3 enables typed, structured host calls");
        println!("with full causal context, security, and idempotency support.");

        Ok(())
    }

    /// Simulate host processing of an EffectRequest
    fn simulate_host_processing(req: &EffectRequest) -> JsonValue {
        // In a real implementation, this would be handled by the CCOS Host
        // For demo purposes, we simulate processing the counter increment

        if let Some(increment) = req.input_payload.get("increment") {
            if let Some(initial) = req.input_payload.get("initial_value") {
                if let (Some(inc_val), Some(init_val)) = (increment.as_i64(), initial.as_i64()) {
                    let new_value = init_val + inc_val;
                    return json!({
                        "success": true,
                        "capability_id": req.capability_id,
                        "result": new_value,
                        "message": format!("Counter incremented from {} to {}", init_val, new_value)
                    });
                }
            }
        }

        json!({
            "success": false,
            "error": "Invalid counter increment request"
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // Note: EffectRequest and CausalContext tests removed as these types are no longer available
    // when effect-boundary feature is disabled

    #[test]
    fn test_execution_outcome_with_effect_request() {
        #[cfg(feature = "effect-boundary")]
        {
            // Test creating an ExecutionOutcome with EffectRequest
            let security_context = RuntimeContext::pure();
            let effect_request = EffectRequest {
                capability_id: "ccos.counter:v1.inc".to_string(),
                input_payload: json!({"key": "test-counter"}),
                security_context,
                causal_context: None,
                timeout_ms: None,
                idempotency_key: None,
                metadata: None,
            };

            let outcome = ExecutionOutcome::RequiresHostEffect(effect_request);
            match outcome {
                ExecutionOutcome::RequiresHostEffect(req) => {
                    assert_eq!(req.capability_id, "ccos.counter:v1.inc");
                }
                _ => panic!("Expected RequiresHostEffect variant"),
            }
        }
    }

    #[test]
    fn test_effect_boundary_demo() {
        #[cfg(feature = "effect-boundary")]
        {
            // Test that the demo function runs without errors
            match effect_boundary_demo::demonstrate_effect_boundary() {
                Ok(_) => {},
                Err(e) => panic!("Demo failed: {:?}", e),
            }
        }
    }
}

/// Causal context information for tracking the origin of a host call
#[cfg(feature = "effect-boundary")]
#[derive(Debug, Clone)]
pub struct CausalContext {
    /// The intent ID that originated this call
    pub intent_id: Option<String>,
    /// The step ID within the intent
    pub step_id: Option<String>,
    /// The plan ID if this is part of a plan execution
    pub plan_id: Option<String>,
}

/// Typed effect request envelope for Host calls
/// This extends HostCall with additional metadata required for the new effect boundary
#[cfg(feature = "effect-boundary")]
#[derive(Debug, Clone)]
pub struct EffectRequest {
    /// The fully-qualified capability identifier (e.g., "ccos.counter:v1.inc")
    pub capability_id: String,
    /// The input payload for the capability call
    pub input_payload: serde_json::Value,
    /// Security context for the call
    pub security_context: RuntimeContext,
    /// Causal context tracking the origin of this call
    pub causal_context: Option<CausalContext>,
    /// Optional timeout in milliseconds
    pub timeout_ms: Option<u64>,
    /// Idempotency key for retry purposes
    pub idempotency_key: Option<String>,
    /// Additional metadata (backwards compatibility with existing HostCall)
    pub metadata: Option<CallMetadata>,
}

/// The outcome of executing an RTFS node.
/// RTFS execution can either complete locally or require host intervention.
#[derive(Debug, Clone)]
pub enum ExecutionOutcome {
    /// Execution completed successfully with a value.
    Complete(Value),
    /// Execution requires host intervention for a non-pure operation (legacy format).
    RequiresHost(HostCall),
    /// Execution requires host intervention for a non-pure operation (new typed format).
    #[cfg(feature = "effect-boundary")]
    RequiresHostEffect(EffectRequest),
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