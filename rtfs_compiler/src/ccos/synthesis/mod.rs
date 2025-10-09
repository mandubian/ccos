use crate::runtime::error::RuntimeResult;
use std::sync::Arc;

pub mod preference_schema;
pub mod status;
pub mod schema_builder;
pub mod artifact_generator;
pub mod telemetry;

// Explicitly re-export commonly used items so examples and consumers can
// import them from `rtfs_compiler::ccos::synthesis` directly.
pub use preference_schema::{extract_with_metrics, ParamType};
// Keep a blanket re-export for convenience (non-breaking)
pub use preference_schema::*;
pub use status::*;

// ===== Public Data Types for Synthesis Pipeline =====

/// Single conversation turn (prompt + answer pair from CausalChain user.ask actions).
#[derive(Debug, Clone)]
pub struct InteractionTurn {
    pub turn_index: usize,
    pub prompt: String,
    pub answer: Option<String>,
}

/// Result of synthesis operation (Phase 8 API).
#[derive(Debug)]
pub struct SynthesisResult {
    pub collector: Option<String>,
    pub planner: Option<String>,
    pub stub: Option<String>,
    pub metrics: SynthesisMetrics,
}

/// Metrics computed during synthesis (spec section 23.5).
#[derive(Debug)]
pub struct SynthesisMetrics {
    pub coverage: f64,           // collected_required / total_required_detected
    pub redundancy: f64,          // duplicate_questions / total_questions
    pub enum_specificity: f64,    // avg(enum_cardinality_weighted)
    pub missing_required: Vec<String>,
    pub turns_total: usize,
}

/// Represents a synthesized capability artifact placeholder.
#[derive(Debug, Clone)]
pub struct SynthesizedCapability {
	pub id: String,
	pub description: Option<String>,
}

impl SynthesizedCapability {
	pub fn new(id: impl Into<String>) -> Self {
		Self { id: id.into(), description: None }
	}
	pub fn with_description(mut self, d: impl Into<String>) -> Self {
		self.description = Some(d.into());
		self
	}
}

/// Stub entrypoint for capability synthesis. Returns a small placeholder
/// and is intended to be replaced by a full synthesis pipeline.
pub fn synthesize_from_dialogue(_dialogue: &str) -> RuntimeResult<SynthesizedCapability> {
	Ok(SynthesizedCapability::new("synth.capability.placeholder"))
}

// ===== Phase 8 Synthesis Pipeline Entry Point =====

/// Synthesize RTFS capabilities from conversation history.
///
/// Phase 8 implementation: extracts parameters, generates collector/planner/stub, emits telemetry.
pub fn synthesize_capabilities(
    _conversation: &[InteractionTurn],
    _ccos: &Arc<crate::ccos::CCOS>,
) -> SynthesisResult {
    // Phase 8 minimal implementation: extract params, generate artifacts, emit minimal metrics.
    let schema = schema_builder::extract_param_schema(_conversation, _ccos);

    let collector = artifact_generator::generate_collector(&schema, "synth.domain");
    let planner = artifact_generator::generate_planner(&schema, "synth.domain");

    // If a domain-specific agent is missing, synthesize a stub id
    let stub_id = format!("synth.domain.agent.stub");
    let stub = artifact_generator::generate_stub(&stub_id, &schema.params.keys().cloned().collect::<Vec<_>>());

    // Compute naive metrics
    let turns_total = _conversation.len();
    let total_required = schema.params.len();
    let collected = schema.params.iter().filter(|(_, m)| m.answer.is_some()).count();

    let coverage = if total_required == 0 { 1.0 } else { (collected as f64) / (total_required as f64) };

    SynthesisResult {
        collector: Some(collector),
        planner: Some(planner),
        stub: Some(stub),
        metrics: SynthesisMetrics {
            coverage,
            redundancy: 0.0,
            enum_specificity: 0.0,
            missing_required: schema.params.iter().filter(|(_,m)| m.answer.is_none()).map(|(k,_)| k.clone()).collect(),
            turns_total,
        },
    }
}
