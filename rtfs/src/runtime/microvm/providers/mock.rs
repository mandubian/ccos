//! Mock MicroVM Provider for Testing

use crate::runtime::error::{RuntimeError, RuntimeResult};
use crate::runtime::microvm::core::{ExecutionContext, ExecutionMetadata, ExecutionResult};
use crate::runtime::microvm::providers::MicroVMProvider;
use crate::runtime::values::Value;
use std::time::Instant;

/// Mock MicroVM provider for testing and development
pub struct MockMicroVMProvider {
    pub initialized: bool,
}

impl MockMicroVMProvider {
    pub fn new() -> Self {
        Self { initialized: false }
    }
}

impl MicroVMProvider for MockMicroVMProvider {
    fn name(&self) -> &'static str {
        "mock"
    }

    fn is_available(&self) -> bool {
        true // Mock provider is always available
    }

    fn initialize(&mut self) -> RuntimeResult<()> {
        self.initialized = true;
        Ok(())
    }

    fn execute_program(&self, context: ExecutionContext) -> RuntimeResult<ExecutionResult> {
        if !self.initialized {
            return Err(RuntimeError::Generic(
                "Mock provider not initialized".to_string(),
            ));
        }

        // 🔒 SECURITY: Capability-based access control for program execution
        if let Some(capability_id) = &context.capability_id {
            // Check if the requested capability is in the allowed permissions
            if !context.capability_permissions.contains(capability_id) {
                return Err(RuntimeError::SecurityViolation {
                    operation: "execute_program".to_string(),
                    capability: capability_id.clone(),
                    context: format!("Allowed permissions: {:?}", context.capability_permissions),
                });
            }
        }

        let start_time = Instant::now();

        let result_value = match context.program {
            Some(program) => match program {
                crate::runtime::microvm::core::Program::RtfsSource(source) => {
                    // Delegate to RTFS runtime for proper evaluation
                    let module_registry = crate::runtime::module_runtime::ModuleRegistry::new();
                    let rtfs_runtime = crate::runtime::Runtime::new_with_tree_walking_strategy(
                        module_registry.into(),
                    );

                    // Use the RTFS runtime to evaluate the source directly
                    match rtfs_runtime.evaluate(&source) {
                        Ok(result) => result,
                        Err(_) => Value::String("Mock RTFS evaluation error".to_string()),
                    }
                }
                crate::runtime::microvm::core::Program::RtfsAst(_) => {
                    Value::String("Mock AST evaluation result".to_string())
                }
                crate::runtime::microvm::core::Program::RtfsBytecode(_) => {
                    Value::String("Mock bytecode evaluation result".to_string())
                }
                crate::runtime::microvm::core::Program::NativeFunction(func) => func(context.args)
                    .unwrap_or_else(|_| Value::String("Mock native function result".to_string())),
                crate::runtime::microvm::core::Program::ExternalProgram { path, args } => {
                    Value::String(format!("Mock external program: {} {:?}", path, args))
                }
            },
            None => Value::String("No program provided".to_string()),
        };

        let mut duration = start_time.elapsed();
        // Respect configured timeout in reported metadata
        if duration > context.config.timeout {
            duration = context.config.timeout;
        }

        Ok(ExecutionResult {
            value: result_value,
            metadata: ExecutionMetadata {
                duration,
                memory_used_mb: 1, // Mock memory usage
                cpu_time: duration,
                network_requests: vec![],
                file_operations: vec![],
            },
        })
    }

    fn execute_capability(&self, context: ExecutionContext) -> RuntimeResult<ExecutionResult> {
        if !self.initialized {
            return Err(RuntimeError::Generic(
                "Mock provider not initialized".to_string(),
            ));
        }

        // 🔒 SECURITY: Minimal boundary validation (central authorization already done)
        // Just ensure the capability ID is present in permissions if specified
        if let Some(capability_id) = &context.capability_id {
            if !context.capability_permissions.contains(capability_id) {
                return Err(RuntimeError::SecurityViolation {
                    operation: "execute_capability".to_string(),
                    capability: capability_id.clone(),
                    context: format!(
                        "Boundary validation failed - capability not in permissions: {:?}",
                        context.capability_permissions
                    ),
                });
            }
        }

        let start_time = Instant::now();

        let result_value = if let Some(capability_id) = context.capability_id {
            match capability_id.as_str() {
                "ccos.math.add" => {
                    if context.args.len() >= 2 {
                        if let (Some(Value::Integer(a)), Some(Value::Integer(b))) =
                            (context.args.get(0), context.args.get(1))
                        {
                            Value::String(format!(
                                "Mock capability execution result: {} + {} = {}",
                                a,
                                b,
                                a + b
                            ))
                        } else {
                            Value::String(
                                "Mock capability execution: Invalid arguments for math.add"
                                    .to_string(),
                            )
                        }
                    } else {
                        Value::String(
                            "Mock capability execution: Insufficient arguments for math.add"
                                .to_string(),
                        )
                    }
                }
                _ => Value::String(format!("Mock capability execution for: {}", capability_id)),
            }
        } else {
            Value::String("Mock capability execution: No capability ID provided".to_string())
        };

        let mut duration = start_time.elapsed();

        // Ensure we have a non-zero duration for testing
        if duration.as_nanos() == 0 {
            duration = std::time::Duration::from_millis(1);
        }
        // Respect configured timeout in reported metadata
        if duration > context.config.timeout {
            duration = context.config.timeout;
        }

        Ok(ExecutionResult {
            value: result_value,
            metadata: ExecutionMetadata {
                duration,
                memory_used_mb: 1,
                cpu_time: duration,
                network_requests: vec![],
                file_operations: vec![],
            },
        })
    }

    fn cleanup(&mut self) -> RuntimeResult<()> {
        self.initialized = false;
        Ok(())
    }

    fn get_config_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "mock_mode": {
                    "type": "string",
                    "enum": ["simple", "detailed"],
                    "default": "simple"
                }
            }
        })
    }
}
