// DialoguePlanner: Conversational planning with external entities
//
// This planner interleaves dialogue turns with planning actions:
// - Build IntentGraph through conversation
// - Discover and connect MCP servers
// - Resolve and synthesize capabilities
// - Execute safe grounding steps
// - Generate RTFS plans progressively
//
// Autonomy level is governed by the Constitution and can increase over time.

pub mod entity;
pub mod planner;
pub mod presenter;
pub mod turn_processor;
pub mod types;

#[cfg(test)]
mod tests;

pub use entity::{DialogueEntity, HumanEntity, LlmEntity};
pub use planner::{DialoguePlanner, DialogueResult};
pub use types::*;
