pub mod rtfs_streaming_syntax;
pub use rtfs_streaming_syntax::{
    RtfsStreamingSyntaxExecutor, RtfsStreamingExpression, StreamReference, ProcessingLogic, StreamOptions, StreamSchema, ValidationRule, ErrorHandlingStrategy, BackpressureStrategy, MultiplexStrategy
};
// Runtime system for RTFS
// This module contains the evaluator, standard library, and runtime value system

pub mod capability;
pub mod capability_registry;
pub mod capability_provider;
pub mod capability_marketplace;
pub mod ccos_environment;
pub mod environment;
pub mod error;
pub mod evaluator;
pub mod host;
pub mod host_interface;
pub mod ir_runtime;
pub mod microvm;
pub mod microvm_config;
pub mod module_runtime;
pub mod stdlib;
pub mod secure_stdlib;
pub mod values;
pub mod security;

#[cfg(test)]
mod stdlib_tests;

pub use environment::{Environment, IrEnvironment};
pub use error::{RuntimeError, RuntimeResult};
pub use evaluator::Evaluator;
pub use ir_runtime::IrRuntime;
pub use module_runtime::{Module, ModuleRegistry};
pub use values::{Function, Value};
pub use ccos_environment::{CCOSEnvironment, CCOSBuilder, SecurityLevel, CapabilityCategory};

use crate::ast::Expression;
use crate::parser;
use crate::runtime::ir_runtime::IrStrategy;
use crate::runtime::security::{RuntimeContext};
use crate::runtime::host::RuntimeHost;
use crate::runtime::capability_marketplace::CapabilityMarketplace;
use crate::runtime::capability_registry::CapabilityRegistry;
use crate::ccos::causal_chain::CausalChain;
use std::rc::Rc;
use crate::ccos::delegation::StaticDelegationEngine;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

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
        let de = Arc::new(StaticDelegationEngine::new(HashMap::new()));
        let security_context = RuntimeContext::pure();
        
        let capability_registry = Arc::new(tokio::sync::RwLock::new(CapabilityRegistry::new()));
        let capability_marketplace = Arc::new(CapabilityMarketplace::new(capability_registry.clone()));
        let causal_chain = Arc::new(Mutex::new(CausalChain::new().expect("Failed to create causal chain")));

        let host = Rc::new(RuntimeHost::new(
            causal_chain,
            capability_marketplace,
            security_context.clone(),
        ));
        
        let evaluator = Evaluator::new(module_registry, de, security_context, host);
        let strategy = Box::new(TreeWalkingStrategy::new(evaluator));
        Self::new(strategy)
    }

    pub fn evaluate(&self, input: &str) -> Result<Value, RuntimeError> {
        let parsed = parser::parse(input).expect("Failed to parse input");
        let module_registry = ModuleRegistry::new();
        let de = Arc::new(StaticDelegationEngine::new(HashMap::new()));
        let security_context = RuntimeContext::pure();
        
        let capability_registry = Arc::new(tokio::sync::RwLock::new(CapabilityRegistry::new()));
        let capability_marketplace = Arc::new(CapabilityMarketplace::new(capability_registry.clone()));
        let causal_chain = Arc::new(Mutex::new(CausalChain::new().expect("Failed to create causal chain")));

        let host = Rc::new(RuntimeHost::new(
            causal_chain,
            capability_marketplace,
            security_context.clone(),
        ));
        
        let mut evaluator = Evaluator::new(Rc::new(module_registry), de, security_context, host);
        evaluator.eval_toplevel(&parsed)
    }

    pub fn evaluate_with_stdlib(&self, input: &str) -> Result<Value, RuntimeError> {
        let parsed = parser::parse(input).expect("Failed to parse input");
        let mut module_registry = ModuleRegistry::new();
        crate::runtime::stdlib::load_stdlib(&mut module_registry)?;
        let de = Arc::new(StaticDelegationEngine::new(HashMap::new()));
        let security_context = RuntimeContext::pure();
        
        let capability_registry = Arc::new(tokio::sync::RwLock::new(CapabilityRegistry::new()));
        let capability_marketplace = Arc::new(CapabilityMarketplace::new(capability_registry.clone()));       
        let causal_chain = Arc::new(Mutex::new(CausalChain::new().expect("Failed to create causal chain")));

        let host = Rc::new(RuntimeHost::new(
            causal_chain,
            capability_marketplace,
            security_context.clone(),
        ));
        
        let mut evaluator = Evaluator::new(Rc::new(module_registry), de, security_context, host);
        evaluator.eval_toplevel(&parsed)
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
        let de = Arc::new(StaticDelegationEngine::new(HashMap::new()));
        let security_context = RuntimeContext::pure();
        
        let capability_registry = Arc::new(tokio::sync::RwLock::new(CapabilityRegistry::new()));
        let capability_marketplace = Arc::new(CapabilityMarketplace::new(capability_registry.clone()));
        let causal_chain = Arc::new(Mutex::new(CausalChain::new().expect("Failed to create causal chain")));

        let host = Rc::new(RuntimeHost::new(
            causal_chain,
            capability_marketplace,
            security_context.clone(),
        ));
        
        let evaluator = Evaluator::new(Rc::new(module_registry), de, security_context, host);
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
