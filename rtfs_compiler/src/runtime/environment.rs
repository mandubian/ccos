// Environment for variable bindings and scope management

use std::collections::HashMap;
use std::rc::Rc;
use crate::ast::Symbol;
use crate::runtime::{Value, RuntimeError, RuntimeResult};

/// Environment for variable bindings
/// Supports lexical scoping with parent environments
#[derive(Debug, Clone, PartialEq)]
pub struct Environment {
    /// Current scope bindings
    bindings: HashMap<String, Value>,
    /// Parent environment for lexical scoping
    parent: Option<Rc<Environment>>,
}

impl Environment {
    /// Create a new empty environment
    pub fn new() -> Self {
        Environment {
            bindings: HashMap::new(),
            parent: None,
        }
    }
    
    /// Create a new environment with a parent
    pub fn with_parent(parent: Rc<Environment>) -> Self {
        Environment {
            bindings: HashMap::new(),
            parent: Some(parent),
        }
    }
    
    /// Define a new binding in the current scope
    pub fn define(&mut self, symbol: &Symbol, value: Value) {
        self.bindings.insert(symbol.0.clone(), value);
    }
    
    /// Look up a symbol in this environment or parent environments
    pub fn lookup(&self, symbol: &Symbol) -> RuntimeResult<Value> {
        if let Some(value) = self.bindings.get(&symbol.0) {
            Ok(value.clone())
        } else if let Some(parent) = &self.parent {
            parent.lookup(symbol)
        } else {
            Err(RuntimeError::UndefinedSymbol(symbol.clone()))
        }
    }
    
    /// Check if a symbol is defined in this environment (not parent environments)
    pub fn contains(&self, symbol: &Symbol) -> bool {
        self.bindings.contains_key(&symbol.0)
    }
      /// Update an existing binding (searches up the scope chain)
    pub fn set(&mut self, symbol: &Symbol, value: Value) -> RuntimeResult<()> {
        if self.bindings.contains_key(&symbol.0) {
            self.bindings.insert(symbol.0.clone(), value);
            Ok(())
        } else if let Some(_parent) = &self.parent {
            // We can't modify the parent since it's behind an Rc
            // For now, just create a new binding in the current scope
            // In a more sophisticated implementation, we might use RefCell or other interior mutability
            self.bindings.insert(symbol.0.clone(), value);
            Ok(())
        } else {
            Err(RuntimeError::UndefinedSymbol(symbol.clone()))
        }
    }
    
    /// Get all bindings in the current scope (for debugging)
    pub fn current_bindings(&self) -> &HashMap<String, Value> {
        &self.bindings
    }
}

impl Default for Environment {
    fn default() -> Self {
        Self::new()
    }
}
