use super::rtfs_bridge::extractors::expression_to_rtfs_string;
use super::storage::Archivable;
use super::types::{Action, Plan, PlanBody, PlanLanguage, PlanStatus, StorableIntent};
use rtfs::utils::format_rtfs_value_pretty;
/// Serializable versions of CCOS types for unified storage architecture
///
/// This module provides archivable versions of CCOS entities that implement
/// the Archivable trait for content-addressable storage.
///
/// Note: For Intents, we reuse the existing StorableIntent from types.rs
/// rather than creating a duplicate ArchivableIntent, since StorableIntent
/// already serves as the official archivable version with full context.
use serde::{Deserialize, Serialize};
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
#[serde(untagged)]
pub enum ArchivablePlanBody {
    // New format: single RTFS string (preferred)
    String(String),
    // Legacy format: steps array (for backward compatibility)
    Legacy {
        steps: Vec<String>,
        preconditions: Vec<String>,
        postconditions: Vec<String>,
    },
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
            body: match &plan.body {
                PlanBody::Rtfs(rtfs_code) => {
                    // Extract just the body if it's wrapped in a (plan ...) form
                    let body_code = if rtfs_code.trim_start().starts_with("(plan") {
                        // Try to parse as top-level construct to extract :body from (plan ...) form
                        match rtfs::parser::parse(rtfs_code) {
                            Ok(top_levels) => {
                                // Look for a Plan top-level construct
                                if let Some(rtfs::ast::TopLevel::Plan(plan_def)) =
                                    top_levels.first()
                                {
                                    // Find the :body property in the plan definition
                                    // Keywords are stored without the : prefix, so :body becomes "body"
                                    if let Some(body_prop) =
                                        plan_def.properties.iter().find(|p| p.key.0 == "body")
                                    {
                                        // Format the body expression as RTFS string
                                        expression_to_rtfs_string(&body_prop.value)
                                    } else {
                                        eprintln!("⚠️  Warning: Plan has (plan ...) form but no :body property found. Available properties: {:?}", 
                                            plan_def.properties.iter().map(|p| &p.key.0).collect::<Vec<_>>());
                                        rtfs_code.clone() // No :body property found, use as-is
                                    }
                                } else {
                                    eprintln!(
                                        "⚠️  Warning: Parsed top-level is not a Plan: {:?}",
                                        top_levels.first()
                                    );
                                    rtfs_code.clone() // Not a Plan top-level, use as-is
                                }
                            }
                            Err(e) => {
                                eprintln!("⚠️  Warning: Failed to parse (plan ...) form: {:?}", e);
                                rtfs_code.clone() // Parse failed, use as-is
                            }
                        }
                    } else {
                        rtfs_code.clone() // Not a (plan ...) form, use as-is
                    };
                    ArchivablePlanBody::String(body_code)
                }
                PlanBody::Wasm(_) => ArchivablePlanBody::String("<WASM bytecode>".to_string()),
            },
            status: plan.status.clone(),
            created_at: plan.created_at,
            metadata: plan
                .metadata
                .iter()
                .map(|(k, v)| (k.clone(), format_rtfs_value_pretty(v)))
                .collect(),

            // Serialize new fields as RTFS pretty-printed strings
            input_schema: plan
                .input_schema
                .as_ref()
                .map(|v| format_rtfs_value_pretty(v)),
            output_schema: plan
                .output_schema
                .as_ref()
                .map(|v| format_rtfs_value_pretty(v)),
            policies: plan
                .policies
                .iter()
                .map(|(k, v)| (k.clone(), format_rtfs_value_pretty(v)))
                .collect(),
            capabilities_required: plan.capabilities_required.clone(),
            annotations: plan
                .annotations
                .iter()
                .map(|(k, v)| (k.clone(), format_rtfs_value_pretty(v)))
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
                .map(|(k, v)| (k.clone(), format_rtfs_value_pretty(v)))
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::InMemoryArchive;

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
            body: ArchivablePlanBody::String("()".to_string()),
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
