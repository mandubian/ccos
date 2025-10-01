use std::sync::Arc;
use crate::runtime::values::Value;
use crate::runtime::error::{RuntimeError, RuntimeResult};
use crate::ccos::capability_marketplace::CapabilityMarketplace;
use crate::ast::{Keyword, MapKey};

pub async fn register_default_capabilities(
    marketplace: &CapabilityMarketplace,
) -> RuntimeResult<()> {
    // This function is extracted from the stdlib to keep stdlib focused on
    // language-level functions. Implementation preserved from original file.

    // Register ccos.echo capability
    marketplace
        .register_local_capability(
            "ccos.echo".to_string(),
            "Echo Capability".to_string(),
            "Echoes the input value back".to_string(),
            Arc::new(|input| {
                match input {
                    // New calling convention: map with :args and optional :context
                    Value::Map(map) => {
                        let args_val = map
                            .get(&MapKey::Keyword(Keyword("args".to_string())))
                            .cloned()
                            .unwrap_or(Value::List(vec![]));
                        match args_val {
                            Value::List(args) => {
                                if args.len() == 1 {
                                    Ok(args[0].clone())
                                } else {
                                    Err(RuntimeError::ArityMismatch { function: "ccos.echo".to_string(), expected: "1".to_string(), actual: args.len() })
                                }
                            }
                            other => Err(RuntimeError::TypeError { expected: "list".to_string(), actual: other.type_name().to_string(), operation: "ccos.echo".to_string() }),
                        }
                    }
                    // Backward compatibility: still accept a plain list
                    Value::List(args) => {
                        if args.len() == 1 {
                            Ok(args[0].clone())
                        } else {
                            Err(RuntimeError::ArityMismatch { function: "ccos.echo".to_string(), expected: "1".to_string(), actual: args.len() })
                        }
                    }
                    _ => Err(RuntimeError::TypeError { expected: "map or list".to_string(), actual: input.type_name().to_string(), operation: "ccos.echo".to_string() }),
                }
            }),
        )
        .await
        .map_err(|e| RuntimeError::Generic(format!("Failed to register ccos.echo: {:?}", e)))?;

    // Register ccos.math.add capability
    marketplace
        .register_local_capability(
            "ccos.math.add".to_string(),
            "Math Add Capability".to_string(),
            "Adds numeric values".to_string(),
            Arc::new(|input| {
                match input {
                    // New calling convention: map with :args containing the argument list
                    Value::Map(map) => {
                        if let Some(args_val) = map.get(&MapKey::Keyword(Keyword("args".to_string()))) {
                            match args_val {
                                Value::List(args) => {
                                    let mut sum = 0i64;
                                    for arg in args {
                                        match arg {
                                            Value::Integer(i) => sum += *i,
                                            _ => return Err(RuntimeError::TypeError { expected: "integer".to_string(), actual: arg.type_name().to_string(), operation: "ccos.math.add".to_string() }),
                                        }
                                    }
                                    Ok(Value::Integer(sum))
                                }
                                other => Err(RuntimeError::TypeError { expected: "list".to_string(), actual: other.type_name().to_string(), operation: "ccos.math.add".to_string() }),
                            }
                        } else {
                            Err(RuntimeError::Generic("Missing :args for ccos.math.add".to_string()))
                        }
                    }
                    // Backward compatibility: direct list of arguments
                    Value::List(args) => {
                        let mut sum = 0i64;
                        for arg in args {
                            match arg {
                                Value::Integer(i) => sum += *i,
                                _ => return Err(RuntimeError::TypeError { expected: "integer".to_string(), actual: arg.type_name().to_string(), operation: "ccos.math.add".to_string() }),
                            }
                        }
                        Ok(Value::Integer(sum))
                    }
                    other => Err(RuntimeError::TypeError { expected: "map or list".to_string(), actual: other.type_name().to_string(), operation: "ccos.math.add".to_string() }),
                }
            }),
        )
        .await
        .map_err(|e| RuntimeError::Generic(format!("Failed to register ccos.math.add: {:?}", e)))?;

    // Register ccos.user.ask capability (interactive user input)
    marketplace
        .register_local_capability(
            "ccos.user.ask".to_string(),
            "User Ask Capability".to_string(),
            "Prompts the user for input and returns their response".to_string(),
            Arc::new(|input| {
                match input {
                    // Map with :args containing [prompt_string]
                    Value::Map(map) => {
                        if let Some(args_val) = map.get(&MapKey::Keyword(Keyword("args".to_string()))) {
                            match args_val {
                                Value::List(args) => {
                                    if args.len() == 1 {
                                        // For now, echo back the prompt as response to avoid blocking IO in tests
                                        Ok(args[0].clone())
                                    } else {
                                        Err(RuntimeError::ArityMismatch { function: "ccos.user.ask".to_string(), expected: "1".to_string(), actual: args.len() })
                                    }
                                }
                                other => Err(RuntimeError::TypeError { expected: "list".to_string(), actual: other.type_name().to_string(), operation: "ccos.user.ask".to_string() }),
                            }
                        } else {
                            Err(RuntimeError::Generic("Missing :args for ccos.user.ask".to_string()))
                        }
                    }
                    _ => Err(RuntimeError::TypeError { expected: "map".to_string(), actual: input.type_name().to_string(), operation: "ccos.user.ask".to_string() }),
                }
            }),
        )
        .await
        .map_err(|e| RuntimeError::Generic(format!("Failed to register ccos.user.ask: {:?}", e)))?;

    Ok(())
}
