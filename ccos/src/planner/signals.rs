use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

use rtfs::runtime::values::Value;

use crate::catalog::{CatalogEntryKind, CatalogFilter, CatalogService};
use crate::types::Intent;

/// Source of a goal signal.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum GoalSignalSource {
    GoalText,
    IntentConstraint,
    IntentPreference,
    ClarifyingAnswer,
    SuccessCriterion,
    Derived { rationale: Option<String> },
}

/// Strength associated with a requirement.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum RequirementPriority {
    Must,
    Should,
    NiceToHave,
}

/// Readiness state of a requirement with respect to available capabilities.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum RequirementReadiness {
    Unknown,
    Identified,
    Incomplete,
    PendingExternal,
    Available,
}

impl Default for RequirementReadiness {
    fn default() -> Self {
        RequirementReadiness::Unknown
    }
}

impl RequirementReadiness {
    pub fn precedence(&self) -> u8 {
        match self {
            RequirementReadiness::Unknown => 0,
            RequirementReadiness::Identified => 10,
            RequirementReadiness::Incomplete => 20,
            RequirementReadiness::PendingExternal => 30,
            RequirementReadiness::Available => 40,
        }
    }
}

/// Enumerates the provenance of a capability once materialized.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum CapabilityProvisionSource {
    ExistingManifest,
    Synthesized,
    MCP,
    HumanProvided,
}

/// Represents a constraint extracted from user intent or clarifications.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoalConstraint {
    pub name: String,
    pub value: Value,
    pub source: GoalSignalSource,
    pub rationale: Option<String>,
}

/// Represents a user preference.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoalPreference {
    pub name: String,
    pub value: Value,
    pub source: GoalSignalSource,
    pub weight: Option<f32>,
    pub rationale: Option<String>,
}

/// Represents a success criterion the planner should satisfy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoalCriterion {
    pub description: String,
    pub source: GoalSignalSource,
}

/// Captures a derived requirement that must be covered by the plan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoalRequirement {
    pub id: String,
    pub kind: GoalRequirementKind,
    pub priority: RequirementPriority,
    pub source: GoalSignalSource,
    pub metadata: BTreeMap<String, Value>,
    #[serde(default)]
    pub readiness: RequirementReadiness,
    #[serde(default)]
    pub provision_source: Option<CapabilityProvisionSource>,
    #[serde(default)]
    pub pending_request_id: Option<String>,
    #[serde(default)]
    pub scaffold_summary: Option<String>,
}

impl GoalRequirement {
    pub fn capability_id(&self) -> Option<&str> {
        if let GoalRequirementKind::MustCallCapability { capability_id } = &self.kind {
            Some(capability_id.as_str())
        } else {
            None
        }
    }

    pub fn bump_readiness(&mut self, readiness: RequirementReadiness) {
        if readiness.precedence() > self.readiness.precedence() {
            self.readiness = readiness;
        }
    }

    pub fn merge_metadata<I>(&mut self, entries: I)
    where
        I: IntoIterator<Item = (String, Value)>,
    {
        for (key, value) in entries {
            self.metadata.insert(key, value);
        }
    }
}

/// Describes different forms of requirements used during coverage checks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GoalRequirementKind {
    MustCallCapability {
        capability_id: String,
    },
    MustSatisfyCapabilityClass {
        class: String,
    },
    MustProduceOutput {
        key: String,
    },
    MustFilter {
        field: Option<String>,
        expected_value: Option<Value>,
    },
    Custom {
        description: String,
    },
}

impl GoalRequirementKind {
    pub fn registry_key(&self) -> &'static str {
        match self {
            GoalRequirementKind::MustCallCapability { .. } => "MustCallCapability",
            GoalRequirementKind::MustSatisfyCapabilityClass { .. } => "MustSatisfyCapabilityClass",
            GoalRequirementKind::MustProduceOutput { .. } => "MustProduceOutput",
            GoalRequirementKind::MustFilter { .. } => "MustFilter",
            GoalRequirementKind::Custom { .. } => "Custom",
        }
    }
}

/// Aggregated signals extracted from goal + intent context.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GoalSignals {
    pub goal_text: String,
    pub constraints: Vec<GoalConstraint>,
    pub preferences: Vec<GoalPreference>,
    pub success_criteria: Vec<GoalCriterion>,
    pub requirements: Vec<GoalRequirement>,
    pub contextual_facts: BTreeMap<String, Value>,
}

impl GoalSignals {
    pub fn new<T: Into<String>>(goal_text: T) -> Self {
        Self {
            goal_text: goal_text.into(),
            ..Default::default()
        }
    }

    pub fn add_constraint(&mut self, constraint: GoalConstraint) {
        self.constraints.push(constraint);
    }

    pub fn add_success_criterion(&mut self, criterion: GoalCriterion) {
        self.success_criteria.push(criterion);
    }

    pub fn add_preference(&mut self, preference: GoalPreference) {
        self.preferences.push(preference);
    }

    pub fn add_requirement(&mut self, requirement: GoalRequirement) {
        self.requirements.push(requirement);
    }

    pub fn add_context_fact<S: Into<String>>(&mut self, key: S, value: Value) {
        self.contextual_facts.insert(key.into(), value);
    }

    pub fn ensure_must_call_capability(&mut self, capability_id: &str, rationale: Option<String>) {
        let mut metadata_entries = vec![(
            "capability_id".to_string(),
            Value::String(capability_id.to_string()),
        )];
        if let Some(ref rationale_text) = rationale {
            metadata_entries.push((
                "rationale".to_string(),
                Value::String(rationale_text.clone()),
            ));
        }

        if let Some(existing) = self.requirements.iter_mut().find(|req| {
            matches!(
                &req.kind,
                GoalRequirementKind::MustCallCapability { capability_id: existing }
                if existing == capability_id
            )
        }) {
            existing.bump_readiness(RequirementReadiness::Identified);
            existing.merge_metadata(metadata_entries);
            return;
        }

        let metadata = metadata_entries.into_iter().collect();

        self.add_requirement(GoalRequirement {
            id: format!("requirement::must_call::{}", capability_id),
            kind: GoalRequirementKind::MustCallCapability {
                capability_id: capability_id.to_string(),
            },
            priority: RequirementPriority::Must,
            source: GoalSignalSource::Derived { rationale },
            metadata,
            readiness: RequirementReadiness::Identified,
            provision_source: None,
            pending_request_id: None,
            scaffold_summary: None,
        });
    }

    pub fn from_goal_and_intent(goal_text: &str, intent: &Intent) -> Self {
        let mut signals = GoalSignals::new(goal_text.to_string());
        signals.absorb_intent(intent);
        signals
    }

    pub fn absorb_intent(&mut self, intent: &Intent) {
        self.add_context_fact("intent_id", Value::String(intent.intent_id.clone()));
        if let Some(name) = intent.name.as_ref() {
            self.add_context_fact("intent_name", Value::String(name.clone()));
        }

        self.ingest_constraints(&intent.constraints, GoalSignalSource::IntentConstraint);
        self.ingest_preferences(&intent.preferences, GoalSignalSource::IntentPreference);

        if let Some(criteria) = &intent.success_criteria {
            self.add_success_criterion(GoalCriterion {
                description: format!("{}", criteria),
                source: GoalSignalSource::SuccessCriterion,
            });
        }
    }

    pub fn constraints_map(&self) -> HashMap<String, Value> {
        let mut map = HashMap::new();
        for constraint in &self.constraints {
            map.insert(constraint.name.clone(), constraint.value.clone());
        }
        map
    }

    pub fn update_capability_requirement<I>(
        &mut self,
        capability_id: &str,
        readiness: RequirementReadiness,
        metadata: I,
    ) where
        I: IntoIterator<Item = (String, Value)>,
    {
        if let Some(requirement) = self.requirements.iter_mut().find(|req| {
            matches!(
                &req.kind,
                GoalRequirementKind::MustCallCapability { capability_id: existing }
                if existing == capability_id
            )
        }) {
            requirement.bump_readiness(readiness);
            requirement.merge_metadata(metadata);
        }
    }

    pub fn set_provision_source(
        &mut self,
        capability_id: &str,
        source: Option<CapabilityProvisionSource>,
    ) {
        if let Some(requirement) = self.requirements.iter_mut().find(|req| {
            matches!(
                &req.kind,
                GoalRequirementKind::MustCallCapability { capability_id: existing }
                if existing == capability_id
            )
        }) {
            requirement.provision_source = source;
        }
    }

    pub fn set_pending_request_id(&mut self, capability_id: &str, request_id: Option<String>) {
        if let Some(requirement) = self.requirements.iter_mut().find(|req| {
            matches!(
                &req.kind,
                GoalRequirementKind::MustCallCapability { capability_id: existing }
                if existing == capability_id
            )
        }) {
            requirement.pending_request_id = request_id;
        }
    }

    pub fn set_scaffold_summary(&mut self, capability_id: &str, summary: Option<String>) {
        if let Some(requirement) = self.requirements.iter_mut().find(|req| {
            matches!(
                &req.kind,
                GoalRequirementKind::MustCallCapability { capability_id: existing }
                if existing == capability_id
            )
        }) {
            requirement.scaffold_summary = summary;
        }
    }

    pub fn capability_requirement(&self, capability_id: &str) -> Option<&GoalRequirement> {
        self.requirements.iter().find(|req| {
            matches!(
                &req.kind,
                GoalRequirementKind::MustCallCapability { capability_id: existing }
                if existing == capability_id
            )
        })
    }

    pub fn capability_readiness(&self, capability_id: &str) -> Option<RequirementReadiness> {
        self.capability_requirement(capability_id)
            .map(|req| req.readiness.clone())
    }

    /// Apply catalog search to extract capability requirements from goal signals.
    /// This replaces the old descriptor-based matching with catalog search.
    pub async fn apply_catalog_search(
        &mut self,
        catalog: &CatalogService,
        min_score: f32,
        max_results: usize,
    ) {
        // Build search query from goal text + constraints + contextual facts
        let mut query_parts = vec![self.goal_text.clone()];

        for constraint in &self.constraints {
            query_parts.push(constraint.name.clone());
            query_parts.push(value_to_lowercase_string(&constraint.value));
        }

        for (key, value) in &self.contextual_facts {
            query_parts.push(key.clone());
            query_parts.push(value_to_lowercase_string(value));
        }

        let query = query_parts.join(" ");

        // Search catalog for capabilities matching the goal signals
        let filter = CatalogFilter::for_kind(CatalogEntryKind::Capability);
        let hits = catalog
            .search_keyword(&query, Some(&filter), max_results * 2)
            .await;

        // Also try semantic search if keyword search returns few results
        let final_hits = if hits.len() < max_results / 2 {
            let semantic_hits = catalog
                .search_semantic(&query, Some(&filter), max_results)
                .await;
            // Merge and deduplicate (prefer higher scores)
            let mut combined: HashMap<String, _> = HashMap::new();
            for hit in hits {
                combined.insert(hit.entry.id.clone(), hit);
            }
            for hit in semantic_hits {
                let existing = combined.get(&hit.entry.id);
                if existing.is_none() || existing.map(|h| h.score) < Some(hit.score) {
                    combined.insert(hit.entry.id.clone(), hit);
                }
            }
            let mut result: Vec<_> = combined.into_values().collect();
            result.sort_by(|a, b| {
                b.score
                    .partial_cmp(&a.score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            result
        } else {
            hits
        };

        // Create requirements for high-scoring matches
        for hit in final_hits.into_iter().take(max_results) {
            if hit.score >= min_score {
                let capability_id = hit.entry.id.clone();
                let source_str = match hit.entry.source {
                    crate::catalog::CatalogSource::Discovered => "discovered",
                    crate::catalog::CatalogSource::Generated => "generated",
                    crate::catalog::CatalogSource::User => "user",
                    crate::catalog::CatalogSource::System => "system",
                    crate::catalog::CatalogSource::Unknown => "unknown",
                };
                let rationale = format!(
                    "Matched via catalog search (score: {:.2}, source: {})",
                    hit.score, source_str
                );

                self.ensure_must_call_capability(&capability_id, Some(rationale));
            }
        }
    }

    fn ingest_constraints(
        &mut self,
        constraints: &HashMap<String, Value>,
        source: GoalSignalSource,
    ) {
        for (name, value) in constraints {
            self.add_constraint(GoalConstraint {
                name: name.clone(),
                value: value.clone(),
                source: source.clone(),
                rationale: None,
            });
            self.derive_requirements_from_constraint(name, value, &source);
        }
    }

    fn ingest_preferences(
        &mut self,
        preferences: &HashMap<String, Value>,
        source: GoalSignalSource,
    ) {
        for (name, value) in preferences {
            self.add_preference(GoalPreference {
                name: name.clone(),
                value: value.clone(),
                source: source.clone(),
                weight: None,
                rationale: None,
            });
        }
    }

    fn derive_requirements_from_constraint(
        &mut self,
        name: &str,
        value: &Value,
        source: &GoalSignalSource,
    ) {
        let normalized = name.trim().to_ascii_lowercase();

        if normalized.contains("filter") {
            let expected_value = match value {
                Value::String(s) => Some(Value::String(s.clone())),
                Value::Integer(i) => Some(Value::Integer(*i)),
                Value::Boolean(b) => Some(Value::Boolean(*b)),
                _ => None,
            };

            self.add_requirement(GoalRequirement {
                id: format!("requirement::must_filter::{}", name),
                kind: GoalRequirementKind::MustFilter {
                    field: None,
                    expected_value,
                },
                priority: RequirementPriority::Must,
                source: source.clone(),
                metadata: BTreeMap::new(),
                readiness: RequirementReadiness::Unknown,
                provision_source: None,
                pending_request_id: None,
                scaffold_summary: None,
            });
        }
    }
}

fn value_to_lowercase_string(value: &Value) -> String {
    match value {
        Value::String(s) => s.to_lowercase(),
        Value::Integer(i) => i.to_string(),
        Value::Float(f) => f.to_string(),
        Value::Boolean(b) => b.to_string(),
        Value::Keyword(k) => k.0.to_lowercase(),
        Value::Symbol(sym) => sym.0.to_lowercase(),
        Value::Vector(items) => items
            .iter()
            .map(value_to_lowercase_string)
            .collect::<Vec<_>>()
            .join(" "),
        Value::Map(map) => {
            let mut parts = Vec::new();
            for (key, val) in map {
                let key_str = match key {
                    rtfs::ast::MapKey::String(s) => s.clone(),
                    rtfs::ast::MapKey::Keyword(k) => k.0.clone(),
                    rtfs::ast::MapKey::Integer(i) => i.to_string(),
                };
                parts.push(format!(
                    "{} {}",
                    key_str.to_lowercase(),
                    value_to_lowercase_string(val)
                ));
            }
            parts.join(" ")
        }
        _ => format!("{:?}", value).to_lowercase(),
    }
}
