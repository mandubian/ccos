// RTFS Capability Provider - Trait definitions
// Note: Implementations are provided by CCOS when RTFS is used with CCOS

use crate::runtime::error::RuntimeResult;
use crate::runtime::values::Value;

/// Trait for capability providers
pub trait CapabilityProvider: Send + Sync {
    fn execute(&self, args: &[Value]) -> RuntimeResult<Value>;
    fn name(&self) -> &str;
}
