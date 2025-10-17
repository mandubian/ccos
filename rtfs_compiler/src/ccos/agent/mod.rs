// CCOS Agent module: consolidates agent registry and Simple* agent types under ccos::agent

pub mod discovery;
pub mod registry;
pub mod registry_shim;
pub mod types;

// Re-export for convenience at ccos::agent::*
pub use discovery::*;
pub use registry::*;
pub use types::*;
