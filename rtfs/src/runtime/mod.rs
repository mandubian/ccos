//! RTFS Runtime System
//!
//! High-level runtime entry points and small helpers. The heavy logic is
//! implemented in the submodules listed below.

pub mod capabilities;
pub mod environment;
pub mod error;
pub mod evaluator;
pub mod execution_outcome;
pub mod host_interface;
pub mod ir_runtime;
pub mod microvm;
pub mod module_runtime;
pub mod param_binding;
pub mod pure_host;
pub mod secure_stdlib;
pub mod security;
pub mod stdlib;
pub mod stubs;
pub mod type_validator;
pub mod values;

#[cfg(test)]
mod stdlib_tests;

pub use capabilities::*;
pub use environment::{Environment, IrEnvironment};
pub use error::{RuntimeError, RuntimeResult};
pub use evaluator::Evaluator;
pub use execution_outcome::{CallMetadata, CausalContext, ExecutionOutcome, HostCall};
pub use ir_runtime::IrRuntime;
pub use ir_runtime::IrStrategy;
pub use module_runtime::{Module, ModuleRegistry};
pub use security::RuntimeContext;
pub use type_validator::{TypeValidator, ValidationError, ValidationResult};
pub use values::{Function, Value};

use crate::ast::{DoExpr, Expression, Literal, TopLevel};
use crate::parser;
// IrStrategy is re-exported below; avoid duplicate local import
use crate::runtime::pure_host::create_pure_host;
use std::sync::Arc;

/// Trait for RTFS runtime operations needed by CCOS
pub trait RTFSRuntime {
    fn parse_expression(&mut self, source: &str) -> Result<Value, RuntimeError>;
    fn value_to_source(&self, value: &Value) -> Result<String, RuntimeError>;
    /// Evaluate expression (CCOS integration method - optional)
    /// Note: Full CCOS integration available when RTFS is used with CCOS
    #[cfg(feature = "ccos-integration")]
    fn evaluate_with_ccos(
        &mut self,
        expression: &Value,
        _ccos: &(), // Placeholder for CCOS integration
    ) -> Result<Value, RuntimeError>;
}

pub trait RuntimeStrategy: std::fmt::Debug + 'static {
    fn run(&mut self, program: &Expression) -> Result<ExecutionOutcome, RuntimeError>;
    fn clone_box(&self) -> Box<dyn RuntimeStrategy>;
}

impl Clone for Box<dyn RuntimeStrategy> {
    fn clone(&self) -> Self {
        self.clone_box()
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
    fn run(&mut self, program: &Expression) -> Result<ExecutionOutcome, RuntimeError> {
        // Evaluator.evaluate now returns Result<ExecutionOutcome, RuntimeError>
        self.evaluator.evaluate(program)
    }

    fn clone_box(&self) -> Box<dyn RuntimeStrategy> {
        Box::new(TreeWalkingStrategy::new(self.evaluator.clone()))
    }
}

#[derive(Clone, Debug, Copy)]
pub enum RuntimeStrategyValue {
    Ast,
    Ir,
    IrWithFallback,
}

pub struct Runtime {
    strategy: Box<dyn RuntimeStrategy>,
}

impl Runtime {
    pub fn new(strategy: Box<dyn RuntimeStrategy>) -> Self {
        Self { strategy }
    }

    pub fn run(&mut self, program: &Expression) -> Result<Value, RuntimeError> {
        match self.strategy.run(program)? {
            ExecutionOutcome::Complete(value) => Ok(value),
            ExecutionOutcome::RequiresHost(host_call) => Err(RuntimeError::Generic(format!(
                "Host call required but not supported in this context: {:?}",
                host_call.capability_id
            ))),
        }
    }

    pub fn new_with_tree_walking_strategy(module_registry: Arc<ModuleRegistry>) -> Self {
        let security_context = RuntimeContext::pure();
        let host = create_pure_host();

        let evaluator = Evaluator::new(Arc::clone(&module_registry), security_context, host);
        let strategy = Box::new(TreeWalkingStrategy::new(evaluator));
        Self::new(strategy)
    }

    pub fn evaluate(&self, input: &str) -> Result<Value, RuntimeError> {
        let parsed = match parser::parse(input) {
            Ok(p) => p,
            Err(e) => return Err(RuntimeError::Generic(format!("Parse error: {}", e))),
        };
        let module_registry = ModuleRegistry::new();
        let security_context = RuntimeContext::pure();
        let host = create_pure_host();

        let mut evaluator = Evaluator::new(Arc::new(module_registry), security_context, host);
        match evaluator.eval_toplevel(&parsed) {
            Ok(ExecutionOutcome::Complete(v)) => Ok(v),
            Ok(ExecutionOutcome::RequiresHost(hc)) => Err(RuntimeError::Generic(format!(
                "Host call required: {}",
                hc.capability_id
            ))),
            #[cfg(feature = "effect-boundary")]
            Ok(ExecutionOutcome::RequiresHost(_)) => Err(RuntimeError::Generic(
                "Host call required but not supported in this context".to_string(),
            )),
            Err(e) => Err(e),
        }
    }

    pub fn evaluate_with_stdlib(&self, input: &str) -> Result<Value, RuntimeError> {
        let parsed = match parser::parse(input) {
            Ok(p) => p,
            Err(e) => return Err(RuntimeError::Generic(format!("Parse error: {}", e))),
        };
        let module_registry = ModuleRegistry::new();
        crate::runtime::stdlib::load_stdlib(&module_registry)?;
        let security_context = RuntimeContext::pure();
        let host = create_pure_host();

        let mut evaluator = Evaluator::new(Arc::new(module_registry), security_context, host);
        match evaluator.eval_toplevel(&parsed) {
            Ok(ExecutionOutcome::Complete(v)) => Ok(v),
            Ok(ExecutionOutcome::RequiresHost(hc)) => Err(RuntimeError::Generic(format!(
                "Host call required: {}",
                hc.capability_id
            ))),
            #[cfg(feature = "effect-boundary")]
            Ok(ExecutionOutcome::RequiresHost(_)) => Err(RuntimeError::Generic(
                "Host call required but not supported in this context".to_string(),
            )),
            Err(e) => Err(e),
        }
    }
}

/// Strategy that tries IR execution first, falls back to AST if IR fails
#[derive(Clone, Debug)]
pub struct IrWithFallbackStrategy {
    ir_strategy: IrStrategy,
    ast_strategy: TreeWalkingStrategy,
}

impl IrWithFallbackStrategy {
    pub fn new(module_registry: Arc<ModuleRegistry>) -> Self {
        let ir_strategy = IrStrategy::new(Arc::clone(&module_registry));
        let security_context = RuntimeContext::pure();
        let host = create_pure_host();

        let evaluator = Evaluator::new(Arc::clone(&module_registry), security_context, host);
        let ast_strategy = TreeWalkingStrategy::new(evaluator);

        Self {
            ir_strategy,
            ast_strategy,
        }
    }
}

impl RuntimeStrategy for IrWithFallbackStrategy {
    fn run(&mut self, program: &Expression) -> Result<ExecutionOutcome, RuntimeError> {
        match self.ir_strategy.run(program) {
            Ok(result) => Ok(result),
            Err(ir_error) => match self.ast_strategy.run(program) {
                Ok(result) => Ok(result),
                Err(_) => Err(ir_error),
            },
        }
    }

    fn clone_box(&self) -> Box<dyn RuntimeStrategy> {
        Box::new(self.clone())
    }
}

impl RTFSRuntime for Runtime {
    fn parse_expression(&mut self, source: &str) -> Result<Value, RuntimeError> {
        let mut toplevels = parser::parse(source)
            .map_err(|e| RuntimeError::Generic(format!("Parse error: {:?}", e)))?;
        let expr: Expression = if toplevels.len() == 1 {
            match toplevels.remove(0) {
                TopLevel::Expression(e) => e,
                other => Expression::Literal(Literal::String(
                    serde_json::to_string(&other).unwrap_or_else(|_| format!("{:?}", other)),
                )),
            }
        } else {
            let exprs: Vec<Expression> = toplevels
                .into_iter()
                .map(|t| match t {
                    TopLevel::Expression(e) => e,
                    other => Expression::Literal(Literal::String(
                        serde_json::to_string(&other).unwrap_or_else(|_| format!("{:?}", other)),
                    )),
                })
                .collect();
            Expression::Do(DoExpr { expressions: exprs })
        };
        Ok(Value::from(expr))
    }

    fn value_to_source(&self, value: &Value) -> Result<String, RuntimeError> {
        Ok(value.to_string())
    }

    #[cfg(feature = "ccos-integration")]
    fn evaluate_with_ccos(
        &mut self,
        expression: &Value,
        _ccos: &(), // Placeholder for CCOS integration
    ) -> Result<Value, RuntimeError> {
        // Temporary implementation: run using the current strategy and host already bound
        let expr = Expression::try_from(expression.clone())?;
        match self.run(&expr) {
            Ok(outcome) => match outcome {
                ExecutionOutcome::Complete(value) => Ok(value),
                _ => Err(RuntimeError::NotImplemented(
                    "Non-complete outcomes not yet handled".to_string(),
                )),
            },
            Err(e) => Err(e),
        }
    }
}
