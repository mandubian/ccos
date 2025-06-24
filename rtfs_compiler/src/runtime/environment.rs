// Environment for variable bindings and scope management

use crate::ast::{Symbol, Expression, Literal};
use std::collections::HashMap;
use std::rc::Rc;
use std::cell::RefCell;

/// Represents a single scope in the environment chain.
/// Each scope contains a map of symbols to their bound expressions.
#[derive(Debug, Default, Clone)]
pub struct Scope {
    bindings: HashMap<Symbol, Rc<Expression>>,
}

impl Scope {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, name: &Symbol) -> Option<Rc<Expression>> {
        self.bindings.get(name).cloned()
    }

    pub fn set(&mut self, name: Symbol, value: Rc<Expression>) {
        self.bindings.insert(name, value);
    }
}

/// The runtime environment, which manages the scope chain for variable lookups.
/// It uses a vector of `Scope`s to represent the nested lexical scopes.
#[derive(Debug, Clone)]
pub struct Environment {
    scopes: Vec<Rc<RefCell<Scope>>>,
}

impl Environment {
    pub fn new() -> Self {
        Self {
            scopes: vec![Rc::new(RefCell::new(Scope::new()))],
        }
    }

    pub fn new_with_builtins() -> Self {
        let env = Environment::new();
        // TODO: Add built-in functions to the root scope
        env
    }

    pub fn get(&self, name: &Symbol) -> Option<Rc<Expression>> {
        for scope in self.scopes.iter().rev() {
            if let Some(value) = scope.borrow().get(name) {
                return Some(value);
            }
        }
        None
    }

    pub fn set(&self, name: Symbol, value: Rc<Expression>) {
        // In a language with immutable bindings by default, `set` should either
        // update a binding in the current scope or create a new one.
        // For simplicity, we're allowing mutable bindings in the current scope.
        self.scopes
            .last()
            .expect("Environment should always have at least one scope")
            .borrow_mut()
            .set(name, value);
    }

    pub fn push_scope(&mut self) {
        self.scopes.push(Rc::new(RefCell::new(Scope::new())));
    }

    pub fn pop_scope(&mut self) {
        if self.scopes.len() > 1 {
            self.scopes.pop();
        } else {
            // Handle error or warning: cannot pop the global scope
        }
    }

    pub fn current_scope(&self) -> Rc<RefCell<Scope>> {
        self.scopes.last().unwrap().clone()
    }
}
