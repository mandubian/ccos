//! Local RTFS synthesizer for simple operations
//!
//! This module detects simple data transformation operations (filter, map, format, display, etc.)
//! and synthesizes them as local RTFS implementations using stdlib functions rather than
//! marking them as incomplete or requiring external services.

use crate::capability_marketplace::types::{CapabilityManifest, LocalCapability};
use crate::discovery::need_extractor::CapabilityNeed;
use crate::synthesis::primitives::{
    PrimitiveContext, PrimitiveRegistry, RestrictedRtfsExecutor, SynthesizedPrimitive,
};
use rtfs::ast::{Keyword, MapTypeEntry, TypeExpr};
use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use rtfs::runtime::values::Value;
use serde_json::{json, Value as JsonValue};
use std::sync::Arc;

/// Detects if a capability represents a simple local operation that can be synthesized
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SimpleOperation {
    Filter,
    Map,
    Project,
    Format,
    Display,
    Sort,
    Reduce,
    GroupBy,
    Join,
    Transform,
    Unknown,
}

/// Analyzes a capability need to determine if it's a simple local operation
pub struct LocalSynthesizer;

impl LocalSynthesizer {
    /// Detect if this capability need represents a simple operation
    pub fn detect_simple_operation(need: &CapabilityNeed) -> SimpleOperation {
        let class_lower = need.capability_class.to_lowercase();
        let rationale_lower = need.rationale.to_lowercase();

        // Check class name patterns
        if class_lower.contains("filter") || class_lower.contains(".filter") {
            return SimpleOperation::Filter;
        }
        if class_lower.contains("map") || class_lower.contains(".map") {
            return SimpleOperation::Map;
        }
        if class_lower.contains("project") || class_lower.contains(".project") {
            return SimpleOperation::Project;
        }
        if class_lower.contains("format") || class_lower.contains(".format") {
            return SimpleOperation::Format;
        }
        if class_lower.contains("display")
            || class_lower.contains(".display")
            || class_lower.contains("show")
            || class_lower.contains(".show")
            || class_lower.contains("print")
            || class_lower.contains(".print")
        {
            return SimpleOperation::Display;
        }
        if class_lower.contains("sort") || class_lower.contains(".sort") {
            return SimpleOperation::Sort;
        }
        if class_lower.contains("reduce") || class_lower.contains(".reduce") {
            return SimpleOperation::Reduce;
        }
        if class_lower.contains("group") && class_lower.contains("by") {
            return SimpleOperation::GroupBy;
        }
        if class_lower.contains("join") || class_lower.contains(".join") {
            return SimpleOperation::Join;
        }
        if class_lower.contains("transform") || class_lower.contains(".transform") {
            return SimpleOperation::Transform;
        }

        // Check rationale for operation hints
        if rationale_lower.contains("filter")
            && (rationale_lower.contains("by")
                || rationale_lower.contains("topic")
                || rationale_lower.contains("keyword"))
        {
            return SimpleOperation::Filter;
        }
        if rationale_lower.contains("project") || rationale_lower.contains("select") {
            return SimpleOperation::Project;
        }
        if rationale_lower.contains("display")
            || rationale_lower.contains("present")
            || rationale_lower.contains("show")
            || rationale_lower.contains("list")
        {
            return SimpleOperation::Display;
        }
        if rationale_lower.contains("format") || rationale_lower.contains("convert") {
            return SimpleOperation::Format;
        }
        if rationale_lower.contains("group by") {
            return SimpleOperation::GroupBy;
        }
        if rationale_lower.contains("join") {
            return SimpleOperation::Join;
        }

        SimpleOperation::Unknown
    }

    /// Check if a capability can be synthesized locally
    pub fn can_synthesize_locally(need: &CapabilityNeed) -> bool {
        Self::detect_simple_operation(need) != SimpleOperation::Unknown
    }

    /// Infer default primitive annotations for a capability need when we can recognize
    /// the operation pattern.
    pub fn infer_primitive_annotations(need: &CapabilityNeed) -> Option<serde_json::Value> {
        let operation = Self::detect_simple_operation(need);
        match operation {
            SimpleOperation::Filter
            | SimpleOperation::Map
            | SimpleOperation::Project
            | SimpleOperation::Sort
            | SimpleOperation::Reduce
            | SimpleOperation::GroupBy
            | SimpleOperation::Join => Some(Self::default_annotations_for(operation, need)),
            _ => None,
        }
    }

    /// Synthesize a simple operation as a local RTFS capability
    pub fn synthesize_locally(need: &CapabilityNeed) -> RuntimeResult<CapabilityManifest> {
        let operation = Self::detect_simple_operation(need);

        match operation {
            SimpleOperation::Filter
            | SimpleOperation::Map
            | SimpleOperation::Project
            | SimpleOperation::Sort
            | SimpleOperation::Reduce
            | SimpleOperation::GroupBy
            | SimpleOperation::Join => Self::synthesize_with_primitives(need, operation),
            SimpleOperation::Display => Self::synthesize_display(need),
            SimpleOperation::Format => Self::synthesize_format(need),
            SimpleOperation::Transform => Self::synthesize_transform(need),
            SimpleOperation::Unknown => Err(RuntimeError::Generic(format!(
                "Cannot synthesize unknown operation: {}",
                need.capability_class
            ))),
        }
    }

    fn synthesize_with_primitives(
        need: &CapabilityNeed,
        operation: SimpleOperation,
    ) -> RuntimeResult<CapabilityManifest> {
        let registry = PrimitiveRegistry::new();

        let defaults = Self::default_annotations_for(operation, need);
        let merged_annotations = Self::merge_annotations(defaults, &need.annotations);

        let input_schema_expr = need
            .input_schema
            .clone()
            .or_else(|| Self::create_input_schema(&need.required_inputs));
        let output_schema_expr = need
            .output_schema
            .clone()
            .or_else(|| Self::create_output_schema(&need.expected_outputs));

        let ctx = PrimitiveContext::from_type_schemas(
            need,
            input_schema_expr.as_ref(),
            output_schema_expr.as_ref(),
            merged_annotations.clone(),
        );

        let synthesized = registry.synthesize(&ctx).map_err(|err| {
            RuntimeError::Generic(format!(
                "Primitive synthesis failed for '{}': {}",
                need.capability_class, err
            ))
        })?;

        Self::manifest_from_synthesized(need, synthesized, merged_annotations)
    }

    fn default_annotations_for(operation: SimpleOperation, need: &CapabilityNeed) -> JsonValue {
        match operation {
            SimpleOperation::Filter => {
                let collection_input =
                    Self::guess_collection_input(need).unwrap_or_else(|| "items".to_string());
                let search_input = Self::guess_search_input(need, &collection_input)
                    .unwrap_or_else(|| {
                        if collection_input == "query" {
                            "search".to_string()
                        } else {
                            "query".to_string()
                        }
                    });
                let output_key =
                    Self::guess_output_key(need).unwrap_or_else(|| "filtered".to_string());
                json!({
                    "primitive": {
                        "kind": "filter",
                        "collection_input": collection_input,
                        "search_input": search_input,
                        "output_key": output_key,
                        "search_fields": []
                    }
                })
            }
            SimpleOperation::Map => {
                let collection_input =
                    Self::guess_collection_input(need).unwrap_or_else(|| "items".to_string());
                let output_key =
                    Self::guess_output_key(need).unwrap_or_else(|| "mapped".to_string());
                json!({
                    "primitive": {
                        "kind": "map",
                        "collection_input": collection_input,
                        "output_key": output_key
                    }
                })
            }
            SimpleOperation::Project => {
                let collection_input =
                    Self::guess_collection_input(need).unwrap_or_else(|| "items".to_string());
                let output_key =
                    Self::guess_output_key(need).unwrap_or_else(|| "projected".to_string());
                json!({
                    "primitive": {
                        "kind": "project",
                        "collection_input": collection_input,
                        "output_key": output_key
                    }
                })
            }
            SimpleOperation::Sort => {
                let collection_input =
                    Self::guess_collection_input(need).unwrap_or_else(|| "items".to_string());
                let output_key =
                    Self::guess_output_key(need).unwrap_or_else(|| "sorted".to_string());
                json!({
                    "primitive": {
                        "kind": "sort",
                        "collection_input": collection_input,
                        "output_key": output_key,
                        "order": ":asc"
                    }
                })
            }
            SimpleOperation::Reduce => {
                let collection_input =
                    Self::guess_collection_input(need).unwrap_or_else(|| "items".to_string());
                let output_key =
                    Self::guess_output_key(need).unwrap_or_else(|| "reduced_value".to_string());
                json!({
                    "primitive": {
                        "kind": "reduce",
                        "collection_input": collection_input,
                        "output_key": output_key,
                        "reducer": {
                            "fn": "+",
                            "initial": "0",
                            "item_field": JsonValue::Null,
                            "item_default": "0"
                        }
                    }
                })
            }
            SimpleOperation::GroupBy => {
                let collection_input =
                    Self::guess_collection_input(need).unwrap_or_else(|| "items".to_string());
                let output_key =
                    Self::guess_output_key(need).unwrap_or_else(|| "grouped".to_string());
                json!({
                    "primitive": {
                        "kind": "groupBy",
                        "collection_input": collection_input,
                        "output_key": output_key
                    }
                })
            }
            SimpleOperation::Join => {
                let left_input = need
                    .required_inputs
                    .get(0)
                    .cloned()
                    .unwrap_or_else(|| "left".to_string());
                let right_input = need
                    .required_inputs
                    .get(1)
                    .cloned()
                    .unwrap_or_else(|| "right".to_string());
                let output_key =
                    Self::guess_output_key(need).unwrap_or_else(|| "joined".to_string());
                json!({
                    "primitive": {
                        "kind": "join",
                        "left_input": left_input,
                        "right_input": right_input,
                        "output_key": output_key,
                        "type": ":inner"
                    }
                })
            }
            _ => json!({}),
        }
    }

    fn merge_annotations(defaults: JsonValue, overrides: &JsonValue) -> JsonValue {
        match (defaults, overrides) {
            (JsonValue::Object(mut default_map), JsonValue::Object(override_map)) => {
                for (key, override_value) in override_map {
                    let merged = if let Some(default_value) = default_map.remove(key) {
                        Self::merge_annotations(default_value, override_value)
                    } else {
                        override_value.clone()
                    };
                    default_map.insert(key.clone(), merged);
                }
                JsonValue::Object(default_map)
            }
            (default_value, JsonValue::Null) => default_value,
            (_, override_value) => override_value.clone(),
        }
    }

    fn manifest_from_synthesized(
        need: &CapabilityNeed,
        synthesized: SynthesizedPrimitive,
        annotations: JsonValue,
    ) -> RuntimeResult<CapabilityManifest> {
        let rtfs_code = synthesized.rtfs_code.clone();
        let primitive_kind = synthesized.primitive_id.as_str().to_string();
        let primitive_metadata =
            serde_json::to_string(&synthesized.metadata).unwrap_or_else(|_| "{}".to_string());
        let annotations_str =
            serde_json::to_string(&annotations).unwrap_or_else(|_| "{}".to_string());

        let handler: Arc<dyn Fn(&Value) -> RuntimeResult<Value> + Send + Sync> =
            Arc::new(move |input: &Value| {
                let executor = RestrictedRtfsExecutor::new();
                executor.evaluate(&rtfs_code, input.clone())
            });

        let mut manifest = CapabilityManifest::new(
            synthesized.capability_id.clone(),
            format!(
                "Synthesized {} primitive: {}",
                primitive_kind, need.capability_class
            ),
            need.rationale.clone(),
            crate::capability_marketplace::types::ProviderType::Local(LocalCapability { handler }),
            "1.0.0".to_string(),
        );

        manifest.input_schema = Some(synthesized.input_schema.clone());
        manifest.output_schema = Some(synthesized.output_schema.clone());
        manifest.metadata.insert(
            "synthesis_method".to_string(),
            "primitive_registry".to_string(),
        );
        manifest
            .metadata
            .insert("primitive_kind".to_string(), primitive_kind);
        manifest.metadata.insert(
            "rtfs_implementation".to_string(),
            synthesized.rtfs_code.clone(),
        );
        manifest
            .metadata
            .insert("primitive_metadata".to_string(), primitive_metadata);
        manifest
            .metadata
            .insert("primitive_annotations".to_string(), annotations_str);
        manifest.metadata.insert(
            "input_schema_compact".to_string(),
            synthesized.input_schema.to_string(),
        );
        manifest.metadata.insert(
            "output_schema_compact".to_string(),
            synthesized.output_schema.to_string(),
        );

        Ok(manifest)
    }

    fn guess_collection_input(need: &CapabilityNeed) -> Option<String> {
        let preferred = [
            "list",
            "items",
            "collection",
            "issues",
            "data",
            "records",
            "entries",
        ];
        need.required_inputs
            .iter()
            .find(|input| preferred.iter().any(|needle| input.contains(needle)))
            .cloned()
            .or_else(|| need.required_inputs.first().cloned())
    }

    fn guess_search_input(need: &CapabilityNeed, collection_input: &str) -> Option<String> {
        let preferred = ["language", "keyword", "topic", "filter", "search", "query"];
        let mut candidate = need
            .required_inputs
            .iter()
            .find(|input| preferred.iter().any(|needle| input.contains(needle)))
            .cloned();

        if candidate
            .as_ref()
            .map(|value| value == collection_input)
            .unwrap_or(false)
        {
            candidate = None;
        }

        candidate.or_else(|| {
            need.required_inputs
                .iter()
                .find(|input| *input != collection_input)
                .cloned()
        })
    }

    fn guess_output_key(need: &CapabilityNeed) -> Option<String> {
        need.expected_outputs.first().cloned()
    }

    /// Synthesize a display operation
    fn synthesize_display(need: &CapabilityNeed) -> RuntimeResult<CapabilityManifest> {
        let input = need
            .required_inputs
            .first()
            .cloned()
            .unwrap_or_else(|| "items".to_string());

        let output = need
            .expected_outputs
            .first()
            .cloned()
            .unwrap_or_else(|| "displayed".to_string());

        let rtfs_code = format!(
            r#"(fn [input]
  (let [
    {} (get input :{})
    formatted-items (map
      (fn [item]
        (let [
          title (str (get item :title ""))
          body (str (get item :body ""))
        ]
          (str "• " title " - " body)
        )
      )
      {}
    )
    result (reduce
      (fn [acc item-str]
        (if (empty? acc)
          item-str
          (str acc "\n" item-str)
        )
      )
      ""
      formatted-items
    )
  ]
    (log result)
    {{:{} result}})
)"#,
            input, input, input, output
        );

        // Store RTFS code in metadata first (before moving into closure)
        let mut metadata = std::collections::HashMap::new();
        let rtfs_code_for_metadata = rtfs_code.clone();
        metadata.insert("rtfs_implementation".to_string(), rtfs_code_for_metadata);

        let handler: Arc<dyn Fn(&Value) -> RuntimeResult<Value> + Send + Sync> =
            Arc::new(move |_input: &Value| {
                let mut result = std::collections::HashMap::new();
                result.insert(
                    rtfs::ast::MapKey::Keyword(Keyword(output.clone())),
                    Value::String(format!(
                        "[Synthesized display operation - requires RTFS execution: {}]",
                        rtfs_code
                    )),
                );
                Ok(Value::Map(result))
            });
        metadata.insert("synthesis_method".to_string(), "local_rtfs".to_string());
        metadata.insert("operation_type".to_string(), "display".to_string());

        let mut manifest = CapabilityManifest::new(
            need.capability_class.clone(),
            format!("Local display: {}", need.capability_class),
            format!("Synthesized local display operation: {}", need.rationale),
            crate::capability_marketplace::types::ProviderType::Local(LocalCapability { handler }),
            "1.0.0".to_string(),
        );
        manifest.metadata = metadata;

        // Set input/output schemas based on required inputs and expected outputs
        manifest.input_schema = Self::create_input_schema(&need.required_inputs);
        manifest.output_schema = Self::create_output_schema(&need.expected_outputs);

        Ok(manifest)
    }

    /// Synthesize a format operation
    fn synthesize_format(need: &CapabilityNeed) -> RuntimeResult<CapabilityManifest> {
        // Similar to display but focused on formatting
        Self::synthesize_display(need)
    }

    /// Synthesize a transform operation
    fn synthesize_transform(_need: &CapabilityNeed) -> RuntimeResult<CapabilityManifest> {
        Err(RuntimeError::Generic(
            "Transform synthesis not yet implemented".to_string(),
        ))
    }

    /// Create input schema from required inputs
    fn create_input_schema(required_inputs: &[String]) -> Option<TypeExpr> {
        if required_inputs.is_empty() {
            return None;
        }

        let entries: Vec<MapTypeEntry> = required_inputs
            .iter()
            .map(|input| MapTypeEntry {
                key: Keyword(input.clone()),
                value_type: Box::new(TypeExpr::Any),
                optional: false,
            })
            .collect();

        Some(TypeExpr::Map {
            entries,
            wildcard: None,
        })
    }

    /// Create output schema from expected outputs
    fn create_output_schema(expected_outputs: &[String]) -> Option<TypeExpr> {
        if expected_outputs.is_empty() {
            return None;
        }

        let entries: Vec<MapTypeEntry> = expected_outputs
            .iter()
            .map(|output| MapTypeEntry {
                key: Keyword(output.clone()),
                value_type: Box::new(TypeExpr::Any),
                optional: false,
            })
            .collect();

        Some(TypeExpr::Map {
            entries,
            wildcard: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability_marketplace::types::ProviderType;
    use rtfs::ast::{Keyword, MapKey};
    use std::collections::HashMap;

    #[test]
    fn test_detect_filter_operation() {
        let need = CapabilityNeed::new(
            "text.filter.by-topic".to_string(),
            vec!["issues".to_string(), "topic".to_string()],
            vec!["filtered_issues".to_string()],
            "Filter issues by topic".to_string(),
        );

        assert_eq!(
            LocalSynthesizer::detect_simple_operation(&need),
            SimpleOperation::Filter
        );
        assert!(LocalSynthesizer::can_synthesize_locally(&need));
    }

    #[test]
    fn test_detect_display_operation() {
        let need = CapabilityNeed::new(
            "ui.display.list".to_string(),
            vec!["items".to_string()],
            vec!["displayed".to_string()],
            "Display list of items".to_string(),
        );

        assert_eq!(
            LocalSynthesizer::detect_simple_operation(&need),
            SimpleOperation::Display
        );
        assert!(LocalSynthesizer::can_synthesize_locally(&need));
    }

    #[test]
    fn test_detect_unknown_operation() {
        let need = CapabilityNeed::new(
            "external.api.call".to_string(),
            vec!["input".to_string()],
            vec!["output".to_string()],
            "Call external API".to_string(),
        );

        assert_eq!(
            LocalSynthesizer::detect_simple_operation(&need),
            SimpleOperation::Unknown
        );
        assert!(!LocalSynthesizer::can_synthesize_locally(&need));
    }

    #[test]
    fn synthesize_filter_executes_through_restricted_runtime() {
        let need = CapabilityNeed::new(
            "text.filter.by-topic".to_string(),
            vec!["issues".to_string(), "topic".to_string()],
            vec!["filtered_issues".to_string()],
            "Filter issues by topic".to_string(),
        );

        let manifest =
            LocalSynthesizer::synthesize_locally(&need).expect("filter synthesis should succeed");

        assert_eq!(
            manifest.metadata.get("synthesis_method"),
            Some(&"primitive_registry".to_string())
        );
        assert_eq!(
            manifest.metadata.get("primitive_kind"),
            Some(&"filter".to_string())
        );

        let provider = match &manifest.provider {
            ProviderType::Local(local) => local,
            other => panic!("expected local provider, found {:?}", other),
        };

        let mut issue1 = HashMap::new();
        issue1.insert(
            MapKey::Keyword(Keyword("title".to_string())),
            Value::String("Rust issue".to_string()),
        );
        issue1.insert(
            MapKey::Keyword(Keyword("body".to_string())),
            Value::String("Help needed".to_string()),
        );

        let mut issue2 = HashMap::new();
        issue2.insert(
            MapKey::Keyword(Keyword("title".to_string())),
            Value::String("Python issue".to_string()),
        );
        issue2.insert(
            MapKey::Keyword(Keyword("body".to_string())),
            Value::String("Interpreter bug".to_string()),
        );

        let issues_value = Value::Vector(vec![Value::Map(issue1), Value::Map(issue2)]);

        let mut input_map = HashMap::new();
        input_map.insert(MapKey::Keyword(Keyword("issues".to_string())), issues_value);
        input_map.insert(
            MapKey::Keyword(Keyword("topic".to_string())),
            Value::String("rust".to_string()),
        );

        let result = (provider.handler)(&Value::Map(input_map))
            .expect("restricted runtime should execute filter");

        let output = match result {
            Value::Map(map) => map,
            other => panic!("expected map result, found {:?}", other),
        };

        let filtered = output
            .get(&MapKey::Keyword(Keyword("filtered_issues".to_string())))
            .expect("filtered output missing");

        let filtered_items = match filtered {
            Value::Vector(items) => items,
            other => panic!("expected vector result, found {:?}", other),
        };

        assert_eq!(filtered_items.len(), 1);
        let only_item = filtered_items
            .first()
            .expect("expected single filtered element");
        let only_map = match only_item {
            Value::Map(map) => map,
            other => panic!("expected map entry, found {:?}", other),
        };
        let title = only_map
            .get(&MapKey::Keyword(Keyword("title".to_string())))
            .unwrap();
        assert_eq!(title, &Value::String("Rust issue".to_string()));
    }
}
