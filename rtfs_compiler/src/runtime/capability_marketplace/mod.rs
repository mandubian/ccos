// Capability Marketplace module - split from monolithic file for maintainability

pub mod types;
pub mod executors;
pub mod discovery;
pub mod marketplace;
pub mod resource_monitor;
// Temporarily disabled to fix resource monitoring tests
// pub mod network_discovery;
pub mod mcp_discovery;
// pub mod a2a_discovery;

pub use types::*;
pub use executors::*;
pub use discovery::*;
pub use resource_monitor::*;
// Temporarily disabled to fix resource monitoring tests
// pub use network_discovery::*;
// pub use mcp_discovery::*;
// pub use a2a_discovery::*;
