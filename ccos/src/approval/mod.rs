//! Approval system for capability discovery and effect execution.
//!
//! This module provides a unified approval queue for managing various types
//! of approval requests including server discovery, effect-based execution,
//! capability synthesis, and LLM prompts.

pub mod queue;
pub mod runtime_state;
pub mod storage_causal;
pub mod storage_file;
pub mod storage_memory;
pub mod types;
pub mod unified_queue;

// Re-export main types for convenience
pub use queue::*;
pub use types::*;
pub use unified_queue::{suggest_auth_env_var, UnifiedApprovalQueue};
