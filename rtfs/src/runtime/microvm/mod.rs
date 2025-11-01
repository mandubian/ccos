//! MicroVM Abstraction Layer for RTFS/CCOS
//!
//! This module provides a pluggable architecture for secure execution environments
//! that can isolate dangerous operations like network access, file I/O, and system calls.

pub mod config;
pub mod core;
pub mod factory;
pub mod providers;
pub mod settings;
#[cfg(test)]
pub mod tests;

pub use config::{FileSystemPolicy, MicroVMConfig, NetworkPolicy};
pub use core::{
    ExecutionContext, ExecutionMetadata, ExecutionResult, FileOperation, NetworkRequest, Program,
};
pub use factory::MicroVMFactory;
pub use providers::MicroVMProvider;
pub use settings::{EnvironmentMicroVMConfig, MicroVMSettings};

// Re-export provider implementations
pub use providers::firecracker::FirecrackerMicroVMProvider;
pub use providers::gvisor::GvisorMicroVMProvider;
pub use providers::mock::MockMicroVMProvider;
pub use providers::process::ProcessMicroVMProvider;
pub use providers::wasm::WasmMicroVMProvider;
