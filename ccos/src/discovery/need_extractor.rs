//! Extract capability needs from plans and orchestrator RTFS

use crate::types::Plan;
use rtfs::runtime::values::Value;

/// Represents a needed capability that may not yet exist
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityNeed {
    /// The capability class/type being requested (e.g., "restaurant.reservation.book")
    pub capability_class: String,
    /// Inputs that this capability should accept
    pub required_inputs: Vec<String>,
    /// Outputs that this capability should produce
    pub expected_outputs: Vec<String>,
    /// Rationale for why this capability is needed
    pub rationale: String,
}

impl CapabilityNeed {
    /// Create a new capability need
    pub fn new(
        capability_class: String,
        required_inputs: Vec<String>,
        expected_outputs: Vec<String>,
        rationale: String,
    ) -> Self {
        Self {
            capability_class,
            required_inputs,
            expected_outputs,
            rationale,
        }
    }
}

/// Extracts capability needs from plans and orchestrator RTFS
pub struct CapabilityNeedExtractor;

impl CapabilityNeedExtractor {
    /// Extract needs from a plan's metadata
    pub fn extract_from_plan(plan: &Plan) -> Vec<CapabilityNeed> {
        let mut needs = Vec::new();
        
        // Check if plan has needs_capabilities metadata
        if let Some(Value::Vector(entries)) = plan.metadata.get("needs_capabilities") {
            for entry in entries {
                if let Value::Map(map) = entry {
                    let capability_class = map
                        .iter()
                        .find(|(k, _)| matches!(k, rtfs::ast::MapKey::String(s) if s == "class"))
                        .and_then(|(_, v)| value_to_string(v))
                        .unwrap_or_default();
                    
                    let required_inputs = map
                        .iter()
                        .find(|(k, _)| {
                            matches!(k, rtfs::ast::MapKey::String(s) if s == "required_inputs")
                        })
                        .and_then(|(_, v)| value_to_string_vec(v))
                        .unwrap_or_default();
                    
                    let expected_outputs = map
                        .iter()
                        .find(|(k, _)| {
                            matches!(k, rtfs::ast::MapKey::String(s) if s == "expected_outputs")
                        })
                        .and_then(|(_, v)| value_to_string_vec(v))
                        .unwrap_or_default();
                    
                    if !capability_class.is_empty() && !required_inputs.is_empty() {
                        needs.push(CapabilityNeed::new(
                            capability_class,
                            required_inputs,
                            expected_outputs,
                            format!("Extracted from plan {}", plan.plan_id),
                        ));
                    }
                }
            }
        }
        
        needs
    }
    
    /// Extract needs from RTFS orchestrator code
    pub fn extract_from_orchestrator(rtfs: &str) -> Vec<CapabilityNeed> {
        // TODO: Parse RTFS and extract capability calls
        // For now, return empty vector
        vec![]
    }
}

// Helper functions to convert Value to String and Vec<String>

fn value_to_string(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Keyword(k) => Some(k.0.clone()),
        _ => None,
    }
}

fn value_to_string_vec(value: &Value) -> Option<Vec<String>> {
    match value {
        Value::Vector(items) | Value::List(items) => {
            let mut out = Vec::new();
            for item in items {
                if let Some(s) = value_to_string(item) {
                    out.push(s);
                }
            }
            Some(out)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Plan;
    use std::collections::HashMap;

    #[test]
    fn test_extract_from_plan_with_needs() {
        let mut plan = Plan::new_rtfs("(do)".to_string(), vec![]);
        
        // Create metadata with needs_capabilities
        let mut metadata = HashMap::new();
        let mut entry = HashMap::new();
        entry.insert(
            rtfs::ast::MapKey::String("class".to_string()),
            Value::String("travel.flights.search".to_string()),
        );
        entry.insert(
            rtfs::ast::MapKey::String("required_inputs".to_string()),
            Value::Vector(vec![
                Value::String("origin".to_string()),
                Value::String("destination".to_string()),
            ]),
        );
        entry.insert(
            rtfs::ast::MapKey::String("expected_outputs".to_string()),
            Value::Vector(vec![Value::String("flight_options".to_string())]),
        );
        metadata.insert(
            "needs_capabilities".to_string(),
            Value::Vector(vec![Value::Map(entry)]),
        );
        
        plan.metadata = metadata;
        
        let needs = CapabilityNeedExtractor::extract_from_plan(&plan);
        
        assert_eq!(needs.len(), 1);
        assert_eq!(needs[0].capability_class, "travel.flights.search");
        assert_eq!(
            needs[0].required_inputs,
            vec!["origin", "destination"]
        );
        assert_eq!(needs[0].expected_outputs, vec!["flight_options"]);
    }
    
    #[test]
    fn test_extract_from_plan_empty() {
        let plan = Plan::new_rtfs("(do)".to_string(), vec![]);
        let needs = CapabilityNeedExtractor::extract_from_plan(&plan);
        assert_eq!(needs.len(), 0);
    }
    
    #[test]
    fn test_capability_need_new() {
        let need = CapabilityNeed::new(
            "test.cap".to_string(),
            vec!["input1".to_string()],
            vec!["output1".to_string()],
            "Test rationale".to_string(),
        );
        
        assert_eq!(need.capability_class, "test.cap");
        assert_eq!(need.required_inputs, vec!["input1"]);
        assert_eq!(need.expected_outputs, vec!["output1"]);
        assert_eq!(need.rationale, "Test rationale");
    }
}

