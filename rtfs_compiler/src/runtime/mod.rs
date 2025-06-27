// Runtime system for RTFS
// This module contains the evaluator, standard library, and runtime value system

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
pub use values::{Value, Function};

use crate::ast::Expression;
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