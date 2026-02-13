use crate::capability_marketplace::CapabilityMarketplace;
use crate::utils::schema_cardinality::{
    cardinality_action, consumer_param_expects_array, is_rtfs_collection_schema, CardinalityAction,
};
use crate::utils::value_conversion::rtfs_value_to_json;
use rtfs::ast::{Keyword, MapKey};
use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use rtfs::runtime::values::Value;
use std::sync::Arc;

fn get_map_string(map: &std::collections::HashMap<MapKey, Value>, key: &str) -> Option<String> {
    map.get(&MapKey::Keyword(Keyword(key.to_string())))
        .or_else(|| map.get(&MapKey::String(key.to_string())))
        .and_then(|v| match v {
            Value::String(s) => Some(s.clone()),
            _ => Some(v.to_string()),
        })
}

fn get_map_value<'a>(
    map: &'a std::collections::HashMap<MapKey, Value>,
    key: &str,
) -> Option<&'a Value> {
    map.get(&MapKey::Keyword(Keyword(key.to_string())))
        .or_else(|| map.get(&MapKey::String(key.to_string())))
}

fn mk_hint(action: &str, reason: &str, param: Option<&str>) -> Value {
    let mut out = std::collections::HashMap::new();
    out.insert(
        MapKey::Keyword(Keyword("action".to_string())),
        Value::Keyword(Keyword(action.to_string())),
    );
    out.insert(
        MapKey::Keyword(Keyword("reason".to_string())),
        Value::String(reason.to_string()),
    );
    if let Some(param) = param {
        out.insert(
            MapKey::Keyword(Keyword("param".to_string())),
            Value::String(param.to_string()),
        );
    }
    Value::Map(out)
}

fn cardinality_hint_impl(input: &Value) -> RuntimeResult<Value> {
    let map = match input {
        Value::Map(m) => m,
        other => {
            return Err(RuntimeError::TypeError {
                expected: "map".to_string(),
                actual: other.type_name().to_string(),
                operation: "ccos.schema.cardinality_hint".to_string(),
            })
        }
    };

    // Expected inputs (generic):
    // - :source_rtfs_schema (string)
    // - :consumer_schema (json schema as map or stringified json)
    // - :param (string)
    let source_schema =
        get_map_string(map, "source_rtfs_schema").or_else(|| get_map_string(map, "source_schema"));
    let param = get_map_string(map, "param").or_else(|| get_map_string(map, "param_name"));
    let consumer_schema_val =
        get_map_value(map, "consumer_schema").or_else(|| get_map_value(map, "target_schema"));

    let Some(source_schema) = source_schema else {
        return Ok(mk_hint(
            "unknown",
            "Missing :source_rtfs_schema",
            param.as_deref(),
        ));
    };
    let Some(param) = param else {
        return Ok(mk_hint("unknown", "Missing :param", None));
    };
    let Some(consumer_schema_val) = consumer_schema_val else {
        return Ok(mk_hint("unknown", "Missing :consumer_schema", Some(&param)));
    };

    let consumer_schema_json = match consumer_schema_val {
        Value::String(s) => serde_json::from_str::<serde_json::Value>(s).map_err(|e| {
            RuntimeError::Generic(format!(
                "Invalid JSON in :consumer_schema for ccos.schema.cardinality_hint: {e}"
            ))
        })?,
        other => rtfs_value_to_json(other)?,
    };

    let source_is_collection = is_rtfs_collection_schema(&source_schema);
    let param_expects_array = consumer_param_expects_array(&consumer_schema_json, &param);

    match cardinality_action(&source_schema, &consumer_schema_json, &param) {
        CardinalityAction::Map => Ok(mk_hint(
            "map",
            "Source is a collection but target param is not an array",
            Some(&param),
        )),
        CardinalityAction::Pass => {
            if source_is_collection {
                if param_expects_array == Some(true) {
                    Ok(mk_hint(
                        "pass",
                        "Target param expects an array",
                        Some(&param),
                    ))
                } else {
                    Ok(mk_hint("pass", "Source is not a collection", Some(&param)))
                }
            } else {
                Ok(mk_hint("pass", "Source is not a collection", Some(&param)))
            }
        }
        CardinalityAction::Unknown => Ok(mk_hint(
            "unknown",
            "Cannot determine whether target param expects an array",
            Some(&param),
        )),
    }
}

pub async fn register_default_capabilities(
    marketplace: &CapabilityMarketplace,
) -> RuntimeResult<()> {
    // Register data processing capabilities
    crate::capabilities::data_processing::register_data_capabilities(marketplace).await?;

    // Register network capabilities (http-fetch, etc.)
    crate::capabilities::network::register_network_capabilities(marketplace, None).await?;

    // This function is extracted from the stdlib to keep stdlib focused on
    // language-level functions. Implementation preserved from original file.

    // Register ccos.echo capability - pure compute, safe for grounding
    marketplace
        .register_local_capability_with_effects(
            "ccos.echo".to_string(),
            "Echo Capability".to_string(),
            "Echoes the input value back".to_string(),
            Arc::new(|input| {
                match input {
                    // New calling convention: map with :args and optional :context
                    Value::Map(map) => {
                        // Try standard :args convention first
                        let args_val = map
                            .get(&MapKey::Keyword(Keyword("args".to_string())))
                            .or_else(|| map.get(&MapKey::String("args".to_string())));

                        if let Some(args_val) = args_val {
                            // Standard path...
                            match args_val {
                                Value::List(args) | Value::Vector(args) => {
                                    if args.len() == 1 {
                                        Ok(args[0].clone())
                                    } else {
                                        Err(RuntimeError::ArityMismatch {
                                            function: "ccos.echo".to_string(),
                                            expected: "1".to_string(),
                                            actual: args.len(),
                                        })
                                    }
                                }
                                other => Err(RuntimeError::TypeError {
                                    expected: "list".to_string(),
                                    actual: other.type_name().to_string(),
                                    operation: "ccos.echo".to_string(),
                                }),
                            }
                        } else {
                            // Fallback: check for "message", "content", "text", "value"
                            for key in &["message", "content", "text", "value"] {
                                if let Some(val) = map
                                    .get(&MapKey::String(key.to_string()))
                                    .or_else(|| map.get(&MapKey::Keyword(Keyword(key.to_string()))))
                                {
                                    return Ok(val.clone());
                                }
                            }
                            // Last resort: if map has exactly one value, use it
                            if map.len() == 1 {
                                if let Some(val) = map.values().next() {
                                    return Ok(val.clone());
                                }
                            }

                            Err(RuntimeError::Generic(
                                "Missing :args (or 'message'/'value') for ccos.echo".to_string(),
                            ))
                        }
                    }
                    // Backward compatibility: still accept a plain list
                    Value::List(args) | Value::Vector(args) => {
                        if args.len() == 1 {
                            Ok(args[0].clone())
                        } else {
                            Err(RuntimeError::ArityMismatch {
                                function: "ccos.echo".to_string(),
                                expected: "1".to_string(),
                                actual: args.len(),
                            })
                        }
                    }
                    _ => Err(RuntimeError::TypeError {
                        expected: "map or list".to_string(),
                        actual: input.type_name().to_string(),
                        operation: "ccos.echo".to_string(),
                    }),
                }
            }),
            vec![":compute".to_string()], // Safe effect - pure data transformation
        )
        .await
        .map_err(|e| RuntimeError::Generic(format!("Failed to register ccos.echo: {:?}", e)))?;

    // Register ccos.math.add capability - pure compute, safe for grounding
    marketplace
        .register_local_capability_with_effects(
            "ccos.math.add".to_string(),
            "Math Add Capability".to_string(),
            "Adds numeric values".to_string(),
            Arc::new(|input| {
                match input {
                    // New calling convention: map with :args containing the argument list
                    Value::Map(map) => {
                        let args_val = map
                            .get(&MapKey::Keyword(Keyword("args".to_string())))
                            .or_else(|| map.get(&MapKey::String("args".to_string())));

                        if let Some(args_val) = args_val {
                            match args_val {
                                Value::List(args) | Value::Vector(args) => {
                                    let mut sum = 0i64;
                                    for arg in args {
                                        match arg {
                                            Value::Integer(i) => sum += i,
                                            _ => {
                                                return Err(RuntimeError::TypeError {
                                                    expected: "integer".to_string(),
                                                    actual: arg.type_name().to_string(),
                                                    operation: "ccos.math.add".to_string(),
                                                })
                                            }
                                        }
                                    }
                                    Ok(Value::Integer(sum))
                                }
                                other => Err(RuntimeError::TypeError {
                                    expected: "list".to_string(),
                                    actual: other.type_name().to_string(),
                                    operation: "ccos.math.add".to_string(),
                                }),
                            }
                        } else {
                            // Fallback: sum all integer values in the map
                            // This supports { "a": 1, "b": 2 } usage
                            let mut sum = 0i64;
                            let mut found_int = false;

                            for val in map.values() {
                                if let Value::Integer(i) = val {
                                    sum += i;
                                    found_int = true;
                                }
                            }

                            if found_int {
                                Ok(Value::Integer(sum))
                            } else {
                                Err(RuntimeError::Generic(
                                    "Missing :args or integer values for ccos.math.add".to_string(),
                                ))
                            }
                        }
                    }
                    // Backward compatibility: direct list of arguments
                    Value::List(args) | Value::Vector(args) => {
                        let mut sum = 0i64;
                        for arg in args {
                            match arg {
                                Value::Integer(i) => sum += i,
                                _ => {
                                    return Err(RuntimeError::TypeError {
                                        expected: "integer".to_string(),
                                        actual: arg.type_name().to_string(),
                                        operation: "ccos.math.add".to_string(),
                                    })
                                }
                            }
                        }
                        Ok(Value::Integer(sum))
                    }
                    other => Err(RuntimeError::TypeError {
                        expected: "map or list".to_string(),
                        actual: other.type_name().to_string(),
                        operation: "ccos.math.add".to_string(),
                    }),
                }
            }),
            vec![":compute".to_string()], // Safe effect - pure computation
        )
        .await
        .map_err(|e| RuntimeError::Generic(format!("Failed to register ccos.math.add: {:?}", e)))?;

    // Register ccos.schema.cardinality_hint capability - deterministic schema helper
    // Effect: :compute - safe for grounding
    // Returns: {:action :map|:pass|:unknown :reason "..." :param "..."}
    marketplace
        .register_local_capability_with_effects(
            "ccos.schema.cardinality_hint".to_string(),
            "Schema Cardinality Hint".to_string(),
            "Suggests whether a consumer call should map over a collection source".to_string(),
            Arc::new(|input| cardinality_hint_impl(input)),
            vec![":compute".to_string()],
        )
        .await
        .map_err(|e| {
            RuntimeError::Generic(format!(
                "Failed to register ccos.schema.cardinality_hint: {:?}",
                e
            ))
        })?;

    // Register ccos.user.ask capability (interactive user input - reads from stdin)
    // Effect: :interactive - NOT safe for grounding (requires human interaction)
    marketplace
        .register_local_capability_with_effects(
            "ccos.user.ask".to_string(),
            "User Ask Capability".to_string(),
            "Prompts the user for input and reads from stdin".to_string(),
            Arc::new(|input| {
                // Extract prompt
                let extract_prompt = |val: &Value| -> Result<String, RuntimeError> {
                    match val {
                        Value::Map(map) => {
                            // Try standard :args convention first
                            let args_val = map
                                .get(&MapKey::Keyword(Keyword("args".to_string())))
                                .or_else(|| map.get(&MapKey::String("args".to_string())));

                            if let Some(args_val) = args_val {
                                if let Value::List(args) | Value::Vector(args) = args_val {
                                    if args.len() == 1 {
                                        return Ok(args[0].to_string());
                                    } else {
                                        return Err(RuntimeError::ArityMismatch {
                                            function: "ccos.user.ask".to_string(),
                                            expected: "1".to_string(),
                                            actual: args.len(),
                                        });
                                    }
                                }
                            }

                            // Fallback: check for common keys like "prompt", "question", "message"
                            // or if map has exactly one string value
                            for key in &["prompt", "question", "message", "text"] {
                                if let Some(val) = map
                                    .get(&MapKey::String(key.to_string()))
                                    .or_else(|| map.get(&MapKey::Keyword(Keyword(key.to_string()))))
                                {
                                    return Ok(val.to_string());
                                }
                            }

                            // Last resort: if map has exactly one entry and value is string, use it
                            if map.len() == 1 {
                                if let Some(val) = map.values().next() {
                                    return Ok(val.to_string());
                                }
                            }

                            Err(RuntimeError::Generic(
                                "Missing :args (or 'prompt'/'question') for ccos.user.ask"
                                    .to_string(),
                            ))
                        }
                        Value::List(args) | Value::Vector(args) => {
                            if args.len() == 1 {
                                Ok(args[0].to_string())
                            } else {
                                Err(RuntimeError::ArityMismatch {
                                    function: "ccos.user.ask".to_string(),
                                    expected: "1".to_string(),
                                    actual: args.len(),
                                })
                            }
                        }
                        other => Err(RuntimeError::TypeError {
                            expected: "map or list".to_string(),
                            actual: other.type_name().to_string(),
                            operation: "ccos.user.ask".to_string(),
                        }),
                    }
                };

                let prompt = extract_prompt(&input)?;

                // Print the prompt for the host/operator
                print!("[ccos.user.ask] {}", prompt);
                use std::io::{self, Write};
                let mut stdout = io::stdout();
                let _ = stdout.flush();

                // Read a line from stdin (blocking). This is intentional: this capability represents
                // interactive user input and must be wired to the host's stdin in this example.
                let mut line = String::new();
                io::stdin()
                    .read_line(&mut line)
                    .map_err(|e| RuntimeError::Generic(format!("Failed to read stdin: {}", e)))?;
                let answer = line.trim().to_string();
                Ok(Value::String(answer))
            }),
            vec![":interactive".to_string()], // NOT safe for automated execution
        )
        .await
        .map_err(|e| RuntimeError::Generic(format!("Failed to register ccos.user.ask: {:?}", e)))?;

    // Register ccos.io.println capability (used by RTFS stdlib println)
    // Effect: :output - safe for grounding (just prints to console)
    marketplace
        .register_local_capability_with_effects(
            "ccos.io.println".to_string(),
            "Print Line Capability".to_string(),
            "Prints a line to stdout".to_string(),
            Arc::new(|input| {
                let message = match input {
                    Value::Map(map) => {
                        let args_val = map
                            .get(&MapKey::Keyword(Keyword("args".to_string())))
                            .or_else(|| map.get(&MapKey::String("args".to_string())));

                        if let Some(args_val) = args_val {
                            match args_val {
                                Value::List(args) | Value::Vector(args) => args
                                    .iter()
                                    .map(|v| match v {
                                        Value::String(s) => s.clone(),
                                        _ => format!("{}", v),
                                    })
                                    .collect::<Vec<_>>()
                                    .join(" "),
                                _ => format!("{}", args_val),
                            }
                        } else {
                            // Fallback: check common keys
                            let mut msg = None;

                            // Check _previous_result first (data pipeline flow)
                            if let Some(val) = map
                                .get(&MapKey::String("_previous_result".to_string()))
                                .or_else(|| {
                                    map.get(&MapKey::Keyword(Keyword(
                                        "_previous_result".to_string(),
                                    )))
                                })
                            {
                                msg = Some(match val {
                                    Value::String(s) => s.clone(),
                                    _ => format!("{}", val),
                                });
                            }

                            if msg.is_none() {
                                for key in &["message", "content", "text", "value"] {
                                    if let Some(val) =
                                        map.get(&MapKey::String(key.to_string())).or_else(|| {
                                            map.get(&MapKey::Keyword(Keyword(key.to_string())))
                                        })
                                    {
                                        msg = Some(match val {
                                            Value::String(s) => s.clone(),
                                            _ => format!("{}", val),
                                        });
                                        break;
                                    }
                                }
                            }

                            msg.unwrap_or_else(|| {
                                // If single value map, use it
                                if map.len() == 1 {
                                    if let Some(val) = map.values().next() {
                                        return match val {
                                            Value::String(s) => s.clone(),
                                            _ => format!("{}", val),
                                        };
                                    }
                                }
                                format!("{}", input)
                            })
                        }
                    }
                    Value::List(args) | Value::Vector(args) => args
                        .iter()
                        .map(|v| match v {
                            Value::String(s) => s.clone(),
                            _ => format!("{}", v),
                        })
                        .collect::<Vec<_>>()
                        .join(" "),
                    other => format!("{}", other),
                };
                println!("{}", message);
                Ok(Value::Nil)
            }),
            vec![":output".to_string()], // Safe effect - console output only
        )
        .await
        .map_err(|e| {
            RuntimeError::Generic(format!("Failed to register ccos.io.println: {:?}", e))
        })?;

    // Register ccos.io.log capability (used by RTFS stdlib log)
    // Effect: :output - safe for grounding (just logs to console)
    marketplace
        .register_local_capability_with_effects(
            "ccos.io.log".to_string(),
            "Log Capability".to_string(),
            "Logs a message".to_string(),
            Arc::new(|input| {
                let message = match input {
                    Value::Map(map) => {
                        let args_val = map
                            .get(&MapKey::Keyword(Keyword("args".to_string())))
                            .or_else(|| map.get(&MapKey::String("args".to_string())));

                        if let Some(args_val) = args_val {
                            match args_val {
                                Value::List(args) | Value::Vector(args) => args
                                    .iter()
                                    .map(|v| match v {
                                        Value::String(s) => s.clone(),
                                        _ => format!("{}", v),
                                    })
                                    .collect::<Vec<_>>()
                                    .join(" "),
                                _ => format!("{}", args_val),
                            }
                        } else {
                            // Fallback: check common keys
                            let mut msg = None;
                            for key in &["message", "content", "text", "value"] {
                                if let Some(val) = map
                                    .get(&MapKey::String(key.to_string()))
                                    .or_else(|| map.get(&MapKey::Keyword(Keyword(key.to_string()))))
                                {
                                    msg = Some(match val {
                                        Value::String(s) => s.clone(),
                                        _ => format!("{}", val),
                                    });
                                    break;
                                }
                            }

                            msg.unwrap_or_else(|| {
                                // If single value map, use it
                                if map.len() == 1 {
                                    if let Some(val) = map.values().next() {
                                        return match val {
                                            Value::String(s) => s.clone(),
                                            _ => format!("{}", val),
                                        };
                                    }
                                }
                                format!("{}", input)
                            })
                        }
                    }
                    Value::List(args) | Value::Vector(args) => args
                        .iter()
                        .map(|v| match v {
                            Value::String(s) => s.clone(),
                            _ => format!("{}", v),
                        })
                        .collect::<Vec<_>>()
                        .join(" "),
                    other => format!("{}", other),
                };
                println!("[CCOS-LOG] {}", message);
                Ok(Value::Nil)
            }),
            vec![":output".to_string()], // Safe effect - console output only
        )
        .await
        .map_err(|e| RuntimeError::Generic(format!("Failed to register ccos.io.log: {:?}", e)))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cardinality_hint_map_when_source_collection_and_param_scalar() {
        let input = Value::Map(std::collections::HashMap::from([
            (
                MapKey::Keyword(Keyword("source_rtfs_schema".to_string())),
                Value::String("[:vector :string]".to_string()),
            ),
            (
                MapKey::Keyword(Keyword("consumer_schema".to_string())),
                crate::utils::value_conversion::json_to_rtfs_value(&serde_json::json!({
                    "type": "object",
                    "properties": {
                        "data": { "type": "string" }
                    }
                }))
                .unwrap(),
            ),
            (
                MapKey::Keyword(Keyword("param".to_string())),
                Value::String("data".to_string()),
            ),
        ]));

        let out = cardinality_hint_impl(&input).unwrap();
        let Value::Map(m) = out else {
            panic!("expected map");
        };
        assert_eq!(
            m.get(&MapKey::Keyword(Keyword("action".to_string()))),
            Some(&Value::Keyword(Keyword("map".to_string())))
        );
    }

    #[test]
    fn test_cardinality_hint_pass_when_param_expects_array() {
        let input = Value::Map(std::collections::HashMap::from([
            (
                MapKey::Keyword(Keyword("source_rtfs_schema".to_string())),
                Value::String("[:vector :string]".to_string()),
            ),
            (
                MapKey::Keyword(Keyword("consumer_schema".to_string())),
                crate::utils::value_conversion::json_to_rtfs_value(&serde_json::json!({
                    "type": "object",
                    "properties": {
                        "data": { "type": "array" }
                    }
                }))
                .unwrap(),
            ),
            (
                MapKey::Keyword(Keyword("param".to_string())),
                Value::String("data".to_string()),
            ),
        ]));

        let out = cardinality_hint_impl(&input).unwrap();
        let Value::Map(m) = out else {
            panic!("expected map");
        };
        assert_eq!(
            m.get(&MapKey::Keyword(Keyword("action".to_string()))),
            Some(&Value::Keyword(Keyword("pass".to_string())))
        );
    }
}
