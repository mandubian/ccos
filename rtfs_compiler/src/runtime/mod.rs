// Runtime system for RTFS
// This module contains the evaluator, standard library, and runtime value system

pub mod evaluator;
pub mod stdlib;
pub mod values;
pub mod environment;
pub mod error;
pub mod ir_runtime;
pub mod module_runtime;

pub use evaluator::Evaluator;
pub use values::Value;
pub use environment::Environment;
pub use error::{RuntimeError, RuntimeResult};

/// Runtime execution strategy
#[derive(Debug, Clone)]
pub enum RuntimeStrategy {
    /// Use AST-based evaluator (stable, compatible)
    Ast,
    /// Use IR-based runtime (high performance)
    Ir,
    /// Use IR with AST fallback for unsupported features
    IrWithFallback,
}

impl Default for RuntimeStrategy {
    fn default() -> Self {
        RuntimeStrategy::Ast // Keep AST as default for now
    }
}

/// Main runtime coordinator that can switch between AST and IR execution
pub struct Runtime {
    strategy: RuntimeStrategy,
    ast_evaluator: evaluator::Evaluator,
    ir_runtime: Option<ir_runtime::IrRuntime>,
}

impl Runtime {
    pub fn new() -> Self {
        Runtime {
            strategy: RuntimeStrategy::default(),
            ast_evaluator: evaluator::Evaluator::new(),
            ir_runtime: Some(ir_runtime::IrRuntime::new()),
        }
    }
    
    pub fn with_strategy(strategy: RuntimeStrategy) -> Self {
        Runtime {
            strategy,
            ast_evaluator: evaluator::Evaluator::new(),
            ir_runtime: Some(ir_runtime::IrRuntime::new()),
        }
    }
    
    pub fn evaluate_expression(&mut self, expr: &crate::ast::Expression) -> RuntimeResult<Value> {
        match self.strategy {
            RuntimeStrategy::Ast => {
                self.ast_evaluator.evaluate(expr)
            }
            RuntimeStrategy::Ir => {
                if let Some(ir_runtime) = &mut self.ir_runtime {
                    // Convert AST to IR then execute
                    let mut converter = crate::ir_converter::IrConverter::new();
                    match converter.convert(expr) {
                        Ok(ir_node) => {
                            let mut env = ir_runtime::IrEnvironment::new();
                            ir_runtime.execute_node(&ir_node, &mut env)
                        },
                        Err(_) => self.ast_evaluator.evaluate(expr) // Fallback to AST
                    }
                } else {
                    self.ast_evaluator.evaluate(expr)
                }
            }
            RuntimeStrategy::IrWithFallback => {
                // Try IR first, fallback to AST on any issues
                if let Some(ir_runtime) = &mut self.ir_runtime {
                    let mut converter = crate::ir_converter::IrConverter::new();
                    match converter.convert(expr) {                        Ok(ir_node) => {
                            let mut env = ir_runtime::IrEnvironment::new();
                            match ir_runtime.execute_node(&ir_node, &mut env) {
                                Ok(result) => Ok(result),
                                Err(_) => self.ast_evaluator.evaluate(expr) // Fallback
                            }
                        }
                        Err(_) => self.ast_evaluator.evaluate(expr) // Fallback
                    }
                } else {
                    self.ast_evaluator.evaluate(expr)
                }
            }
        }
    }
}
