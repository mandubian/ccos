// RTFS Compiler Library
// Exposes modules for binary targets and external use

pub mod ast;
pub mod parser;
pub mod runtime;
pub mod ir;
pub mod ir_converter;
pub mod ir_optimizer;
pub mod enhanced_ir_optimizer;
pub mod development_tooling;
pub mod integration_tests;
pub mod agent;
pub mod error_reporting;

// Re-export commonly used types for convenience
pub use runtime::{Runtime, RuntimeStrategy, Evaluator};
pub use development_tooling::{RtfsRepl, RtfsTestFramework};
pub use parser::parse_expression;
pub use agent::{AgentDiscoveryClient, AgentCommunicationClient, AgentRegistry, AgentProfileManager};

