// RTFS Compiler Library
// Exposes modules for binary targets and external use

pub mod ast;
// pub mod compiler; // TODO: Add compiler module
pub mod error_reporting;
pub mod parser;
pub mod runtime;
pub mod validator;
pub mod development_tooling;
pub mod agent;

// For access to IR converter and optimizer
pub mod ir;

// Test modules
#[cfg(test)]
mod tests;

// In lib.rs, we re-export the key components from our submodules
// to make them accessible to other parts of the crate or external users.

// Re-export the main parsing function and the AST.
pub use ast::*;
pub use parser::{parse, parse_expression, errors::PestParseError};
pub use runtime::{Runtime, RuntimeStrategy};
pub use runtime::evaluator::Evaluator;
pub use development_tooling::{RtfsRepl, RtfsTestFramework};
pub use agent::{AgentDiscoveryClient, AgentCommunicationClient, AgentRegistry, AgentProfileManager};

// Re-export IR modules for external use
pub use ir::core::*;
pub use ir::converter::*;
pub use ir::optimizer::*;
pub use ir::enhanced_optimizer::*;
pub use ir::demo::*;