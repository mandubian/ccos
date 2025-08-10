//! RTFS Runtime System
//! 
//! This module provides the core runtime for executing RTFS programs,
//! including value representation, execution context, and capability management.

pub mod rtfs_streaming_syntax;
pub use rtfs_streaming_syntax::{
    RtfsStreamingSyntaxExecutor, RtfsStreamingExpression, StreamReference, ProcessingLogic, StreamOptions, StreamSchema, ValidationRule, ErrorHandlingStrategy, BackpressureStrategy, MultiplexStrategy
};

// Runtime system for RTFS
// This module contains the evaluator, standard library, and runtime value system

pub mod capability;
pub mod streaming;
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
#[cfg(test)]
pub mod microvm_tests;
pub mod module_runtime;
pub mod stdlib;
pub mod secure_stdlib;
pub mod type_validator;  // Add type validator module
pub mod values;
pub mod security;

#[cfg(test)]
mod stdlib_tests;

pub use environment::{Environment, IrEnvironment};
pub use error::{RuntimeError, RuntimeResult};
pub use evaluator::Evaluator;
pub use ir_runtime::IrRuntime;
pub use module_runtime::{Module, ModuleRegistry};
pub use type_validator::{TypeValidator, ValidationError, ValidationResult};  // Export type validator
pub use values::{Function, Value};
pub use ccos_environment::{CCOSEnvironment, CCOSBuilder, SecurityLevel, CapabilityCategory};
pub use security::RuntimeContext;

use crate::ast::Expression;
use crate::parser;
use crate::runtime::ir_runtime::IrStrategy;
use crate::runtime::host::RuntimeHost;
use crate::runtime::capability_marketplace::CapabilityMarketplace;
use crate::runtime::capability_registry::CapabilityRegistry;
use crate::ccos::causal_chain::CausalChain;
use std::rc::Rc;
use crate::ccos::delegation::StaticDelegationEngine;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Trait for RTFS runtime operations needed by CCOS
/// This provides the interface for CCOS to interact with RTFS expressions
pub trait RTFSRuntime {
    /// Parse an RTFS expression string into a Value
    fn parse_expression(&mut self, source: &str) -> Result<Value, RuntimeError>;
    
    /// Convert a Value back to RTFS source code
    fn value_to_source(&self, value: &Value) -> Result<String, RuntimeError>;
    
    /// Evaluate an expression with access to CCOS context
    fn evaluate_with_ccos(&mut self, expression: &Value, ccos: &crate::ccos::CCOS) -> Result<Value, RuntimeError>;
}

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
}

impl RTFSRuntime for Runtime {
    fn parse_expression(&mut self, source: &str) -> Result<Value, RuntimeError> {
        let parsed = parser::parse(source)
            .map_err(|e| RuntimeError::Generic(format!("Parse error: {}", e)))?;
        
        // Convert TopLevel to Expression - this is a simplified approach
        // In a real implementation, you'd want to handle this more carefully
        if let Some(top_level) = parsed.first() {
            match top_level {
                crate::ast::TopLevel::Expression(expr) => self.run(expr),
                _ => Err(RuntimeError::Generic("Expected expression".to_string())),
            }
        } else {
            Err(RuntimeError::Generic("Empty parse result".to_string()))
        }
    }
    
    fn value_to_source(&self, value: &Value) -> Result<String, RuntimeError> {
        // Simple conversion back to source - in a real implementation this would be more sophisticated
        match value {
            Value::String(s) => Ok(format!("\"{}\"", s)),
            Value::Integer(i) => Ok(i.to_string()),
            Value::Float(f) => Ok(f.to_string()),
            Value::Boolean(b) => Ok(b.to_string()),
            Value::Nil => Ok("nil".to_string()),
            Value::Vector(v) => {
                let elements: Vec<String> = v.iter()
                    .map(|v| self.value_to_source(v))
                    .collect::<Result<Vec<String>, RuntimeError>>()?;
                Ok(format!("[{}]", elements.join(" ")))
            },
            Value::Map(m) => {
                let pairs: Vec<String> = m.iter()
                    .map(|(k, v)| {
                        let key = format!("{:?}", k);
                        let value = self.value_to_source(v)?;
                        Ok(format!("{} {}", key, value))
                    })
                    .collect::<Result<Vec<String>, RuntimeError>>()?;
                Ok(format!("{{{}}}", pairs.join(" ")))
            },
            _ => Ok(format!("{:?}", value)), // Fallback for complex types
        }
    }
    
    fn evaluate_with_ccos(&mut self, expression: &Value, _ccos: &crate::ccos::CCOS) -> Result<Value, RuntimeError> {
        // For now, just return the expression as-is
        // In a real implementation, this would evaluate the expression in the CCOS context
        Ok(expression.clone())
    }
}

impl Runtime {
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
                    Err(_ast_error) => {
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
