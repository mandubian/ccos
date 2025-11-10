//! Extract capability needs from plans and orchestrator RTFS

use crate::types::Plan;
use rtfs::ast::TypeExpr;
use rtfs::runtime::values::Value;
use serde_json::Value as JsonValue;

/// Represents a needed capability that may not yet exist
#[derive(Debug, Clone, PartialEq)]
pub struct CapabilityNeed {
    /// The capability class/type being requested (e.g., "restaurant.reservation.book")
    pub capability_class: String,
    /// Inputs that this capability should accept
    pub required_inputs: Vec<String>,
    /// Outputs that this capability should produce
    pub expected_outputs: Vec<String>,
    /// Rationale for why this capability is needed
    pub rationale: String,
    /// Optional structured annotations (e.g. primitive hints) describing the need.
    pub annotations: serde_json::Value,
    /// Optional input schema associated with this need.
    pub input_schema: Option<TypeExpr>,
    /// Optional output schema associated with this need.
    pub output_schema: Option<TypeExpr>,
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
            annotations: serde_json::Value::Null,
            input_schema: None,
            output_schema: None,
        }
    }

    /// Attach structured annotations (overwriting previous annotations).
    pub fn with_annotations(mut self, annotations: serde_json::Value) -> Self {
        self.annotations = annotations;
        self
    }

    /// Attach explicit input/output schemas to this need.
    pub fn with_schemas(mut self, input: Option<TypeExpr>, output: Option<TypeExpr>) -> Self {
        self.input_schema = input;
        self.output_schema = output;
        self
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
                        // Try to extract a better rationale from plan metadata
                        let rationale =
                            Self::extract_rationale_from_plan_metadata(&map, &capability_class)
                                .unwrap_or_else(|| {
                                    format!("Capability needed: {}", capability_class)
                                });

                        let annotations = map_get(&map, "primitive_annotations")
                            .and_then(|value| serde_json::to_value(value).ok())
                            .unwrap_or(JsonValue::Null);

                        let input_schema = map_get(&map, "input_schema")
                            .and_then(value_to_string)
                            .and_then(|schema| TypeExpr::from_str(&schema).ok());

                        let output_schema = map_get(&map, "output_schema")
                            .and_then(value_to_string)
                            .and_then(|schema| TypeExpr::from_str(&schema).ok());

                        needs.push(
                            CapabilityNeed::new(
                                capability_class,
                                required_inputs,
                                expected_outputs,
                                rationale,
                            )
                            .with_annotations(annotations)
                            .with_schemas(input_schema, output_schema),
                        );
                    }
                }
            }
        }

        // If no metadata found, try extracting from RTFS plan body
        if needs.is_empty() {
            if let crate::types::PlanBody::Rtfs(rtfs) = &plan.body {
                needs = Self::extract_from_orchestrator(rtfs);
            }
        }

        needs
    }

    /// Extract needs from RTFS orchestrator code
    /// Parses RTFS to find `(call :capability.id {...})` patterns
    pub fn extract_from_orchestrator(rtfs: &str) -> Vec<CapabilityNeed> {
        let mut needs = Vec::new();
        let bytes = rtfs.as_bytes();
        let mut i = 0;

        // Find all (call :capability.id patterns
        while i < bytes.len() {
            // Look for "(call "
            if i + 6 <= bytes.len() && &rtfs[i..i + 6] == "(call " {
                let mut pos = i + 6;
                // Skip whitespace
                while pos < bytes.len() && (bytes[pos] as char).is_whitespace() {
                    pos += 1;
                }

                // Check for keyword starting with :
                if pos < bytes.len() && bytes[pos] == b':' {
                    pos += 1;
                    let start = pos;

                    // Extract capability ID (alphanumeric, dots, underscores, hyphens)
                    while pos < bytes.len() {
                        let ch = bytes[pos] as char;
                        if ch.is_alphanumeric() || ch == '.' || ch == '_' || ch == '-' {
                            pos += 1;
                        } else {
                            break;
                        }
                    }

                    if pos > start {
                        let capability_id = rtfs[start..pos].to_string();

                        // Find the argument map for this call
                        if let Some(arg_map) = extract_map_from_position(rtfs, pos) {
                            // Extract keys from the map as potential inputs
                            let inputs = extract_map_keys(&arg_map);

                            // Create a need
                            // Generate a functional rationale from capability ID
                            let rationale =
                                Self::capability_id_to_functional_description(&capability_id);

                            needs.push(
                                CapabilityNeed::new(
                                    capability_id.clone(),
                                    inputs,
                                    vec!["result".to_string()], // Default output
                                    rationale,
                                )
                                .with_annotations(JsonValue::Null),
                            );
                        } else {
                            // No arg map found, still create a need with empty inputs
                            // Generate a functional rationale from capability ID
                            let rationale =
                                Self::capability_id_to_functional_description(&capability_id);

                            needs.push(
                                CapabilityNeed::new(
                                    capability_id.clone(),
                                    vec![],
                                    vec!["result".to_string()],
                                    rationale,
                                )
                                .with_annotations(JsonValue::Null),
                            );
                        }
                    }
                }
            }
            i += 1;
        }

        needs
    }

    /// Extract a rationale from plan metadata if available
    fn extract_rationale_from_plan_metadata(
        map: &std::collections::HashMap<rtfs::ast::MapKey, Value>,
        _capability_class: &str,
    ) -> Option<String> {
        // Try to find description or name fields
        if let Some(Value::String(desc)) = map_get(map, "description") {
            return Some(desc.clone());
        }
        if let Some(Value::String(name)) = map_get(map, "name") {
            // Convert step name to functional description
            return Some(Self::capability_id_to_functional_description(name));
        }
        None
    }

    /// Convert a capability ID or name to a functional description for better semantic matching
    fn capability_id_to_functional_description(capability_id: &str) -> String {
        // If it's already functional (contains verbs), return as-is or enhance
        let lower = capability_id.to_lowercase();
        let functional_verbs = [
            "list", "get", "retrieve", "fetch", "search", "find", "create", "update", "delete",
        ];

        // Handle common patterns
        if lower.contains("list") && lower.contains("issue") {
            if lower.contains("github") {
                return "List issues in a GitHub repository".to_string();
            }
            return "List issues".to_string();
        }

        if lower.contains("list") && lower.contains("pull") {
            if lower.contains("github") {
                return "List pull requests in a GitHub repository".to_string();
            }
            return "List pull requests".to_string();
        }

        // Parse capability ID parts (e.g., "github.issues.list" -> "List issues in a GitHub repository")
        let parts: Vec<&str> = capability_id.split('.').collect();
        if parts.len() >= 2 {
            if let Some(action) = parts.last() {
                match *action {
                    "list" if parts.len() >= 3 => {
                        let domain = parts[0];
                        let resource = parts[parts.len() - 2];
                        return format!("List {} in a {} repository", resource, domain);
                    }
                    "get" | "retrieve" => {
                        return format!("Retrieve {}", capability_id);
                    }
                    "search" => {
                        return format!("Search for {}", capability_id);
                    }
                    _ => {}
                }
            }
        }

        // Fallback: construct a functional description from the ID
        if functional_verbs.iter().any(|verb| lower.contains(verb)) {
            capability_id.to_string()
        } else {
            format!("Execute capability: {}", capability_id)
        }
    }
}

/// Helper function to get a value from a map (handles both string and keyword keys)
fn map_get<'a>(
    map: &'a std::collections::HashMap<rtfs::ast::MapKey, Value>,
    key: &str,
) -> Option<&'a Value> {
    use rtfs::ast::{Keyword, MapKey};
    map.get(&MapKey::String(key.to_string()))
        .or_else(|| map.get(&MapKey::Keyword(Keyword(key.to_string()))))
}

// Helper functions for RTFS parsing

/// Extract a balanced map `{ ... }` starting from a position in the string
fn extract_map_from_position(text: &str, start_pos: usize) -> Option<String> {
    let bytes = text.as_bytes();
    let mut pos = start_pos;

    // Skip whitespace to find opening brace
    while pos < bytes.len() && (bytes[pos] as char).is_whitespace() {
        pos += 1;
    }

    if pos >= bytes.len() || (bytes[pos] as char) != '{' {
        return None;
    }

    // Extract balanced braces
    let mut depth = 0;
    let start = pos;
    let mut in_string = false;
    let mut escape = false;

    while pos < bytes.len() {
        let ch = bytes[pos] as char;

        if escape {
            escape = false;
            pos += 1;
            continue;
        }

        match ch {
            '\\' => {
                escape = true;
                pos += 1;
            }
            '"' => {
                in_string = !in_string;
                pos += 1;
            }
            '{' if !in_string => {
                depth += 1;
                pos += 1;
            }
            '}' if !in_string => {
                depth -= 1;
                pos += 1;
                if depth == 0 {
                    return Some(text[start..pos].to_string());
                }
            }
            _ => pos += 1,
        }
    }

    None
}

/// Extract keys from an RTFS map string like `{ :key1 val1 :key2 val2 }`
fn extract_map_keys(map_str: &str) -> Vec<String> {
    let mut keys = Vec::new();
    let bytes = map_str.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        // Look for :keyword pattern
        if bytes[i] == b':' {
            i += 1;
            let start = i;

            // Extract keyword (letters, numbers, underscores, hyphens)
            while i < bytes.len() {
                let ch = bytes[i] as char;
                if ch.is_alphanumeric() || ch == '_' || ch == '-' {
                    i += 1;
                } else {
                    break;
                }
            }

            if i > start {
                keys.push(map_str[start..i].to_string());
            }
        } else {
            i += 1;
        }
    }

    keys
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
        assert_eq!(needs[0].required_inputs, vec!["origin", "destination"]);
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
