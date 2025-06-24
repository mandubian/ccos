// Runtime system for RTFS
// This module contains the evaluator, standard library, and runtime value system

pub mod environment;
pub mod error;
pub mod evaluator;
pub mod ir_runtime;
pub mod module_loader;
pub mod standard_library;
pub mod values;

use crate::ast::TopLevel;
use crate::runtime::evaluator::Evaluator;
use crate::runtime::error::RuntimeError;
pub use crate::runtime::values::Value;

pub trait RuntimeStrategy {
    fn run(&self, program: &[TopLevel]) -> Result<Value, RuntimeError>;
}

pub struct Runtime {
    evaluator: Evaluator,
}

impl Runtime {
    pub fn new(strategy: Box<dyn RuntimeStrategy>) -> Self {
        // This needs to be properly initialized with a module registry
        unimplemented!();
    }

    pub fn run(&self, program: &[TopLevel]) -> Result<Value, RuntimeError> {
        // This will eventually use the strategy
        self.evaluator.eval_toplevel(program)
    }

    pub fn new_with_tree_walking_strategy() -> Self {
        // This needs to be properly initialized with a module registry
        unimplemented!();
    }
}
