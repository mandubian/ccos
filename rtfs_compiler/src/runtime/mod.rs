// Runtime system for RTFS
// This module contains the evaluator, standard library, and runtime value system

pub mod capability;
pub mod environment;
pub mod error;
pub mod evaluator;
pub mod ir_runtime;
pub mod module_runtime;
pub mod stdlib;
pub mod values;

#[cfg(test)]
mod stdlib_tests;

pub use environment::{Environment, IrEnvironment};
pub use error::{RuntimeError, RuntimeResult};
pub use evaluator::Evaluator;
pub use ir_runtime::IrRuntime;
pub use module_runtime::{Module, ModuleRegistry};
pub use values::{Function, Value};

use crate::ast::Expression;
use crate::runtime::ir_runtime::IrStrategy;
use std::rc::Rc;

#[derive(Clone, Debug, Copy)]
pub enum RuntimeStrategyValue {
    Ast,
    Ir,
    IrWithFallback,
}

pub trait RuntimeStrategy: std::fmt::Debug {
    fn run(&mut self, program: &Expression) -> Result<Value, RuntimeError>;
    fn clone_box(&self) -> Box<dyn RuntimeStrategy>;
}

impl Clone for Box<dyn RuntimeStrategy> {
    fn clone(&self) -> Self {
        self.clone_box()
    }
}

pub struct Runtime {
    strategy: Box<dyn RuntimeStrategy>,
}

impl Runtime {
    pub fn new(strategy: Box<dyn RuntimeStrategy>) -> Self {
        Self { strategy }
    }

    pub fn run(&mut self, program: &Expression) -> Result<Value, RuntimeError> {
        self.strategy.run(program)
    }

    pub fn new_with_tree_walking_strategy(module_registry: Rc<ModuleRegistry>) -> Self {
        let evaluator = Evaluator::new(module_registry);
        let strategy = Box::new(TreeWalkingStrategy::new(evaluator));
        Self::new(strategy)
    }
}

#[derive(Clone, Debug)]
pub struct TreeWalkingStrategy {
    evaluator: Evaluator,
}

impl TreeWalkingStrategy {
    pub fn new(evaluator: Evaluator) -> Self {
        Self { evaluator }
    }
}

impl RuntimeStrategy for TreeWalkingStrategy {
    fn run(&mut self, program: &Expression) -> Result<Value, RuntimeError> {
        self.evaluator.evaluate(program)
    }

    fn clone_box(&self) -> Box<dyn RuntimeStrategy> {
        Box::new(self.clone())
    }
}

/// Strategy that tries IR execution first, falls back to AST if IR fails
#[derive(Clone, Debug)]
pub struct IrWithFallbackStrategy {
    ir_strategy: IrStrategy,
    ast_strategy: TreeWalkingStrategy,
}

impl IrWithFallbackStrategy {
    pub fn new(module_registry: ModuleRegistry) -> Self {
        let ir_strategy = IrStrategy::new(module_registry.clone());
        let evaluator = Evaluator::new(Rc::new(module_registry));
        let ast_strategy = TreeWalkingStrategy::new(evaluator);

        Self {
            ir_strategy,
            ast_strategy,
        }
    }
}

impl RuntimeStrategy for IrWithFallbackStrategy {
    fn run(&mut self, program: &Expression) -> Result<Value, RuntimeError> {
        // Try IR execution first
        match self.ir_strategy.run(program) {
            Ok(result) => Ok(result),
            Err(ir_error) => {
                // If IR fails, fall back to AST execution
                match self.ast_strategy.run(program) {
                    Ok(result) => Ok(result),
                    Err(ast_error) => {
                        // If both fail, return the IR error (more specific)
                        Err(ir_error)
                    }
                }
            }
        }
    }

    fn clone_box(&self) -> Box<dyn RuntimeStrategy> {
        Box::new(self.clone())
    }
}
