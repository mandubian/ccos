//! MicroVM Abstraction Layer for RTFS/CCOS
//!
//! This module provides a pluggable architecture for secure execution environments
//! that can isolate dangerous operations like network access, file I/O, and system calls.

pub mod config;
pub mod core;
pub mod factory;
pub mod providers;

pub use config::{MicroVMConfig, NetworkPolicy, FileSystemPolicy};
pub use core::{Program, ExecutionContext, ExecutionResult, ExecutionMetadata, NetworkRequest, FileOperation};
pub use factory::MicroVMFactory;
pub use providers::MicroVMProvider;

// Re-export provider implementations
pub use providers::mock::MockMicroVMProvider;
pub use providers::process::ProcessMicroVMProvider;
pub use providers::firecracker::FirecrackerMicroVMProvider;
pub use providers::gvisor::GvisorMicroVMProvider;
pub use providers::wasm::WasmMicroVMProvider;
