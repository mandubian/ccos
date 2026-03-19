//! Agent Runtime Submodule.
//!
//! Contains all logic for running an agent locally, including parsing its SKILL.md,
//! managing Tier 1 and Tier 2 memory, and enforcing the execution lifecycle.

pub mod analysis;
pub mod approved_exec_cache;
pub mod artifact;
pub mod capability_inference;
pub mod content_store;
pub mod crypto;
pub mod disclosure;
pub mod guard;
pub mod lifecycle;
pub mod mcp;
pub mod memory;
pub mod openrouter_catalog;
pub mod parser;
pub mod promotion_store;
pub mod reevaluation_state;
pub mod remote_access;
pub mod session_budget;
pub mod session_context;
pub mod session_snapshot;
pub mod session_timeline;
pub mod session_tracer;
pub mod store;
pub mod tool_call_processor;
pub mod tools;
pub mod tools_promotion;
