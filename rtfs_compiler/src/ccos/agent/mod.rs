// CCOS Agent module: consolidates agent registry and Simple* agent types under ccos::agent

pub mod registry;
pub mod types;
pub mod discovery;

// Re-export for convenience at ccos::agent::*
pub use registry::*;
pub use types::*;
pub use discovery::*;
