use super::storage::Archivable;
use super::types::{Action, Plan, PlanBody, PlanLanguage, PlanStatus, StorableIntent};
/// Serializable versions of CCOS types for unified storage architecture
///
/// This module provides archivable versions of CCOS entities that implement
/// the Archivable trait for content-addressable storage.
///
/// Note: For Intents, we reuse the existing StorableIntent from types.rs
/// rather than creating a duplicate ArchivableIntent, since StorableIntent
/// already serves as the official archivable version with full context.
use serde::{Deserialize, Serialize};
use serde_json;
use std::collections::HashMap;

/// Serializable version of Plan for archiving
/// Following the StorableIntent pattern, RTFS expressions are stored as strings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchivablePlan {
    pub plan_id: String,
    pub name: Option<String>,
    pub intent_ids: Vec<String>,
    pub language: ArchivablePlanLanguage,
    pub body: ArchivablePlanBody,
    pub status: PlanStatus,
    pub created_at: u64,
    pub metadata: HashMap<String, String>, // Simplified metadata as strings

    // New first-class Plan attributes
    pub input_schema: Option<String>, // Schema for plan inputs (serialized as JSON string)
    pub output_schema: Option<String>, // Schema for plan outputs (serialized as JSON string)
    pub policies: HashMap<String, String>, // Execution policies (serialized as JSON strings)
    pub capabilities_required: Vec<String>, // Capabilities this plan depends on
    pub annotations: HashMap<String, String>, // Provenance and metadata (serialized as JSON strings)
}

impl Archivable for ArchivablePlan {
    fn entity_id(&self) -> String {
        self.plan_id.clone()
    }
    fn entity_type(&self) -> &'static str {
        "ArchivablePlan"
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ArchivablePlanLanguage {
    Rtfs20,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchivablePlanBody {
    pub steps: Vec<String>, // RTFS expressions as strings
    pub preconditions: Vec<String>,
    pub postconditions: Vec<String>,
}

impl From<&PlanLanguage> for ArchivablePlanLanguage {
    fn from(lang: &PlanLanguage) -> Self {
        match lang {
            PlanLanguage::Rtfs20 => ArchivablePlanLanguage::Rtfs20,
            PlanLanguage::Wasm => ArchivablePlanLanguage::Rtfs20, // Default to Rtfs20 for unsupported types
            PlanLanguage::Python => ArchivablePlanLanguage::Rtfs20,
            PlanLanguage::GraphJson => ArchivablePlanLanguage::Rtfs20,
            PlanLanguage::Other(_) => ArchivablePlanLanguage::Rtfs20,
        }
    }
}

impl From<&Plan> for ArchivablePlan {
    fn from(plan: &Plan) -> Self {
        Self {
            plan_id: plan.plan_id.clone(),
            name: plan.name.clone(),
            intent_ids: plan.intent_ids.clone(),
            language: (&plan.language).into(),
            body: ArchivablePlanBody {
                steps: match &plan.body {
                    PlanBody::Rtfs(rtfs_code) => {
                        vec![rtfs_code.clone()] // Store RTFS as string
                    }
                    PlanBody::Wasm(_) => {
                        vec!["<WASM bytecode>".to_string()] // Placeholder for WASM
                    }
                },
                preconditions: vec![], // Plan doesn't have preconditions in current structure
                postconditions: vec![], // Plan doesn't have postconditions in current structure
            },
            status: plan.status.clone(),
            created_at: plan.created_at,
            metadata: plan
                .metadata
                .iter()
                .map(|(k, v)| (k.clone(), format!("{:?}", v)))
                .collect(),

            // Serialize new fields as JSON strings
            input_schema: plan
                .input_schema
                .as_ref()
                .map(|v| serde_json::to_string(v).unwrap_or_default()),
            output_schema: plan
                .output_schema
                .as_ref()
                .map(|v| serde_json::to_string(v).unwrap_or_default()),
            policies: plan
                .policies
                .iter()
                .map(|(k, v)| (k.clone(), serde_json::to_string(v).unwrap_or_default()))
                .collect(),
            capabilities_required: plan.capabilities_required.clone(),
            annotations: plan
                .annotations
                .iter()
                .map(|(k, v)| (k.clone(), serde_json::to_string(v).unwrap_or_default()))
                .collect(),
        }
    }
}

/// Add Archivable implementation for existing StorableIntent
/// StorableIntent is already the official archivable version of Intent
impl Archivable for StorableIntent {
    fn entity_id(&self) -> String {
        self.intent_id.clone()
    }
    fn entity_type(&self) -> &'static str {
        "StorableIntent"
    }
}

/// Serializable version of Action for archiving
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchivableAction {
    pub action_id: String,
    pub parent_action_id: Option<String>,
    pub plan_id: String,
    pub intent_id: String,
    pub action_type: String, // Simplified as string
    pub status: String,      // Simplified as string
    pub created_at: u64,
    pub metadata: HashMap<String, String>,
}

impl Archivable for ArchivableAction {
    fn entity_id(&self) -> String {
        self.action_id.clone()
    }
    fn entity_type(&self) -> &'static str {
        "ArchivableAction"
    }
}

impl From<&Action> for ArchivableAction {
    fn from(action: &Action) -> Self {
        // Derive a simple status from the optional execution result
        let status = match &action.result {
            Some(res) => {
                if res.success {
                    "Success".to_string()
                } else {
                    "Failure".to_string()
                }
            }
            None => "Pending".to_string(),
        };

        Self {
            action_id: action.action_id.clone(),
            parent_action_id: action.parent_action_id.clone(),
            plan_id: action.plan_id.clone(),
            intent_id: action.intent_id.clone(),
            action_type: format!("{:?}", action.action_type),
            status,
            created_at: action.timestamp,
            metadata: action
                .metadata
                .iter()
                .map(|(k, v)| (k.clone(), format!("{:?}", v)))
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ccos::storage::InMemoryArchive;

    #[test]
    fn test_unified_archivable_storage() {
        // Demonstrate that all CCOS entities can be stored in unified architecture
        let _plan_archive = InMemoryArchive::<ArchivablePlan>::new();
        let _intent_archive = InMemoryArchive::<StorableIntent>::new(); // Uses existing StorableIntent
        let _action_archive = InMemoryArchive::<ArchivableAction>::new();

        // All implement the same Archivable trait for consistent storage interface
        let plan = ArchivablePlan {
            plan_id: "test-plan".to_string(),
            name: Some("Test Plan".to_string()),
            intent_ids: vec![],
            language: ArchivablePlanLanguage::Rtfs20,
            body: ArchivablePlanBody {
                steps: vec![],
                preconditions: vec![],
                postconditions: vec![],
            },
            status: PlanStatus::Draft,
            created_at: 0,
            metadata: HashMap::new(),
            input_schema: None,
            output_schema: None,
            policies: HashMap::new(),
            capabilities_required: vec![],
            annotations: HashMap::new(),
        };

        assert_eq!(plan.entity_id(), "test-plan");
        assert_eq!(plan.entity_type(), "ArchivablePlan");
    }
}
