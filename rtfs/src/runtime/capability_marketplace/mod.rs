// RTFS Capability Marketplace - Placeholder types
// Note: Full implementation is provided by CCOS when RTFS is used with CCOS

use crate::runtime::error::RuntimeResult;
use crate::runtime::values::Value;

/// Placeholder for capability marketplace
/// CCOS provides the full implementation
#[derive(Debug, Clone)]
pub struct CapabilityMarketplace;

impl CapabilityMarketplace {
    pub fn new() -> Self {
        Self
    }

    pub async fn execute_capability(&self, _id: &str, _args: &[Value]) -> RuntimeResult<Value> {
        Err(crate::runtime::error::RuntimeError::NotImplemented(
            "Capability execution requires CCOS integration".to_string(),
        ))
    }
}
