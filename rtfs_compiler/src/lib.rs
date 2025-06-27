// RTFS Compiler Library
// Exposes modules for binary targets and external use

pub mod ast;
pub mod ir;
pub mod ir_converter;
pub mod ir_optimizer;
pub mod runtime;
pub mod parser;
pub mod validator;
pub mod error_reporting;
pub mod integration_tests;
pub mod development_tooling;
pub mod agent;

// In lib.rs, we re-export the key components from our submodules
// to make them accessible to other parts of the crate or external users.

// Re-export the main parsing function and the AST.
pub use ast::*;
pub use parser::{parse, parse_expression, errors::PestParseError};
pub use runtime::{Runtime, RuntimeStrategy};
pub use runtime::evaluator::Evaluator;
pub use development_tooling::{RtfsRepl, RtfsTestFramework};
pub use agent::{AgentDiscoveryClient, AgentCommunicationClient, AgentRegistry, AgentProfileManager};