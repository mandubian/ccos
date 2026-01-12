//! WebAssembly MicroVM Provider

use crate::runtime::error::{RuntimeError, RuntimeResult};
use crate::runtime::microvm::core::{ExecutionContext, ExecutionMetadata, ExecutionResult};
use crate::runtime::microvm::providers::MicroVMProvider;
use crate::runtime::values::Value;
use std::io::Cursor;
use std::time::Instant;
use wasmtime::*;
use wasmtime_wasi::sync::WasiCtxBuilder;
use wasmtime_wasi::WasiCtx;

struct HostState {
    wasi: WasiCtx,
}

/// WebAssembly MicroVM provider for sandboxed execution
pub struct WasmMicroVMProvider {
    initialized: bool,
    engine: Engine,
}

impl WasmMicroVMProvider {
    pub fn new() -> Self {
        let mut config = Config::new();
        config.wasm_component_model(true);
        let engine = Engine::new(&config).unwrap_or_else(|_| Engine::default());

        Self {
            initialized: false,
            engine,
        }
    }

    fn execute_wasm_binary(
        &self,
        bytes: &[u8],
        context: &ExecutionContext,
    ) -> RuntimeResult<Value> {
        // Prepare input data (JSON)
        let input_json = if !context.args.is_empty() {
            let arg = &context.args[0];
            let json_val = crate::utils::rtfs_value_to_json(arg).unwrap_or(serde_json::Value::Null);
            serde_json::to_string(&json_val).unwrap_or_default()
        } else {
            "null".to_string()
        };

        // Setup WASI pipes
        let stdout_buf = Vec::new();
        let stdout_pipe = wasi_common::pipe::WritePipe::new(stdout_buf);
        let stdin_pipe = wasi_common::pipe::ReadPipe::new(Cursor::new(input_json));

        // Build WASI context
        let wasi = WasiCtxBuilder::new()
            .stdin(Box::new(stdin_pipe))
            .stdout(Box::new(stdout_pipe.clone()))
            .inherit_stderr()
            .build();

        let mut linker = Linker::new(&self.engine);
        wasmtime_wasi::add_to_linker(&mut linker, |state: &mut HostState| &mut state.wasi)
            .map_err(|e| RuntimeError::Generic(format!("Failed to add WASI to linker: {}", e)))?;

        // Compile and instantiate
        let module = Module::from_binary(&self.engine, bytes)
            .map_err(|e| RuntimeError::Generic(format!("Failed to compile WASM: {}", e)))?;

        let output_bytes = {
            let mut store = Store::new(&self.engine, HostState { wasi });

            let instance = linker
                .instantiate(&mut store, &module)
                .map_err(|e| RuntimeError::Generic(format!("Failed to instantiate WASM: {}", e)))?;

            // Call _start (standard WASI entry point)
            let start = instance
                .get_typed_func::<(), ()>(&mut store, "_start")
                .map_err(|e| {
                    RuntimeError::Generic(format!("Failed to find _start in WASM: {}", e))
                })?;

            start
                .call(&mut store, ())
                .map_err(|e| RuntimeError::Generic(format!("WASM execution failed: {}", e)))?;

            // Store is about to be dropped
            drop(store);

            stdout_pipe
                .try_into_inner()
                .map_err(|_| RuntimeError::Generic("Failed to get WASM output".to_string()))?
        };

        let output_str = String::from_utf8_lossy(&output_bytes).to_string();
        let trimmed_output = output_str.trim();

        if trimmed_output.is_empty() {
            return Ok(Value::Nil);
        }

        // Try to parse output as JSON, otherwise return as string
        if let Ok(json_val) = serde_json::from_str::<serde_json::Value>(trimmed_output) {
            crate::utils::json_to_rtfs_value(&json_val)
        } else {
            Ok(Value::String(output_str))
        }
    }

    fn execute_simple_wasm_module(
        &self,
        source: &str,
        _context: &ExecutionContext,
    ) -> RuntimeResult<Value> {
        // TODO: Implement actual WASM compilation and execution
        // This is a placeholder that returns a mock result

        // For now, just return a string indicating WASM execution
        Ok(Value::String(format!(
            "WASM execution result for: {}",
            source
        )))
    }

    fn create_simple_wasm_module(&self, _source: &str) -> RuntimeResult<Vec<u8>> {
        // TODO: Implement actual WASM module compilation
        // This is a placeholder that returns empty bytes
        Ok(vec![])
    }
}

impl MicroVMProvider for WasmMicroVMProvider {
    fn name(&self) -> &'static str {
        "wasm"
    }

    fn is_available(&self) -> bool {
        // Check if wasmtime is available
        // For now, assume it's available if the wasmtime crate is linked
        true
    }

    fn initialize(&mut self) -> RuntimeResult<()> {
        // Initialize WASM runtime
        self.initialized = true;
        Ok(())
    }

    fn execute_program(&self, context: ExecutionContext) -> RuntimeResult<ExecutionResult> {
        if !self.initialized {
            return Err(RuntimeError::Generic(
                "WASM provider not initialized".to_string(),
            ));
        }

        // 🔒 SECURITY: Minimal boundary validation (central authorization already done)
        // Just ensure the capability ID is present in permissions if specified
        if let Some(capability_id) = &context.capability_id {
            if !context.capability_permissions.contains(capability_id) {
                return Err(RuntimeError::SecurityViolation {
                    operation: "execute_program".to_string(),
                    capability: capability_id.clone(),
                    context: format!(
                        "Boundary validation failed - capability not in permissions: {:?}",
                        context.capability_permissions
                    ),
                });
            }
        }

        let start_time = Instant::now();

        let result_value = match context.program {
            Some(ref program) => match program {
                crate::runtime::microvm::core::Program::ScriptSource { language, source } => {
                    // TODO: In a real implementation, this would load a WASM-compiled interpreter
                    // (e.g. RustPython.wasm for Python, QuickJS.wasm for JS) and execute the source.
                    // For now, we return a mock result to allow testing the flow.
                    Value::String(format!(
                        "WASM execution result for {:?} script: {}",
                        language, source
                    ))
                }
                crate::runtime::microvm::core::Program::RtfsSource(source) => {
                    self.execute_simple_wasm_module(&source, &context)?
                }
                crate::runtime::microvm::core::Program::RtfsAst(_) => {
                    Value::String("WASM AST execution not yet implemented".to_string())
                }
                crate::runtime::microvm::core::Program::RtfsBytecode(_) => {
                    Value::String("WASM bytecode execution not yet implemented".to_string())
                }
                crate::runtime::microvm::core::Program::NativeFunction(_) => {
                    return Err(RuntimeError::Generic(
                        "Native functions not supported in WASM provider".to_string(),
                    ));
                }
                crate::runtime::microvm::core::Program::ExternalProgram { .. } => {
                    return Err(RuntimeError::Generic(
                        "External programs not supported in WASM provider".to_string(),
                    ));
                }
                crate::runtime::microvm::core::Program::Binary { language, source } => {
                    if *language == crate::runtime::microvm::core::ScriptLanguage::Wasm {
                        self.execute_wasm_binary(source, &context)?
                    } else {
                        return Err(RuntimeError::Generic(format!(
                            "WASM provider does not support binary language: {:?}",
                            language
                        )));
                    }
                }
            },
            None => Value::String("No program provided".to_string()),
        };

        let duration = start_time.elapsed();

        Ok(ExecutionResult {
            value: result_value,
            metadata: ExecutionMetadata {
                duration,
                memory_used_mb: 5, // Estimate for WASM execution
                cpu_time: duration,
                network_requests: vec![],
                file_operations: vec![],
            },
        })
    }

    fn execute_capability(&self, context: ExecutionContext) -> RuntimeResult<ExecutionResult> {
        if !self.initialized {
            return Err(RuntimeError::Generic(
                "WASM provider not initialized".to_string(),
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

        self.execute_program(context)
    }

    fn cleanup(&mut self) -> RuntimeResult<()> {
        self.initialized = false;
        Ok(())
    }

    fn get_config_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "wasm_memory_size": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 65536,
                    "default": 1024,
                    "description": "WASM memory size in pages (64KB each)"
                },
                "wasm_stack_size": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 1024,
                    "default": 64,
                    "description": "WASM stack size in pages"
                },
                "enable_wasm_threads": {
                    "type": "boolean",
                    "default": false,
                    "description": "Enable WASM threads support"
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::microvm::{ExecutionContext, Program};
    use crate::runtime::values::Value;

    #[test]
    fn test_wasm_execution() {
        let mut provider = WasmMicroVMProvider::new();
        provider.initialize().unwrap();

        // Simple WASM that prints "Hello from WASM"
        let wat = r#"
            (module
                (import "wasi_snapshot_preview1" "fd_write" (func $fd_write (param i32 i32 i32 i32) (result i32)))
                (memory 1)
                (export "memory" (memory 0))
                (data (i32.const 0) "Hello from WASM\n")
                (func $main (export "_start")
                    ;; iov.base = 0
                    (i32.store (i32.const 16) (i32.const 0))
                    ;; iov.len = 16
                    (i32.store (i32.const 20) (i32.const 16))
                    (call $fd_write
                        (i32.const 1) ;; stdout
                        (i32.const 16) ;; iovs ptr
                        (i32.const 1) ;; iovs len
                        (i32.const 24) ;; nwritten ptr
                    )
                    drop
                )
            )
        "#;
        let wasm_bytes = wat::parse_str(wat).unwrap();

        let context = ExecutionContext {
            execution_id: "test".to_string(),
            program: Some(Program::Binary {
                language: crate::runtime::microvm::core::ScriptLanguage::Wasm,
                source: wasm_bytes,
            }),
            capability_id: None,
            capability_permissions: vec![],
            args: vec![Value::String("test input".to_string())],
            config: Default::default(),
            runtime_context: None,
        };

        let result = provider.execute_program(context).unwrap();
        if let Value::String(s) = result.value {
            assert!(s.contains("Hello from WASM"));
        } else {
            // It might return the string directly if it's not valid JSON
            let output_str = format!("{:?}", result.value);
            assert!(output_str.contains("Hello from WASM"));
        }
    }
}
