//! Agent Runtime Submodule.
//!
//! Contains all logic for running an agent locally, including parsing its SKILL.md,
//! managing Tier 1 and Tier 2 memory, and enforcing the execution lifecycle.

pub mod analysis;
pub mod artifact;
pub mod capability_inference;
pub mod content_store;
pub mod crypto;
pub mod disclosure;
pub mod guard;
pub mod lifecycle;
pub mod mcp;
pub mod memory;
pub mod parser;
pub mod reevaluation_state;
pub mod remote_access;
pub mod session_context;
pub mod session_snapshot;
pub mod session_tracer;
pub mod store;
pub mod tool_call_processor;
pub mod tools;
