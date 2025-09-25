//! Pure host implementation for RTFS-only testing and standalone usage
//!
//! This provides a minimal implementation of HostInterface that doesn't
//! depend on CCOS components like CausalChain or CapabilityMarketplace.

use crate::runtime::error::RuntimeResult;
use crate::runtime::host_interface::HostInterface;
use crate::runtime::values::Value;
use std::sync::Arc;

/// A pure host implementation that provides minimal functionality
/// for RTFS testing without CCOS dependencies
#[derive(Debug)]
pub struct PureHost;

impl PureHost {
    pub fn new() -> Self {
        Self
    }
}

impl HostInterface for PureHost {
    fn execute_capability(&self, _name: &str, _args: &[Value]) -> RuntimeResult<Value> {
        // For pure RTFS testing, we can either:
        // 1. Return a default value
        // 2. Return an error indicating capability not available
        // 3. Implement a few basic capabilities for testing

        // For now, return an error to make it clear that capabilities
        // are not available in pure mode
        Err(crate::runtime::error::RuntimeError::Generic(
            format!("Capability '{}' not available in pure RTFS mode. Use CCOS host for full capability support.", _name)
        ))
    }

    fn notify_step_started(&self, _step_name: &str) -> RuntimeResult<String> {
        // Pure mode: return a dummy action ID
        Ok("pure-step-action-id".to_string())
    }

    fn notify_step_completed(
        &self,
        _step_action_id: &str,
        _result: &crate::ccos::types::ExecutionResult,
    ) -> RuntimeResult<()> {
        // Pure mode: no-op for step notifications
        Ok(())
    }

    fn notify_step_failed(&self, _step_action_id: &str, _error: &str) -> RuntimeResult<()> {
        // Pure mode: no-op for step notifications
        Ok(())
    }

    fn set_execution_context(
        &self,
        _plan_id: String,
        _intent_ids: Vec<String>,
        _parent_action_id: String,
    ) {
        // Pure mode: no-op for execution context
    }

    fn clear_execution_context(&self) {
        // Pure mode: no-op for execution context
    }

    fn set_step_exposure_override(&self, _expose: bool, _context_keys: Option<Vec<String>>) {
        // Pure mode: no-op for step exposure override
    }

    fn clear_step_exposure_override(&self) {
        // Pure mode: no-op for step exposure override
    }

    fn get_context_value(&self, _key: &str) -> Option<Value> {
        // Pure mode: no context values available
        None
    }
}

impl Default for PureHost {
    fn default() -> Self {
        Self::new()
    }
}

/// Convenience function to create a PureHost wrapped in Arc
pub fn create_pure_host() -> Arc<dyn HostInterface> {
    Arc::new(PureHost::new())
}
