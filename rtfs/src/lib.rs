// RTFS Language Library
// Main library crate for the RTFS language (parser, compiler, runtime)

pub mod ast;
pub mod config;
pub mod development_tooling;
pub mod error_reporting;
pub mod examples_helpers;
pub mod input_handling;
pub mod ir;
pub mod parser;
pub mod parser_error_reporter;
pub mod runtime;

pub mod bytecode;
pub mod supervisor;
pub mod utils;
pub mod validator;

// Test modules - tests are in tests/ directory and inline #[test] functions

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

