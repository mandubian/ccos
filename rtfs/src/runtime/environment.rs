// Environment for variable bindings and scope management

use crate::ast::Symbol;
use crate::ir::core::NodeId;
use crate::runtime::error::RuntimeError;
use crate::runtime::module_runtime::ModuleRegistry;
use crate::runtime::values::Value;
use std::collections::HashMap;
use std::sync::Arc;

/// The runtime environment, which manages the scope chain for variable lookups for the AST evaluator.
#[derive(Debug, Clone)]
pub struct Environment {
    parent: Option<Arc<Environment>>,
    bindings: HashMap<Symbol, Value>,
}

impl Environment {
    /// Creates a new, empty global environment.
    pub fn new() -> Self {
        Environment {
            parent: None,
            bindings: HashMap::new(),
        }
    }

    /// Creates a new child environment that inherits from a parent.
    pub fn with_parent(parent: Arc<Environment>) -> Self {
        Environment {
            parent: Some(parent),
            bindings: HashMap::new(),
        }
    }

    /// Looks up a symbol by searching the current environment and then its parents.
    pub fn lookup(&self, name: &Symbol) -> Option<Value> {
        if let Some(value) = self.bindings.get(name) {
            Some(value.clone())
        } else if let Some(parent) = &self.parent {
            parent.lookup(name)
        } else {
            None
        }
    }

    /// Defines a new variable or updates an existing one in the current scope.
    pub fn define(&mut self, name: &Symbol, value: Value) {
        self.bindings.insert(name.clone(), value);
    }

    pub fn symbol_names(&self) -> Vec<String> {
        let mut names = self
            .bindings
            .keys()
            .map(|s| s.0.clone())
            .collect::<Vec<_>>();

        // Also collect names from parent environments to ensure we get all stdlib functions
        if let Some(parent) = &self.parent {
            let mut parent_names = parent.symbol_names();
            names.append(&mut parent_names);
        }

        // Remove duplicates while preserving current environment's bindings taking precedence
        names.sort();
        names.dedup();
        names
    }

    /// Find the name of a function value by searching through all bindings
    pub fn find_function_name(&self, func_value: &Value) -> Option<&str> {
        for (symbol, value) in &self.bindings {
            if value == func_value {
                return Some(&symbol.0);
            }
        }
        // Search parent environments
        if let Some(parent) = &self.parent {
            return parent.find_function_name(func_value);
        }
        None
    }
}

/// The runtime environment for the IR interpreter.
#[derive(Debug, Clone, Default)]
pub struct IrEnvironment {
    parent: Option<Arc<IrEnvironment>>,
    bindings: HashMap<String, Value>,
}

impl IrEnvironment {
    /// Creates a new, empty global environment.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a new environment with the standard library loaded.
    pub fn with_stdlib(module_registry: &ModuleRegistry) -> Result<Self, RuntimeError> {
        let mut env = Self::new();
        if let Some(stdlib_module) = module_registry.get_module("stdlib") {
            for (name, export) in stdlib_module
                .exports
                .read()
                .map_err(|e| RuntimeError::InternalError(format!("RwLock poisoned: {}", e)))?
                .iter()
            {
                env.define(name.clone(), export.value.clone());
            }
        }
        Ok(env)
    }

    pub fn with_parent(parent: Arc<IrEnvironment>) -> Self {
        IrEnvironment {
            parent: Some(parent),
            bindings: HashMap::new(),
        }
    }

    /// Creates a new child environment that inherits from a parent.
    pub fn new_child(&self) -> Self {
        IrEnvironment {
            parent: Some(Arc::new(self.clone())),
            bindings: HashMap::new(),
        }
    }

    /// Creates a new child environment for an IR function call, binding arguments to parameters.
    pub fn new_child_for_ir(
        &self,
        params: &[String],
        args: &[Value],
        is_variadic: bool,
    ) -> Result<Self, RuntimeError> {
        let mut new_env = self.new_child();
        if is_variadic {
            if args.len() < params.len() - 1 {
                return Err(RuntimeError::Generic(format!(
                    "Variadic function expected at least {} arguments, but got {}",
                    params.len() - 1,
                    args.len()
                )));
            }
            let (required_params, rest_param) = params.split_at(params.len() - 1);
            for (param, arg) in required_params.iter().zip(args.iter()) {
                new_env.define(param.clone(), arg.clone());
            }
            let rest_args = args[params.len() - 1..].to_vec();
            new_env.define(rest_param[0].clone(), Value::List(rest_args));
        } else {
            if params.len() != args.len() {
                return Err(RuntimeError::Generic(format!(
                    "Function expected {} arguments, but got {}",
                    params.len(),
                    args.len()
                )));
            }
            for (param, arg) in params.iter().zip(args.iter()) {
                new_env.define(param.clone(), arg.clone());
            }
        }
        Ok(new_env)
    }

    /// Looks up a symbol by searching the current environment and then its parents.
    pub fn get(&self, name: &str) -> Option<Value> {
        if let Some(value) = self.bindings.get(name) {
            Some(value.clone())
        } else if let Some(parent) = &self.parent {
            parent.get(name)
        } else {
            None
        }
    }

    /// Defines a new variable or updates an existing one in the current scope.
    pub fn define(&mut self, name: String, value: Value) {
        self.bindings.insert(name, value);
    }

    pub fn binding_count(&self) -> usize {
        self.bindings.len()
    }

    /// Get list of binding names for debugging
    pub fn binding_names(&self) -> Vec<String> {
        self.bindings.keys().cloned().collect()
    }

    /// Check if environment has parent
    pub fn has_parent(&self) -> bool {
        self.parent.is_some()
    }

    /// Get parent binding count
    pub fn parent_binding_count(&self) -> usize {
        self.parent.as_ref().map(|p| p.binding_count()).unwrap_or(0)
    }

    /// Defines a function in the IR environment. Currently an alias for `define`.
    pub fn define_ir_function(&mut self, name: String, _node_id: NodeId, value: Value) {
        self.define(name, value);
    }

    /// Find the name of a function value by searching through all bindings
    pub fn find_function_name(&self, func_value: &Value) -> Option<&str> {
        for (name, value) in &self.bindings {
            if value == func_value {
                return Some(name);
            }
        }
        // Search parent environments
        if let Some(parent) = &self.parent {
            return parent.find_function_name(func_value);
        }
        None
    }
}
