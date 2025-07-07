use crate::ir::core::IrNode;
use wasmtime::{Engine, Module, Store, Instance, Val};
use crate::runtime::{RuntimeError, RuntimeResult, Value};
use std::sync::Arc;

/// Trait for pluggable back-ends that turn RTFS IR modules into machine-executable bytecode.
/// Returns a raw byte vector suitable for publishing to the L4 cache.
pub trait BytecodeBackend: Send + Sync + std::fmt::Debug {
    /// Compile an IR module to bytecode.
    fn compile_module(&self, ir: &IrNode) -> Vec<u8>;

    /// Target identifier (e.g. "wasm32", "rtfs-bc").
    fn target_id(&self) -> &'static str;
}

/// Very thin stub that returns a valid empty WebAssembly module (\0asm header only)
/// so we can move real bytes through the pipeline without yet implementing full code-gen.
#[derive(Debug, Default, Clone)]
pub struct WasmBackend;

impl BytecodeBackend for WasmBackend {
    fn compile_module(&self, _ir: &IrNode) -> Vec<u8> {
        // Minimal WASM binary: magic + version = 8 bytes
        vec![0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00]
    }

    fn target_id(&self) -> &'static str {
        "wasm32"
    }
}

/// Trait for executing previously compiled bytecode.
/// Implementations load the bytecode into a VM / runtime and invoke the requested function.
pub trait BytecodeExecutor: Send + Sync + std::fmt::Debug {
    /// Execute a function in the provided bytecode module.
    ///
    /// * `bytecode` – raw bytes of the module
    /// * `fn_name` – exported symbol to invoke
    /// * `args` – RTFS runtime arguments to pass (conversion is backend-specific)
    fn execute_module(
        &self,
        bytecode: &[u8],
        fn_name: &str,
        args: &[crate::runtime::Value],
    ) -> crate::runtime::RuntimeResult<crate::runtime::Value>;

    /// Target identifier (must match the backend that produced the bytecode)
    fn target_id(&self) -> &'static str;
}

/// A very thin placeholder executor that accepts WASM modules but doesn't actually run them yet.
/// It simply returns Nil until a real engine (e.g. wasmtime) is wired in.
#[derive(Clone)]
pub struct WasmExecutor {
    engine: Arc<Engine>,
}

impl std::fmt::Debug for WasmExecutor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "WasmExecutor")
    }
}

impl WasmExecutor {
    pub fn new() -> Self {
        Self {
            engine: Arc::new(Engine::default()),
        }
    }
}

// Utility to simplify float conversions
fn f64_to_bits(f: f64) -> u64 { f.to_bits() }
fn bits_to_f64(b: u64) -> f64 { f64::from_bits(b) }
fn f32_to_bits(f: f64) -> u32 { (f as f32).to_bits() }
fn bits_to_f32(b: u32) -> f64 { f32::from_bits(b) as f64 }

impl BytecodeExecutor for WasmExecutor {
    fn execute_module(
        &self,
        bytecode: &[u8],
        fn_name: &str,
        args: &[Value],
    ) -> RuntimeResult<Value> {
        // Compile module
        let module = Module::from_binary(&self.engine, bytecode).map_err(|e| RuntimeError::Generic(format!("Wasm load error: {e}")))?;
        // Create a store (empty context)
        let mut store = Store::new(&self.engine, ());
        let instance = Instance::new(&mut store, &module, &[]).map_err(|e| RuntimeError::Generic(format!("Wasm instantiate error: {e}")))?;
        let func = instance.get_func(&mut store, fn_name).ok_or_else(|| RuntimeError::Generic(format!("Function '{}' not found in module", fn_name)))?;
        // Map arguments
        let wasm_args: Vec<Val> = args
            .iter()
            .filter_map(|v| match v {
                Value::Integer(i) => Some(Val::I64(*i)),
                Value::Float(f) => Some(Val::F64(f64_to_bits(*f))),
                Value::Nil => None,
                _ => None, // Unsupported for now
            })
            .collect();

        // Prepare results slice
        let result_count = func.ty(&store).results().len();
        let mut results = vec![Val::I32(0); result_count];

        func.call(&mut store, &wasm_args, &mut results)
            .map_err(|e| RuntimeError::Generic(format!("Wasm call error: {e}")))?;

        // Return first result if present
        if let Some(val) = results.first() {
            match val {
                Val::I32(i) => Ok(Value::Integer(*i as i64)),
                Val::I64(i) => Ok(Value::Integer(*i)),
                Val::F32(bits) => Ok(Value::Float(bits_to_f32(*bits))),
                Val::F64(bits) => Ok(Value::Float(bits_to_f64(*bits))),
                _ => Err(RuntimeError::Generic("Unsupported return type from Wasm".to_string())),
            }
        } else {
            Ok(Value::Nil)
        }
    }

    fn target_id(&self) -> &'static str {
        "wasm32"
    }
} 