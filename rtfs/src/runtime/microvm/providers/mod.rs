//! MicroVM Provider Implementations

pub mod firecracker;
pub mod gvisor;
pub mod mock;
pub mod process;
pub mod wasm;

use crate::runtime::error::RuntimeResult;
use crate::runtime::microvm::core::{ExecutionContext, ExecutionResult};

/// Trait for MicroVM providers that can execute programs in isolated environments
pub trait MicroVMProvider: Send + Sync {
    /// Name of this MicroVM provider
    fn name(&self) -> &'static str;

    /// Check if this provider is available on the current system
    fn is_available(&self) -> bool;

    /// Initialize the MicroVM provider
    fn initialize(&mut self) -> RuntimeResult<()>;

    /// Execute a program with capability permissions (NEW: primary method)
    fn execute_program(&self, context: ExecutionContext) -> RuntimeResult<ExecutionResult>;

    /// Execute a specific capability (for backward compatibility)
    fn execute_capability(&self, context: ExecutionContext) -> RuntimeResult<ExecutionResult> {
        // Default implementation for backward compatibility
        self.execute_program(context)
    }

    /// Legacy execute method (deprecated, use execute_program or execute_capability)
    #[deprecated(
        since = "2.0.0",
        note = "Use execute_program or execute_capability instead"
    )]
    fn execute(&self, context: ExecutionContext) -> RuntimeResult<ExecutionResult> {
        self.execute_capability(context)
    }

    /// Cleanup resources
    fn cleanup(&mut self) -> RuntimeResult<()>;

    /// Get provider-specific configuration options
    fn get_config_schema(&self) -> serde_json::Value;
}
