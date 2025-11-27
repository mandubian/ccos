use crate::capability_marketplace::CapabilityMarketplace;
use rtfs::ast::{Keyword, MapKey};
use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use rtfs::runtime::values::Value;
use std::cmp::Ordering;
use std::sync::Arc;

pub async fn register_data_capabilities(marketplace: &CapabilityMarketplace) -> RuntimeResult<()> {
    // ccos.data.filter
    marketplace
        .register_local_capability(
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

                match data {
                    Value::List(items) | Value::Vector(items) => {
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
                    }
                    _ => Err(RuntimeError::TypeError {
                        expected: "list".to_string(),
                        actual: data.type_name().to_string(),
                        operation: "ccos.data.filter param 'data'".to_string(),
                    }),
                }
            }),
        )
        .await
        .map_err(|e| RuntimeError::Generic(format!("Failed to register ccos.data.filter: {:?}", e)))?;

    // ccos.data.sort
    marketplace
        .register_local_capability(
            "ccos.data.sort".to_string(),
            "Sort Data".to_string(),
            "Sorts a list of objects. Params: data (list), key (string), order (string: 'asc'|'desc')".to_string(),
            Arc::new(|input| {
                let (data, params_val) = extract_data_and_params(input, "ccos.data.sort", "params")?;
                
                let (key, order) = if let Some(Value::Map(params)) = params_val {
                    let k = params.get(&MapKey::String("key".to_string()))
                        .or_else(|| params.get(&MapKey::Keyword(Keyword("key".to_string()))))
                        .map(|v| v.to_string());
                    
                    let o = params.get(&MapKey::String("order".to_string()))
                        .or_else(|| params.get(&MapKey::Keyword(Keyword("order".to_string()))))
                        .map(|v| v.to_string().to_lowercase())
                        .unwrap_or("asc".to_string());
                        
                    (k, o)
                } else {
                    (None, "asc".to_string())
                };

                match data {
                    Value::List(items) | Value::Vector(items) => {
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
                    }
                    _ => Err(RuntimeError::TypeError {
                        expected: "list".to_string(),
                        actual: data.type_name().to_string(),
                        operation: "ccos.data.sort".to_string(),
                    }),
                }
            }),
        )
        .await
        .map_err(|e| RuntimeError::Generic(format!("Failed to register ccos.data.sort: {:?}", e)))?;

    // ccos.data.select (limit/take)
    marketplace
        .register_local_capability(
            "ccos.data.select".to_string(),
            "Select Data".to_string(),
            "Selects items from a list. Params: data (list), count (int)".to_string(),
            Arc::new(|input| {
                let (data, params_val) = extract_data_and_params(input, "ccos.data.select", "params")?;
                
                let count = if let Some(Value::Map(params)) = params_val {
                    params.get(&MapKey::String("count".to_string()))
                        .or_else(|| params.get(&MapKey::Keyword(Keyword("count".to_string()))))
                        .and_then(|v| match v {
                            Value::Integer(i) => Some(*i as usize),
                            _ => None,
                        })
                        .unwrap_or(1)
                } else {
                    1
                };

                match data {
                    Value::List(items) | Value::Vector(items) => {
                        let selected: Vec<Value> = items.iter().take(count).cloned().collect();
                        // If count is 1 and we want a single item? 
                        // The LLM prompt implies "Select the repository", singular. 
                        // But usually data ops preserve collection type unless specified.
                        // Let's return a list for consistency, or maybe value if singular?
                        // For safety/composition, list is better. But if next step expects an object ID...
                        // Let's stick to list.
                        Ok(Value::List(selected))
                    }
                    _ => Err(RuntimeError::TypeError {
                        expected: "list".to_string(),
                        actual: data.type_name().to_string(),
                        operation: "ccos.data.select".to_string(),
                    }),
                }
            }),
        )
        .await
        .map_err(|e| RuntimeError::Generic(format!("Failed to register ccos.data.select: {:?}", e)))?;

    Ok(())
}

// Helpers

fn extract_data_and_params<'a>(input: &'a Value, op_name: &str, secondary_param: &str) -> RuntimeResult<(&'a Value, Option<&'a Value>)> {
    match input {
        Value::Map(map) => {
            // Check for "data" or "_previous_result"
            let data = map.get(&MapKey::String("data".to_string()))
                .or_else(|| map.get(&MapKey::Keyword(Keyword("data".to_string()))))
                .or_else(|| map.get(&MapKey::String("_previous_result".to_string())))
                .or_else(|| map.get(&MapKey::Keyword(Keyword("_previous_result".to_string()))));
                
            let params = map.get(&MapKey::String(secondary_param.to_string()))
                .or_else(|| map.get(&MapKey::Keyword(Keyword(secondary_param.to_string()))));
            
            // If we didn't find data explicitly, but map has other keys, maybe the map IS the params and data is missing (or implicitly passed)?
            // But usually we need data.
            
            if let Some(d) = data {
                // If params is not found, maybe look for flattened keys in the map itself?
                let p = if params.is_some() { params } else { Some(input) };
                Ok((d, p))
            } else {
                Err(RuntimeError::Generic(format!("Missing 'data' or '_previous_result' for {}", op_name)))
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
        (Value::Integer(i), Value::Float(f)) => (*i as f64).partial_cmp(f).unwrap_or(Ordering::Equal),
        (Value::Float(f), Value::Integer(i)) => f.partial_cmp(&(*i as f64)).unwrap_or(Ordering::Equal),
        
        (Value::String(s1), Value::String(s2)) => s1.cmp(s2),
        (Value::Boolean(b1), Value::Boolean(b2)) => b1.cmp(b2),
        _ => Ordering::Equal, // Incomparable
    }
}

