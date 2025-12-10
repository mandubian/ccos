//! Agents Module for CCOS
//!
//! Provides persistent agent identities with memory and learning capabilities.
//! Designed to support future multi-agent coordination and task delegation.

pub mod capabilities;
pub mod identity;
pub mod memory;

pub use identity::{AgentConstraints, AgentIdentity, AgentRegistry};
pub use memory::{AgentMemory, LearnedPattern};
