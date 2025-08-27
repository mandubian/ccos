//! RTFS Runtime System
//!
//! High-level runtime entry points and small helpers. The heavy logic is
//! implemented in the submodules listed below.

pub mod streaming;
pub mod capability_marketplace;
pub mod ccos_environment;
pub mod environment;
pub mod error;
pub mod evaluator;
pub mod host;
pub mod host_interface;
pub mod ir_runtime;
pub mod microvm;
pub mod module_runtime;
pub mod stdlib;
pub mod secure_stdlib;
pub mod type_validator;
pub mod values;
pub mod security;
pub mod param_binding;
#[cfg(feature = "metrics_exporter")]
pub mod metrics_exporter;
pub mod capabilities;

#[cfg(test)]
mod stdlib_tests;

pub use environment::{Environment, IrEnvironment};
pub use error::{RuntimeError, RuntimeResult};
pub use evaluator::Evaluator;
pub use ir_runtime::IrRuntime;
pub use module_runtime::{Module, ModuleRegistry};
pub use type_validator::{TypeValidator, ValidationError, ValidationResult};
pub use values::{Function, Value};
pub use ccos_environment::{CCOSEnvironment, CCOSBuilder, SecurityLevel, CapabilityCategory};
pub use security::RuntimeContext;

use crate::ast::{Expression, Literal, TopLevel, DoExpr};
use crate::parser;
use crate::runtime::ir_runtime::IrStrategy;
use crate::runtime::host::RuntimeHost;
use crate::runtime::capability_marketplace::CapabilityMarketplace;
use crate::runtime::capabilities::registry::CapabilityRegistry;
use crate::ccos::causal_chain::CausalChain;
use crate::ccos::delegation::StaticDelegationEngine;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Trait for RTFS runtime operations needed by CCOS
pub trait RTFSRuntime {
    fn parse_expression(&mut self, source: &str) -> Result<Value, RuntimeError>;
    fn value_to_source(&self, value: &Value) -> Result<String, RuntimeError>;
    fn evaluate_with_ccos(&mut self, expression: &Value, ccos: &crate::ccos::CCOS) -> Result<Value, RuntimeError>;
}

pub trait RuntimeStrategy: std::fmt::Debug + 'static {
    fn run(&mut self, program: &Expression) -> Result<Value, RuntimeError>;
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
    fn run(&mut self, program: &Expression) -> Result<Value, RuntimeError> {
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
        self.strategy.run(program)
    }

    pub fn new_with_tree_walking_strategy(module_registry: Arc<ModuleRegistry>) -> Self {
        let de = Arc::new(StaticDelegationEngine::new(HashMap::new()));
        let security_context = RuntimeContext::pure();

        let capability_registry = Arc::new(tokio::sync::RwLock::new(CapabilityRegistry::new()));
        let capability_marketplace = Arc::new(CapabilityMarketplace::new(capability_registry.clone()));
        let causal_chain = Arc::new(Mutex::new(CausalChain::new().expect("Failed to create causal chain")));

        let host = Arc::new(RuntimeHost::new(
            causal_chain,
            capability_marketplace,
            security_context.clone(),
        ));

        let evaluator = Evaluator::new(module_registry.clone(), de, security_context, host);
        let strategy = Box::new(TreeWalkingStrategy::new(evaluator));
        Self::new(strategy)
    }

    pub fn evaluate(&self, input: &str) -> Result<Value, RuntimeError> {
        let parsed = match parser::parse(input) {
            Ok(p) => p,
            Err(e) => return Err(RuntimeError::Generic(format!("Parse error: {:?}", e))),
        };
        let module_registry = ModuleRegistry::new();
        let de = Arc::new(StaticDelegationEngine::new(HashMap::new()));
        let security_context = RuntimeContext::pure();

        let capability_registry = Arc::new(tokio::sync::RwLock::new(CapabilityRegistry::new()));
        let capability_marketplace = Arc::new(CapabilityMarketplace::new(capability_registry.clone()));
        let causal_chain = Arc::new(Mutex::new(CausalChain::new().expect("Failed to create causal chain")));

        let host = Arc::new(RuntimeHost::new(
            causal_chain,
            capability_marketplace,
            security_context.clone(),
        ));

        let mut evaluator = Evaluator::new(Arc::new(module_registry), de, security_context, host);
        evaluator.eval_toplevel(&parsed)
    }

    pub fn evaluate_with_stdlib(&self, input: &str) -> Result<Value, RuntimeError> {
        let parsed = match parser::parse(input) {
            Ok(p) => p,
            Err(e) => return Err(RuntimeError::Generic(format!("Parse error: {:?}", e))),
        };
        let mut module_registry = ModuleRegistry::new();
        crate::runtime::stdlib::load_stdlib(&mut module_registry)?;
        let de = Arc::new(StaticDelegationEngine::new(HashMap::new()));
        let security_context = RuntimeContext::pure();

        let capability_registry = Arc::new(tokio::sync::RwLock::new(CapabilityRegistry::new()));
        let capability_marketplace = Arc::new(CapabilityMarketplace::new(capability_registry.clone()));
        let causal_chain = Arc::new(Mutex::new(CausalChain::new().expect("Failed to create causal chain")));

        let host = Arc::new(RuntimeHost::new(
            causal_chain,
            capability_marketplace,
            security_context.clone(),
        ));

        let mut evaluator = Evaluator::new(Arc::new(module_registry), de, security_context, host);
        evaluator.eval_toplevel(&parsed)
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
        let de = Arc::new(StaticDelegationEngine::new(HashMap::new()));
        let security_context = RuntimeContext::pure();

        let capability_registry = Arc::new(tokio::sync::RwLock::new(CapabilityRegistry::new()));
        let capability_marketplace = Arc::new(CapabilityMarketplace::new(capability_registry.clone()));
        let causal_chain = Arc::new(Mutex::new(CausalChain::new().expect("Failed to create causal chain")));

        let host = Arc::new(RuntimeHost::new(
            causal_chain,
            capability_marketplace,
            security_context.clone(),
        ));

        let evaluator = Evaluator::new(Arc::new(module_registry), de, security_context, host);
        let ast_strategy = TreeWalkingStrategy::new(evaluator);

        Self {
            ir_strategy,
            ast_strategy,
        }
    }
}

impl RuntimeStrategy for IrWithFallbackStrategy {
    fn run(&mut self, program: &Expression) -> Result<Value, RuntimeError> {
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
                other => Expression::Literal(Literal::String(format!("{:?}", other))),
            }
        } else {
            let exprs: Vec<Expression> = toplevels
                .into_iter()
                .map(|t| match t {
                    TopLevel::Expression(e) => e,
                    other => Expression::Literal(Literal::String(format!("{:?}", other))),
                })
                .collect();
            Expression::Do(DoExpr { expressions: exprs })
        };
        Ok(Value::from(expr))
    }

    fn value_to_source(&self, value: &Value) -> Result<String, RuntimeError> {
        Ok(value.to_string())
    }


    fn evaluate_with_ccos(&mut self, expression: &Value, _ccos: &crate::ccos::CCOS) -> Result<Value, RuntimeError> {
        // Temporary implementation: run using the current strategy and host already bound
        let expr = Expression::try_from(expression.clone())?;
        self.run(&expr)
    }
}
