use crate::ast::MapKey;
use crate::runtime::values::Value;
use std::collections::HashMap;

/// Helper: get a string value for a key from a runtime Value::Map whose keys are MapKey.
pub fn get_map_string_value(map: &HashMap<MapKey, Value>, key: &str) -> Option<String> {
    for (k, v) in map.iter() {
        let k_str = k.to_string();
        let k_trim = k_str.trim_start_matches(':');
        if k_trim == key {
            return match v {
                Value::String(s) => Some(s.clone()),
                other => Some(other.to_string()),
            };
        }
    }
    None
}

/// Extract question prompt from a PlanPaused action-like argument value.
/// Accepts either a plain string or a map containing common keys like `prompt`, `question`, or `text`.
pub fn extract_question_prompt_from_value(val: &Value) -> Option<String> {
    match val {
        Value::String(s) => Some(s.clone()),
        Value::Map(map) => {
            if let Some(p) = get_map_string_value(map, "prompt") {
                return Some(p);
            }
            if let Some(p) = get_map_string_value(map, "question") {
                return Some(p);
            }
            if let Some(p) = get_map_string_value(map, "text") {
                return Some(p);
            }
            for (_k, v) in map.iter() {
                if let Value::String(s) = v {
                    return Some(s.clone());
                }
            }
            None
        }
        _ => None,
    }
}

/// Wrapper to match the example's expected signature: examine Action.arguments[1]
pub fn extract_question_prompt_from_action(action: &crate::ccos::types::Action) -> Option<String> {
    if let Some(args) = &action.arguments {
        if args.len() >= 2 {
            return extract_question_prompt_from_value(&args[1]);
        }
    }
    None
}
