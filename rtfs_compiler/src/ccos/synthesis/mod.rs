use crate::runtime::error::RuntimeResult;

pub mod api_introspector;
pub mod mcp_introspector;
pub mod mcp_session;
pub mod schema_serializer;
pub mod artifact_generator;
pub mod auth_injector;
pub mod capability_synthesizer;
pub mod continuous_resolution;
pub mod dependency_extractor;
pub mod feature_flags;
pub mod governance_policies;
pub mod graphql_importer;
pub mod http_wrapper;
pub mod mcp_proxy_adapter;
pub mod mcp_registry_client;
pub mod missing_capability_resolver;
pub mod openapi_importer;
pub mod preference_schema;
pub mod registration_flow;
pub mod schema_builder;
pub mod skill_extractor;
pub mod static_analyzers;
pub mod status;
pub mod telemetry;
pub mod validation_harness;
pub mod web_search_discovery;

// Integration tests live in a sibling file to keep the main module tidy.
#[cfg(test)]
mod integration_tests;
#[cfg(test)]
mod runtime_integration_tests;

// Explicitly re-export commonly used items so examples and consumers can
// import them from `rtfs_compiler::ccos::synthesis` directly.
pub use preference_schema::{extract_with_metrics, ParamType};
// Keep a blanket re-export for convenience (non-breaking)
pub use preference_schema::*;
pub use skill_extractor::*;
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
    pub pending_capabilities: Vec<String>, // Capabilities that need to be resolved before execution
    pub metrics: SynthesisMetrics,
}

/// Metrics computed during synthesis (spec section 23.5).
#[derive(Debug)]
pub struct SynthesisMetrics {
    pub coverage: f64,         // collected_required / total_required_detected
    pub redundancy: f64,       // duplicate_questions / total_questions
    pub enum_specificity: f64, // avg(enum_cardinality_weighted)
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
        Self {
            id: id.into(),
            description: None,
        }
    }
    pub fn with_description(mut self, d: impl Into<String>) -> Self {
        self.description = Some(d.into());
        self
    }
}

/// Entrypoint for capability synthesis. Returns a synthesized capability
/// that may have pending dependencies requiring resolution.
pub fn synthesize_from_dialogue(_dialogue: &str) -> RuntimeResult<SynthesizedCapability> {
    Ok(SynthesizedCapability::new("synth.capability.placeholder"))
}

// ===== Phase 8 Synthesis Pipeline Entry Point =====

/// Synthesize RTFS capabilities from conversation history.
///
/// Phase 8 implementation: extracts parameters, generates collector/planner/stub, emits telemetry.
pub fn synthesize_capabilities(_conversation: &[InteractionTurn]) -> SynthesisResult {
    // Backwards-compatible call: no marketplace snapshot available -> delegate
    synthesize_capabilities_with_marketplace(_conversation, &[])
}

/// New: synthesis entrypoint that allows providing a marketplace snapshot for registry-first planner generation (v0.1)
pub fn synthesize_capabilities_with_marketplace(
    _conversation: &[InteractionTurn],
    marketplace_snapshot: &[crate::ccos::capability_marketplace::types::CapabilityManifest],
) -> SynthesisResult {
    // Phase 8 minimal implementation: extract params, generate artifacts, emit minimal metrics.
    let schema = schema_builder::extract_param_schema(_conversation);

    let collector = artifact_generator::generate_collector(&schema, "synth.domain");
    // Use v0.1 registry-first planner when a marketplace snapshot is provided
    let planner = if !marketplace_snapshot.is_empty() {
        artifact_generator::generate_planner_generic_v0_1(
            &schema,
            _conversation,
            "synth.domain",
            marketplace_snapshot,
        )
    } else {
        artifact_generator::generate_planner(&schema, _conversation, "synth.domain")
    };

    // Collect any missing capabilities that need to be resolved
    let mut pending_capabilities = Vec::new();

    // Phase 1: Extract dependencies from generated artifacts
    let mut dependency_metadata = std::collections::HashMap::new();
    if !planner.is_empty() {
        if let Ok(dep_result) = dependency_extractor::extract_dependencies(&planner) {
            // Check dependencies against marketplace
            let (resolved, missing) = dependency_extractor::check_dependencies_against_marketplace(
                &dep_result.dependencies,
                marketplace_snapshot,
            );

            // Add dependency metadata
            dependency_metadata.insert(
                "dependencies.total".to_string(),
                dep_result.dependencies.len().to_string(),
            );
            dependency_metadata.insert(
                "dependencies.resolved".to_string(),
                resolved.len().to_string(),
            );
            dependency_metadata.insert(
                "dependencies.missing".to_string(),
                missing.len().to_string(),
            );

            if !missing.is_empty() {
                let missing_list: Vec<String> = missing.iter().cloned().collect();
                dependency_metadata
                    .insert("needs_capabilities".to_string(), missing_list.join(","));

                // Add missing capabilities to pending list for deferred execution
                pending_capabilities.extend(missing.clone());

                // Create audit event data for missing dependencies
                let audit_data = dependency_extractor::create_audit_event_data(
                    "synth.domain.planner.v1",
                    &missing,
                );
                eprintln!(
                    "AUDIT: capability_deps_missing - {}",
                    audit_data
                        .get("missing_capabilities")
                        .unwrap_or(&"none".to_string())
                );
            }
        }
    }

    // Compute naive metrics
    let turns_total = _conversation.len();
    let total_required = schema.params.len();
    let collected = schema
        .params
        .iter()
        .filter(|(_, m)| m.answer.is_some())
        .count();

    let coverage = if total_required == 0 {
        1.0
    } else {
        (collected as f64) / (total_required as f64)
    };

    SynthesisResult {
        collector: Some(collector),
        planner: Some(planner),
        pending_capabilities,
        metrics: SynthesisMetrics {
            coverage,
            redundancy: 0.0,
            enum_specificity: 0.0,
            missing_required: schema
                .params
                .iter()
                .filter(|(_, m)| m.answer.is_none())
                .map(|(k, _)| k.clone())
                .collect(),
            turns_total,
        },
    }
}
