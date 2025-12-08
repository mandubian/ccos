use crate::capability_marketplace::CapabilityMarketplace;
use rtfs::ast::{Keyword, MapKey};
use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use rtfs::runtime::values::Value;
use std::cmp::Ordering;
use std::sync::Arc;

pub async fn register_data_capabilities(marketplace: &CapabilityMarketplace) -> RuntimeResult<()> {
    // ccos.data.filter - pure compute capability, safe for grounding
    marketplace
        .register_local_capability_with_effects(
            "ccos.data.filter".to_string(),
            "Filter Data".to_string(),
            "Filters a list of objects based on exact match criteria. Params: data (list), criteria (map)".to_string(),
            Arc::new(|input| {
                let (data, criteria) = extract_data_and_params(input, "ccos.data.filter", "criteria")?;

                let criteria_map = match criteria {
                    Some(Value::Map(m)) => Some(m),
                    None => None,
                    Some(_) => return Err(RuntimeError::TypeError {
                        expected: "map".to_string(),
                        actual: "other".to_string(),
                        operation: "ccos.data.filter param 'criteria'".to_string(),
                    }),
                };

                // Extract list from data - handle both direct lists and wrapped results
                let items = extract_list_from_value(data)?;

                if let Some(crit) = criteria_map {
                    let filtered: Vec<Value> = items.iter().filter(|item| {
                        if let Value::Map(item_map) = item {
                            crit.iter().all(|(k, v)| {
                                item_map.get(k).map_or(false, |val| val == v)
                            })
                        } else {
                            false
                        }
                    }).cloned().collect();
                    Ok(Value::List(filtered))
                } else {
                    // No criteria, return all
                    Ok(Value::List(items.clone()))
                }
            }),
            vec![":compute".to_string()], // Safe effect - pure data transformation
        )
        .await
        .map_err(|e| RuntimeError::Generic(format!("Failed to register ccos.data.filter: {:?}", e)))?;

    // ccos.data.sort - pure compute capability, safe for grounding
    marketplace
        .register_local_capability_with_effects(
            "ccos.data.sort".to_string(),
            "Sort Data".to_string(),
            "Sorts a list of objects. Params: data (list), key (string), order (string: 'asc'|'desc')".to_string(),
            Arc::new(|input| {
                let (data, params_val) = extract_data_and_params(input, "ccos.data.sort", "params")?;

                let (key, order) = if let Some(Value::Map(params)) = params_val {
                    let k = params.get(&MapKey::String("key".to_string()))
                        .or_else(|| params.get(&MapKey::Keyword(Keyword("key".to_string()))))
                        .map(|v| value_to_clean_string(v));

                    let o = params.get(&MapKey::String("order".to_string()))
                        .or_else(|| params.get(&MapKey::Keyword(Keyword("order".to_string()))))
                        .map(|v| value_to_clean_string(v).to_lowercase())
                        .unwrap_or("asc".to_string());

                    (k, o)
                } else {
                    (None, "asc".to_string())
                };

                // Extract list from data - handle both direct lists and wrapped results like {"items": [...]}
                let items = extract_list_from_value(data)?;

                let mut sorted = items.clone();
                sorted.sort_by(|a, b| {
                    let val_a = if let Some(ref k) = key {
                        get_path(a, k)
                    } else {
                        Some(a)
                    };
                    let val_b = if let Some(ref k) = key {
                        get_path(b, k)
                    } else {
                        Some(b)
                    };

                    match (val_a, val_b) {
                        (Some(va), Some(vb)) => compare_values(va, vb),
                        (Some(_), None) => Ordering::Less,
                        (None, Some(_)) => Ordering::Greater,
                        (None, None) => Ordering::Equal,
                    }
                });

                if order == "desc" {
                    sorted.reverse();
                }

                Ok(Value::List(sorted))
            }),
            vec![":compute".to_string()], // Safe effect - pure data transformation
        )
        .await
        .map_err(|e| RuntimeError::Generic(format!("Failed to register ccos.data.sort: {:?}", e)))?;

    // ccos.data.select (limit/take) - pure compute capability, safe for grounding
    marketplace
        .register_local_capability_with_effects(
            "ccos.data.select".to_string(),
            "Select Data".to_string(),
            "Selects items from a list. Params: data (list), count (int)".to_string(),
            Arc::new(|input| {
                let (data, params_val) =
                    extract_data_and_params(input, "ccos.data.select", "params")?;

                let count = if let Some(Value::Map(params)) = params_val {
                    params
                        .get(&MapKey::String("count".to_string()))
                        .or_else(|| params.get(&MapKey::Keyword(Keyword("count".to_string()))))
                        .and_then(|v| match v {
                            Value::Integer(i) => Some(*i as usize),
                            Value::String(s) => s.parse().ok(),
                            _ => None,
                        })
                        .unwrap_or(1)
                } else {
                    1
                };

                // Extract list from data - handle both direct lists and wrapped results
                let items = extract_list_from_value(data)?;

                let selected: Vec<Value> = items.iter().take(count).cloned().collect();

                // If count is 1, return the single item directly for convenience
                // This makes the output cleaner for "get the one with most stars" type queries
                if count == 1 && selected.len() == 1 {
                    Ok(selected.into_iter().next().unwrap())
                } else {
                    Ok(Value::List(selected))
                }
            }),
            vec![":compute".to_string()], // Safe effect - pure data transformation
        )
        .await
        .map_err(|e| {
            RuntimeError::Generic(format!("Failed to register ccos.data.select: {:?}", e))
        })?;

    Ok(())
}

// Helpers

/// Extract a list from a value, handling various wrapper formats.
/// This function recursively unwraps common container patterns to find the underlying list.
fn extract_list_from_value(data: &Value) -> RuntimeResult<Vec<Value>> {
    // Direct list - return immediately
    if let Value::List(items) | Value::Vector(items) = data {
        return Ok(items.clone());
    }

    // If it's a map, try to find a list inside
    if let Value::Map(map) = data {
        // First, try to find any key that contains a list
        for (_key, val) in map.iter() {
            match val {
                Value::List(items) | Value::Vector(items) => {
                    return Ok(items.clone());
                }
                // Recurse into nested maps (e.g., {"content": {"items": [...]}})
                Value::Map(_) => {
                    if let Ok(items) = extract_list_from_value(val) {
                        return Ok(items);
                    }
                }
                _ => continue,
            }
        }

        // No list found in any key
        return Err(RuntimeError::TypeError {
            expected: "list or map containing a list".to_string(),
            actual: format!(
                "map with keys: {:?}",
                map.keys().take(5).collect::<Vec<_>>()
            ),
            operation: "extract_list_from_value".to_string(),
        });
    }

    Err(RuntimeError::TypeError {
        expected: "list or map containing a list".to_string(),
        actual: data.type_name().to_string(),
        operation: "extract_list_from_value".to_string(),
    })
}

/// Convert a Value to a clean string (strips quotes)
fn value_to_clean_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => {
            let s = other.to_string();
            // Remove surrounding quotes if present
            s.trim_matches('"').to_string()
        }
    }
}

fn extract_data_and_params<'a>(
    input: &'a Value,
    op_name: &str,
    secondary_param: &str,
) -> RuntimeResult<(&'a Value, Option<&'a Value>)> {
    match input {
        Value::Map(map) => {
            // Get _previous_result first (this is the actual data from pipeline)
            let previous_result = map
                .get(&MapKey::String("_previous_result".to_string()))
                .or_else(|| map.get(&MapKey::Keyword(Keyword("_previous_result".to_string()))));

            // Get "data" param (may be actual data or a string reference like "step_0_result")
            let data_param = map
                .get(&MapKey::String("data".to_string()))
                .or_else(|| map.get(&MapKey::Keyword(Keyword("data".to_string()))));

            // Determine actual data to use:
            // 1. If "data" is a list/map (actual data), use it
            // 2. If "data" is a string (reference like "step_0_result"), use _previous_result instead
            // 3. If no "data", use _previous_result
            let data = match data_param {
                Some(Value::List(_)) | Some(Value::Vector(_)) | Some(Value::Map(_)) => data_param,
                Some(Value::String(s)) => {
                    // This is a reference string like "step_0_result", use _previous_result
                    log::debug!(
                        "[data_processing] 'data' is string reference '{}', using _previous_result",
                        s
                    );
                    previous_result
                }
                _ => previous_result,
            };

            let params = map
                .get(&MapKey::String(secondary_param.to_string()))
                .or_else(|| map.get(&MapKey::Keyword(Keyword(secondary_param.to_string()))));

            if let Some(d) = data {
                // If params is not found, use the input map itself for param extraction
                let p = if params.is_some() {
                    params
                } else {
                    Some(input)
                };
                Ok((d, p))
            } else {
                Err(RuntimeError::Generic(format!(
                    "Missing data for {}. Provide 'data' (list/map) or ensure '_previous_result' is passed from previous step.",
                    op_name
                )))
            }
        }
        _ => Err(RuntimeError::TypeError {
            expected: "map".to_string(),
            actual: input.type_name().to_string(),
            operation: op_name.to_string(),
        }),
    }
}

fn get_path<'a>(val: &'a Value, path: &str) -> Option<&'a Value> {
    // Simple top-level key for now. Could support dot notation.
    if let Value::Map(m) = val {
        m.get(&MapKey::String(path.to_string()))
            .or_else(|| m.get(&MapKey::Keyword(Keyword(path.to_string()))))
    } else {
        None
    }
}

fn compare_values(a: &Value, b: &Value) -> Ordering {
    match (a, b) {
        (Value::Integer(i1), Value::Integer(i2)) => i1.cmp(i2),
        (Value::Float(f1), Value::Float(f2)) => f1.partial_cmp(f2).unwrap_or(Ordering::Equal),
        // Handle numeric mix
        (Value::Integer(i), Value::Float(f)) => {
            (*i as f64).partial_cmp(f).unwrap_or(Ordering::Equal)
        }
        (Value::Float(f), Value::Integer(i)) => {
            f.partial_cmp(&(*i as f64)).unwrap_or(Ordering::Equal)
        }

        (Value::String(s1), Value::String(s2)) => s1.cmp(s2),
        (Value::Boolean(b1), Value::Boolean(b2)) => b1.cmp(b2),
        _ => Ordering::Equal, // Incomparable
    }
}
