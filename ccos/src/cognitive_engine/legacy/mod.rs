//! Legacy arbiter engines
//!
//! This module contains older / alternative arbiter implementations that are
//! not used by the primary CCOS runtime path (which uses `DelegatingCognitiveEngine`).
//! They are kept for experiments, standalone usage, and historical reference.

pub mod arbiter_factory;
pub mod dummy_arbiter;
pub mod hybrid_arbiter;
pub mod llm_arbiter;
pub mod template_arbiter;

pub use arbiter_factory::ArbiterFactory;
pub use dummy_arbiter::DummyArbiter;
pub use hybrid_arbiter::HybridArbiter;
pub use llm_arbiter::LlmArbiter;
pub use template_arbiter::TemplateArbiter;
