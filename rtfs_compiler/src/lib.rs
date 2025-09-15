// RTFS Compiler Library
// Main library crate for the RTFS compiler
pub mod ast;
pub mod ccos;
pub mod config;
pub mod development_tooling;
pub mod error_reporting;
pub mod input_handling;
pub mod ir;
pub mod parser;
pub mod parser_error_reporter;
pub mod runtime;

pub mod supervisor;
pub mod utils;
pub mod validator;
pub mod bytecode;

// Test modules
#[cfg(test)]
mod tests;

// In lib.rs, we re-export the key components from our submodules
// to make them accessible to other parts of the crate or external users.

// Re-export the main parsing function and the AST.
pub use ast::*;
pub use development_tooling::{RtfsRepl, RtfsTestFramework};
pub use parser::{errors::PestParseError, parse, parse_expression};
pub use runtime::evaluator::Evaluator;
pub use runtime::{Runtime, RuntimeStrategy};

// Re-export IR modules for external use
pub use ir::core::*;

// Re-export all RTFS 2.0 object builders
pub mod builders;
pub use builders::{
    ActionBuilder, CapabilityBuilder, IntentBuilder, ModuleBuilder, PlanBuilder, ResourceBuilder,
};

// Re-export RTFS utilities
pub use utils::*;

// Re-export CCOS components
pub use ccos::types::*;
