use crate::capability_marketplace::CapabilityMarketplace;
use rtfs::ast::{Keyword, MapKey};
use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use rtfs::runtime::values::Value;
use std::sync::Arc;

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
                            .or_else(|| map.get(&MapKey::String("args".to_string())))
                            .cloned()
                            .unwrap_or(Value::List(vec![]));
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
                        let args_val = map
                            .get(&MapKey::Keyword(Keyword("args".to_string())))
                            .or_else(|| map.get(&MapKey::String("args".to_string())));

                        if let Some(args_val) = args_val {
                            match args_val {
                                Value::List(args) | Value::Vector(args) => {
                                    let mut sum = 0i64;
                                    for arg in args {
                                        match arg {
                                            Value::Integer(i) => sum += *i,
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
                            Err(RuntimeError::Generic(
                                "Missing :args for ccos.math.add".to_string(),
                            ))
                        }
                    }
                    // Backward compatibility: direct list of arguments
                    Value::List(args) | Value::Vector(args) => {
                        let mut sum = 0i64;
                        for arg in args {
                            match arg {
                                Value::Integer(i) => sum += *i,
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
        )
        .await
        .map_err(|e| RuntimeError::Generic(format!("Failed to register ccos.math.add: {:?}", e)))?;

    // Register ccos.user.ask capability (interactive user input - reads from stdin)
    marketplace
        .register_local_capability(
            "ccos.user.ask".to_string(),
            "User Ask Capability".to_string(),
            "Prompts the user for input and reads from stdin".to_string(),
            Arc::new(|input| {
                // Extract prompt
                let extract_prompt = |val: &Value| -> Result<String, RuntimeError> {
                    match val {
                        Value::Map(map) => {
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
                            Err(RuntimeError::Generic(
                                "Missing :args for ccos.user.ask".to_string(),
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
        )
        .await
        .map_err(|e| RuntimeError::Generic(format!("Failed to register ccos.user.ask: {:?}", e)))?;

    // ccos.io.println and ccos.io.log have been removed from core capabilities
    // as they are available via RTFS standard library functions (println, log).

    Ok(())
}
