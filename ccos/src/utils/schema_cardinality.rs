use serde_json::Value as JsonValue;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CardinalityAction {
    Map,
    Pass,
    Unknown,
}

pub fn is_rtfs_collection_schema(rtfs_schema: &str) -> bool {
    let trimmed = rtfs_schema.trim_start();
    trimmed.starts_with("[:vector") || trimmed.starts_with("[:list") || trimmed.starts_with("[:set")
}

pub fn consumer_param_expects_array(consumer_schema: &JsonValue, param: &str) -> Option<bool> {
    consumer_schema
        .get("properties")
        .and_then(|p| p.as_object())
        .and_then(|props| props.get(param))
        .and_then(|prop_def| {
            let type_val = prop_def.get("type")?;
            if let Some(s) = type_val.as_str() {
                return Some(s == "array");
            }
            if let Some(arr) = type_val.as_array() {
                return Some(arr.iter().any(|v| v.as_str() == Some("array")));
            }
            None
        })
}

/// Conservative policy:
/// - If source is a collection and target param is explicitly NOT an array => Map
/// - If target param expects array => Pass
/// - If unknown => Unknown
/// - If source is not a collection => Pass
pub fn cardinality_action(
    source_rtfs_schema: &str,
    consumer_schema: &JsonValue,
    param: &str,
) -> CardinalityAction {
    if is_rtfs_collection_schema(source_rtfs_schema) {
        match consumer_param_expects_array(consumer_schema, param) {
            Some(false) => CardinalityAction::Map,
            Some(true) => CardinalityAction::Pass,
            None => CardinalityAction::Unknown,
        }
    } else {
        CardinalityAction::Pass
    }
}
