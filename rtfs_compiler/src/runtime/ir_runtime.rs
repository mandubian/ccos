// IR Runtime - Efficient execution engine for typed RTFS IR
// This runtime leverages type information and pre-resolved bindings for performance

use std::rc::Rc;
use std::cell::RefCell;
use std::collections::HashMap;
use crate::ir::*;
use crate::runtime::{Value, RuntimeError, RuntimeResult, Environment};
use crate::runtime::values::{Function, Arity};
use crate::runtime::stdlib::StandardLibrary;
use crate::runtime::module_runtime::{ModuleRegistry, CompiledModule, ModuleMetadata};
use crate::ast::{Keyword, MapKey};

/// IR-based runtime executor
pub struct IrRuntime {
    global_env: Rc<Environment>,
    node_cache: HashMap<NodeId, Value>, // Cache for pure expressions
    call_stack: Vec<CallFrame>,
    module_registry: ModuleRegistry,
    recursion_depth: usize,
    max_recursion_depth: usize,
    task_context: Option<Value>,
}

/// Call frame for debugging and error reporting
#[derive(Debug, Clone)]
pub struct CallFrame {
    pub node_id: NodeId,
    pub function_name: Option<String>,
    pub source_location: Option<SourceLocation>,
}

/// Represents a tail call that needs to be executed.
#[derive(Debug, Clone)]
pub struct TailCall {
    pub function: Value,
    pub args: Vec<Value>,
}

/// Optimized environment that uses pre-resolved binding IDs
#[derive(Debug, Clone)]
pub struct IrEnvironment {
    bindings: HashMap<NodeId, Value>, // Keyed by binding node ID, not name
    parent: Option<Rc<IrEnvironment>>,
}

impl IrEnvironment {
    pub fn new() -> Self {
        IrEnvironment {
            bindings: HashMap::new(),
            parent: None,
        }
    }
    
    pub fn with_parent(parent: Rc<IrEnvironment>) -> Self {
        IrEnvironment {
            bindings: HashMap::new(),
            parent: Some(parent),
        }
    }
    
    pub fn define(&mut self, binding_id: NodeId, value: Value) {
        self.bindings.insert(binding_id, value);
    }
    
    pub fn lookup(&self, binding_id: NodeId) -> Option<&Value> {
        self.bindings.get(&binding_id).or_else(|| {
            self.parent.as_ref().and_then(|p| p.lookup(binding_id))
        })
    }
    
    pub fn lookup_local(&self, binding_id: NodeId) -> Option<&Value> {
        self.bindings.get(&binding_id)
    }
    
    pub fn update(&mut self, binding_id: NodeId, value: Value) -> bool {
        if self.bindings.contains_key(&binding_id) {
            self.bindings.insert(binding_id, value);
            true
        } else {
            false
        }
    }
    
    /// Get the number of bindings in the current scope (used for generating unique IDs)
    pub fn binding_count(&self) -> usize {
        self.bindings.len()
    }
}

impl IrRuntime {    /// Create a new IR runtime with standard library
    pub fn new() -> Self {
        let global_env = StandardLibrary::create_global_environment();
        IrRuntime {
            global_env: Rc::new(global_env),
            node_cache: HashMap::new(),
            call_stack: Vec::new(),
            module_registry: ModuleRegistry::new(),
            recursion_depth: 0,
            max_recursion_depth: 10_000, // Safety limit
            task_context: None,
        }
    }
    
    pub fn set_task_context(&mut self, context: Value) {
        self.task_context = Some(context);
    }

    pub fn module_registry_mut(&mut self) -> &mut ModuleRegistry {
        &mut self.module_registry
    }

    pub fn add_module_path(&mut self, path: std::path::PathBuf) {
        self.module_registry.add_module_path(path);
    }

    /// Execute an IR program
    pub fn execute_program(&mut self, program: &IrNode) -> RuntimeResult<Value> {
        match program {
            IrNode::Program { forms, .. } => {
                let mut last_value = Value::Nil;
                for form in forms {
                    last_value = self.execute_node(form, &mut IrEnvironment::new(), false)?;
                }
                Ok(last_value)
            }
            _ => Err(RuntimeError::InvalidProgram("Expected Program node".to_string())),
        }
    }
    
    /// Execute a single IR node
    pub fn execute_node(&mut self, node: &IrNode, env: &mut IrEnvironment, is_tail: bool) -> RuntimeResult<Value> {
        // Check cache for pure expressions
        if self.is_pure_expression(node) {
            if let Some(cached_value) = self.node_cache.get(&node.id()) {
                return Ok(cached_value.clone());
            }
        }
        
        let result = self.execute_node_uncached(node, env, is_tail)?;
        
        // Cache pure expressions
        if self.is_pure_expression(node) {
            self.node_cache.insert(node.id(), result.clone());
        }
        
        Ok(result)
    }
    
    /// Execute node without caching
    fn execute_node_uncached(&mut self, node: &IrNode, env: &mut IrEnvironment, is_tail: bool) -> RuntimeResult<Value> {
        match node {
            IrNode::Literal { value, .. } => self.execute_literal(value),
            
            IrNode::Vector { elements, .. } => {
                self.execute_vector(elements, env)
            }
            
            IrNode::Map { entries, .. } => {
                self.execute_map(entries, env)
            }
            
            IrNode::VariableRef { binding_id, name, .. } => {
                match env.lookup(*binding_id) {
                    Some(value) => {
                        // If we find a placeholder, resolve it. This is key for letrec.
                        if let Value::FunctionPlaceholder(placeholder) = value {
                            Ok(placeholder.borrow().clone())
                        } else {
                            Ok(value.clone())
                        }
                    },
                    None => {
                        // Check if it's a qualified symbol (e.g., "module/symbol")
                        if ModuleRegistry::is_qualified_symbol(name) {
                            // Resolve through module registry
                            self.module_registry.resolve_qualified_symbol(name)
                        } else {
                            // Fallback to global environment lookup by name
                            let global_env = Environment::with_parent(self.global_env.clone());
                            global_env.lookup(&crate::ast::Symbol(name.clone()))
                        }
                    }
                }
            }
            
            IrNode::Apply { function, arguments, .. } => {
                self.execute_apply(function, arguments, env, is_tail)
            }
            
            IrNode::If { condition, then_branch, else_branch, .. } => {
                self.execute_if(condition, then_branch, else_branch.as_deref(), env, is_tail)
            }
            
            IrNode::Let { bindings, body, .. } => {
                self.execute_let(bindings, body, env, is_tail)
            }
            
            IrNode::Do { expressions, .. } => {
                self.execute_do(expressions, env, is_tail)
            }
            
            IrNode::Lambda { params, body, captures, .. } => {
                self.execute_lambda(params, body, captures, env)
            }
            
            IrNode::Match { expression, clauses, .. } => {
                self.execute_match(expression, clauses, env)
            }
            
            IrNode::TryCatch { try_body, catch_clauses, finally_body, .. } => {
                self.execute_try_catch(try_body, catch_clauses, finally_body.as_deref(), env)
            }
            
            IrNode::Parallel { bindings, .. } => {
                self.execute_parallel(bindings, env)
            }
            
            IrNode::WithResource { binding, init_expr, body, .. } => {
                self.execute_with_resource(binding, init_expr, body, env)
            }
            
            IrNode::LogStep { level, values, location, .. } => {
                self.execute_log_step(level, values, location.as_deref(), env)
            }
            
            IrNode::TaskContextAccess { field_name, .. } => {
                self.execute_task_context_access(field_name)
            }
            
            IrNode::FunctionDef { name: _name, lambda, .. } => {
                let function_value = self.execute_node(lambda, env, false)?;
                env.define(node.id(), function_value.clone());
                Ok(function_value)
            }            IrNode::VariableDef { name: _name, init_expr, .. } => {
                let value = self.execute_node(init_expr, env, false)?;
                env.define(node.id(), value.clone());
                Ok(value)
            }
            
            IrNode::Module { name, exports, definitions, .. } => {
                self.execute_module(name, exports, definitions, env)
            }
            
            IrNode::Import { module_name, alias, imports, .. } => {
                self.execute_import(module_name, alias.as_deref(), imports.as_ref(), env)
            }
            
            _ => {
                Err(RuntimeError::NotImplemented(format!("IR node type not implemented: {:?}", node)))
            }
        }
    }
    
    /// Execute a literal value
    fn execute_literal(&self, literal: &crate::ast::Literal) -> RuntimeResult<Value> {
        match literal {
            crate::ast::Literal::Integer(n) => Ok(Value::Integer(*n)),
            crate::ast::Literal::Float(f) => Ok(Value::Float(*f)),
            crate::ast::Literal::String(s) => Ok(Value::String(s.clone())),
            crate::ast::Literal::Boolean(b) => Ok(Value::Boolean(*b)),
            crate::ast::Literal::Keyword(k) => Ok(Value::Keyword(k.clone())),
            crate::ast::Literal::Nil => Ok(Value::Nil),
        }
    }

    /// Execute vector creation
    fn execute_vector(&mut self, elements: &[IrNode], env: &mut IrEnvironment) -> RuntimeResult<Value> {
        let mut values = Vec::new();
        for element in elements {
            values.push(self.execute_node(element, env, false)?);
        }
        Ok(Value::Vector(values))
    }

    /// Execute map creation
    fn execute_map(&mut self, entries: &[crate::ir::IrMapEntry], env: &mut IrEnvironment) -> RuntimeResult<Value> {
        let mut map = std::collections::HashMap::new();
        for entry in entries {
            let key_value = self.execute_node(&entry.key, env, false)?;
            let value_value = self.execute_node(&entry.value, env, false)?;
            
            // Convert key to MapKey
            let map_key = match key_value {
                Value::String(s) => crate::ast::MapKey::String(s),
                Value::Keyword(k) => crate::ast::MapKey::Keyword(k),
                Value::Integer(i) => crate::ast::MapKey::String(i.to_string()), // Convert to string for now
                _ => return Err(RuntimeError::TypeError {
                    expected: "string, keyword, or integer".to_string(),
                    actual: key_value.type_name().to_string(),
                    operation: "map key".to_string(),
                }),
            };
            
            map.insert(map_key, value_value);
        }
        Ok(Value::Map(map))
    }

    /// Execute function application with optimized dispatch
    fn execute_apply(&mut self, function: &IrNode, arguments: &[IrNode], env: &mut IrEnvironment, is_tail: bool) -> RuntimeResult<Value> {
        let func_value = self.execute_node(function, env, false)?;
        let mut arg_values = Vec::new();
        
        for arg in arguments {
            arg_values.push(self.execute_node(arg, env, false)?);
        }

        if is_tail {
            return Err(RuntimeError::TailCall {
                function: func_value,
                args: arg_values,
            });
        }
        
        // Add call frame for debugging
        self.call_stack.push(CallFrame {
            node_id: function.id(),
            function_name: None, // TODO: Extract function name from IR
            source_location: function.source_location().cloned(),
        });
        
        let result = self.call_function(func_value, &arg_values, env);
        self.call_stack.pop();
        result
    }

    /// Call a function value (similar to AST runtime but with IR context)
    fn call_function(&mut self, func: Value, args: &[Value], env: &mut IrEnvironment) -> RuntimeResult<Value> {
        match func {
            Value::Function(Function::Builtin { name, func, arity, .. }) => {
                self.check_arity(&arity, args.len())?;
                
                // Special handling for higher-order functions that need runtime context
                match name.as_str() {
                    "map" => self.builtin_map(args, env),
                    "map-fn" => self.builtin_map(args, env),  // map-fn is an alias for map
                    "filter" => self.builtin_filter(args, env),
                    "reduce" => self.builtin_reduce(args, env),
                    _ => func(args)  // Regular built-in functions
                }
            }
            Value::Function(Function::UserDefined { params, body, closure, .. }) => {
                self.call_user_function(params, None, body, closure, args, env)
            }
            Value::Function(Function::IrLambda { params, variadic_param, body, closure_env }) => {
                self.call_ir_lambda(params, variadic_param, body, *closure_env, args.to_vec(), env)
            }
            Value::FunctionPlaceholder(placeholder) => {
                // Resolve the placeholder to get the actual function value
                let resolved_func = placeholder.borrow().clone();
                match resolved_func {
                    Value::Nil => {
                        // Placeholder hasn't been resolved yet
                        Err(RuntimeError::InternalError("Function placeholder not resolved".to_string()))
                    }
                    _ => {
                        // Recursively call with the resolved function
                        self.call_function(resolved_func, args, env)
                    }
                }
            }
            Value::Keyword(keyword) => {
                // Keywords act as functions: (:key map) is equivalent to (get map :key)
                if args.len() == 1 {
                    match &args[0] {
                        Value::Map(map) => {
                            let map_key = crate::ast::MapKey::Keyword(keyword);
                            Ok(map.get(&map_key).cloned().unwrap_or(Value::Nil))
                        },
                        _ => Err(RuntimeError::TypeError {
                            expected: "map".to_string(),
                            actual: args[0].type_name().to_string(),
                            operation: "keyword lookup".to_string(),
                        }),
                    }
                } else if args.len() == 2 {
                    // (:key map default) is equivalent to (get map :key default)
                    match &args[0] {
                        Value::Map(map) => {
                            let map_key = crate::ast::MapKey::Keyword(keyword);
                            Ok(map.get(&map_key).cloned().unwrap_or(args[1].clone()))
                        },
                        _ => Err(RuntimeError::TypeError {
                            expected: "map".to_string(),
                            actual: args[0].type_name().to_string(),
                            operation: "keyword lookup".to_string(),
                        }),
                    }
                } else {
                    Err(RuntimeError::ArityMismatch {
                        function: format!(":{}", keyword.0),
                        expected: "1 or 2".to_string(),
                        actual: args.len(),
                    })
                }
            },
            _ => Err(RuntimeError::NotCallable(format!("{:?}", func))),
        }
    }
    
    /// Call user-defined function with IR environment
    fn call_user_function(
        &mut self,
        params: Vec<crate::ast::ParamDef>,
        _variadic_param: Option<crate::ast::ParamDef>,
        body: Vec<crate::ast::Expression>,
        _closure: Environment,
        args: &[Value],
        _env: &mut IrEnvironment,
    ) -> RuntimeResult<Value> {
        // Create new environment for function scope
        let mut func_env = IrEnvironment::new();
        
        // Bind parameters - simplified for now
        for (i, param) in params.iter().enumerate() {
            if let Some(arg_value) = args.get(i) {
                // For now, only handle simple parameter binding
                if let crate::ast::Pattern::Symbol(_sym) = &param.pattern {
                    // In full implementation, would use parameter binding ID
                    // For now, using a placeholder approach
                    func_env.define(i as NodeId + 1000, arg_value.clone());
                }
            }
        }
        
        // Execute function body - would need to convert AST to IR first
        // This is a simplified placeholder
        Ok(Value::Nil)
    }
    
    /// Call IR lambda function with tail call optimization for both direct and mutual recursion
    fn call_ir_lambda(
        &mut self,
        mut params: Vec<IrNode>,
        _variadic_param: Option<Box<IrNode>>,
        mut body: Vec<IrNode>,
        mut closure_env: IrEnvironment,
        mut args: Vec<Value>,
        _current_env: &mut IrEnvironment,
    ) -> RuntimeResult<Value> {
        'tco: loop {
            // Check recursion depth
            self.recursion_depth += 1;
            if self.recursion_depth > self.max_recursion_depth {
                return Err(RuntimeError::StackOverflow(format!(
                    "Maximum recursion depth {} exceeded",
                    self.max_recursion_depth
                )));
            }

            // Create function environment from closure
            let mut func_env = IrEnvironment::with_parent(Rc::new(closure_env));
            
            // Bind parameters to arguments
            for (i, param) in params.iter().enumerate() {
                if let Some(arg_value) = args.get(i) {
                    if let IrNode::Param { binding, .. } = param {
                        if let IrNode::VariableBinding { id, .. } = binding.as_ref() {
                            func_env.define(*id, arg_value.clone());
                        }
                    }
                }
            }
            
            // Execute function body with tail call optimization
            let mut result = Value::Nil;
            let body_len = body.len();
            for (i, expr) in body.iter().enumerate() {
                let is_tail = i == body_len - 1;
                match self.execute_node(expr, &mut func_env, is_tail) {
                    Ok(val) => result = val,
                    Err(RuntimeError::TailCall { function, args: next_args }) => {
                        match function {
                            Value::Function(Function::IrLambda { 
                                params: next_params, 
                                variadic_param: _next_variadic,
                                body: next_body, 
                                closure_env: next_closure_env 
                            }) => {
                                // This is a tail call to another IR lambda. We can optimize.
                                params = next_params;
                                body = next_body;
                                closure_env = *next_closure_env;
                                args = next_args;
                                
                                self.recursion_depth -= 1; // we are not making a new stack frame
                                continue 'tco;
                            }
                            _ => {
                                // Tail call to a builtin or other function type. Cannot optimize with the loop.
                                self.recursion_depth -= 1;
                                return self.call_function(function, &next_args, &mut func_env);
                            }
                        }
                    }
                    Err(e) => return Err(e),
                }
            }
            
            self.recursion_depth -= 1;
            return Ok(result);
        }
    }

    /// Execute if expression with type-aware short-circuiting
    fn execute_if(
        &mut self,
        condition: &IrNode,
        then_branch: &IrNode,
        else_branch: Option<&IrNode>,
        env: &mut IrEnvironment,
        is_tail: bool,
    ) -> RuntimeResult<Value> {
        let cond_val = self.execute_node(condition, env, false)?;
        if cond_val.is_truthy() {
            self.execute_node(then_branch, env, is_tail)
        } else if let Some(else_branch) = else_branch {
            self.execute_node(else_branch, env, is_tail)
        } else {
            Ok(Value::Nil)
        }
    }

    fn execute_let(
        &mut self,
        bindings: &[IrLetBinding],
        body: &[IrNode],
        env: &mut IrEnvironment,
        is_tail: bool,
    ) -> RuntimeResult<Value> {
        // Implements letrec for functions and let* for values.
        let mut local_env = IrEnvironment::with_parent(Rc::new(env.clone()));
        let mut placeholders = HashMap::new();

        // Pass 1: Create placeholders for function bindings to enable mutual recursion.
        for binding in bindings {
            if let IrNode::Lambda { .. } = &binding.init_expr {
                if let IrNode::VariableBinding { id, .. } = &binding.pattern {
                    let placeholder = Rc::new(RefCell::new(Value::Nil));
                    local_env.define(*id, Value::FunctionPlaceholder(placeholder.clone()));
                    placeholders.insert(*id, placeholder);
                }
            }
        }

        // Pass 2: Evaluate all bindings.
        for binding in bindings {
            if let IrNode::VariableBinding { id, .. } = &binding.pattern {
                // The binding's value is evaluated in the environment that includes placeholders.
                let value = self.execute_node(&binding.init_expr, &mut local_env, false)?;
                
                if let Some(placeholder) = placeholders.get(id) {
                    // If it was a function, we now have the real Function value.
                    // Update the placeholder that other functions in this let block hold a reference to.
                    *placeholder.borrow_mut() = value;
                } else {
                    // If it was a simple value, just define it in the local environment.
                    // This provides let* semantics for values.
                    local_env.define(*id, value);
                }
            } else {
                return Err(RuntimeError::InternalError(format!(
                    "Unsupported pattern in let binding: {:?}",
                    binding.pattern
                )));
            }
        }

        // Execute the body in the fully populated local environment.
        let mut result = Value::Nil;
        let body_len = body.len();
        for (i, expr) in body.iter().enumerate() {
            let is_last = i == body_len - 1;
            result = self.execute_node(expr, &mut local_env, is_tail && is_last)?;
        }

        Ok(result)
    }

    fn execute_do(&mut self, expressions: &[IrNode], env: &mut IrEnvironment, is_tail: bool) -> RuntimeResult<Value> {
        let mut result = Value::Nil;
        let expressions_len = expressions.len();
        for (i, expr) in expressions.iter().enumerate() {
            let expr_is_tail = is_tail && (i == expressions_len - 1);
            result = self.execute_node(expr, env, expr_is_tail)?;
        }
        Ok(result)
    }
    
    /// Execute lambda creation
    fn execute_lambda(
        &mut self,
        params: &[IrNode],
        body: &[IrNode],
        _captures: &[IrCapture], // Captures are implicitly the passed 'env'
        env: &mut IrEnvironment,
    ) -> RuntimeResult<Value> {
        // The closure environment is a clone of the environment where the lambda is defined.
        // This is crucial for letrec to work, as the env will contain the placeholders.
        let closure = env.clone();

        let func = Function::IrLambda {
            params: params.to_vec(),
            variadic_param: None, // TODO: Implement variadic params
            body: body.to_vec(),
            closure_env: Box::new(closure),
        };
        
        Ok(Value::Function(func))
    }    /// Execute a module definition
    fn execute_module(&mut self, name: &str, exports: &[String], definitions: &[IrNode], _env: &mut IrEnvironment) -> RuntimeResult<Value> {
        let mut module_env = IrEnvironment::new();
        
        let def_count = definitions.len();
        for (i, def) in definitions.iter().enumerate() {
            let expr_is_tail = i == def_count - 1;
            self.execute_node(def, &mut module_env, expr_is_tail)?;
        }
        
        // Register module
        let compiled_module = CompiledModule {
            metadata: ModuleMetadata {
                name: name.to_string(),
                docstring: None,
                source_file: None,
                version: None,
                compiled_at: std::time::SystemTime::now(),
            },
            ir_node: IrNode::Module {
                id: 0, // TODO: Proper ID
                name: name.to_string(),
                exports: exports.to_vec(),
                definitions: definitions.to_vec(),
                source_location: None,
            },
            exports: HashMap::new(), // This should be populated correctly
            namespace: Rc::new(module_env),
            dependencies: Vec::new(),
        };
        self.module_registry.register_module(compiled_module)?;
        
        Ok(Value::Nil)
    }
    
    /// Execute an import statement
    fn execute_import(&mut self, module_name: &str, alias: Option<&str>, imports: Option<&Vec<String>>, env: &mut IrEnvironment) -> RuntimeResult<Value> {
        // For now, we'll implement a basic mock import system
        // In a full implementation, this would:
        // 1. Load the module from the registry
        // 2. Import the specified symbols
        // 3. Handle aliasing and qualified names
        
        println!("Importing from module '{}' with alias {:?} and imports {:?}", 
                 module_name, alias, imports);
        
        // Create mock symbols for demonstration
        match module_name {
            "rtfs.core.string" => {
                // Mock string utilities
                if let Some(import_list) = imports {
                    for symbol_name in import_list {
                        match symbol_name.as_str() {
                            "length" => {
                                // Create a mock string length function
                                let mock_value = Value::Function(crate::runtime::values::Function::Builtin {
                                    name: "string/length".to_string(),
                                    arity: crate::runtime::values::Arity::Exact(1),
                                    func: |args| {
                                        if let Some(Value::String(s)) = args.get(0) {
                                            Ok(Value::Integer(s.len() as i64))                                        } else {
                                            Err(RuntimeError::TypeError {
                                                expected: "string".to_string(),
                                                actual: args.get(0).map_or("nil".to_string(), |v| v.type_name().to_string()),
                                                operation: "string/length".to_string(),
                                            })
                                        }
                                    }
                                });
                                // Use a unique ID for the import binding
                                let binding_id = 1000 + symbol_name.len() as u64; // Simple ID generation
                                env.define(binding_id, mock_value);
                            }
                            _ => {
                                // Mock other string functions
                                let mock_value = Value::Nil;
                                let binding_id = 1000 + symbol_name.len() as u64;
                                env.define(binding_id, mock_value);
                            }
                        }
                    }
                }
            }
            _ => {
                // Mock other modules by doing nothing for now
                println!("Warning: Mock import for module '{}' - no symbols loaded", module_name);
            }
        }
        
        Ok(Value::Nil) // Import doesn't return a value
    }
    
    // Placeholder implementations for remaining methods
    fn execute_match(&mut self, _expression: &IrNode, _clauses: &[IrMatchClause], _env: &mut IrEnvironment) -> RuntimeResult<Value> {
        // TODO: Implement pattern matching
        Ok(Value::Nil)
    }
    
    fn execute_try_catch(
        &mut self,
        _try_body: &[IrNode],
        _catch_clauses: &[IrCatchClause],
        _finally_body: Option<&[IrNode]>,
        _env: &mut IrEnvironment,
    ) -> RuntimeResult<Value> {
        // TODO: Implement try-catch
        Ok(Value::Nil)
    }
    
    fn execute_parallel(&mut self, _bindings: &[IrParallelBinding], _env: &mut IrEnvironment) -> RuntimeResult<Value> {
        // TODO: Implement parallel execution
        Ok(Value::Nil)
    }
    
    fn execute_with_resource(
        &mut self,
        _binding: &IrNode,
        _init_expr: &IrNode,
        _body: &[IrNode],
        _env: &mut IrEnvironment,
    ) -> RuntimeResult<Value> {
        // TODO: Implement resource management
        Ok(Value::Nil)
    }
    
    fn execute_log_step(&mut self, level: &Keyword, values: &[IrNode], location: Option<&str>, env: &mut IrEnvironment) -> RuntimeResult<Value> {
        let mut log_values = Vec::new();
        for value_node in values {
            log_values.push(self.execute_node(value_node, env, false)?);
        }
        
        let message = log_values.iter().map(|v| v.to_string()).collect::<Vec<String>>().join(" ");
        let location_info = location.map_or("".to_string(), |loc| format!(" at {}", loc));
        
        println!("[{}] {}{}", level.0.to_uppercase(), message, location_info);
        
        Ok(Value::Nil)
    }
    
    fn execute_task_context_access(&mut self, field_name: &Keyword) -> RuntimeResult<Value> {
        match &self.task_context {
            Some(Value::Map(context_map)) => {
                let key = MapKey::Keyword(field_name.clone());
                Ok(context_map.get(&key).cloned().unwrap_or(Value::Nil))
            }
            Some(_) => Err(RuntimeError::TypeError {
                expected: "map".to_string(),
                actual: self.task_context.as_ref().unwrap().type_name().to_string(),
                operation: "task context access".to_string(),
            }),
            None => Ok(Value::Nil), // Return Nil if no context is set
        }
    }
    
    /// Check if an expression is pure (has no side effects)
    fn is_pure_expression(&self, node: &IrNode) -> bool {
        // A more sophisticated implementation would analyze the expression tree
        matches!(node, IrNode::Literal { .. } | IrNode::Vector { .. } | IrNode::Map { .. })
    }

    fn check_arity(&self, arity: &Arity, num_args: usize) -> RuntimeResult<()> {
        match arity {
            Arity::Exact(n) => {
                if *n != num_args {
                    return Err(RuntimeError::ArityMismatch {
                        function: "<builtin>".to_string(),
                        expected: n.to_string(),
                        actual: num_args,
                    });
                }
            }
            Arity::AtLeast(min) => {
                if num_args < *min {
                    return Err(RuntimeError::ArityMismatch {
                        function: "<builtin>".to_string(),
                        expected: format!("at least {}", min),
                        actual: num_args,
                    });
                }
            }
            Arity::Range(min, max) => {
                if num_args < *min || num_args > *max {
                    return Err(RuntimeError::ArityMismatch {
                        function: "<builtin>".to_string(),
                        expected: format!("between {} and {}", min, max),
                        actual: num_args,
                    });
                }
            }
            Arity::Any => {}
        }
        Ok(())
    }

    fn builtin_map(&mut self, args: &[Value], env: &mut IrEnvironment) -> RuntimeResult<Value> {
        if args.len() != 2 {
            return Err(RuntimeError::ArityMismatch { 
                function: "map".to_string(), 
                expected: "2".to_string(), 
                actual: args.len() 
            });
        }

        let func = &args[0];
        let list = &args[1];

        if let Value::Vector(vec) = list {
            let mut result_vec = Vec::new();
            for item in vec {
                let result = self.call_function(func.clone(), &[item.clone()], env)?;
                result_vec.push(result);
            }
            Ok(Value::Vector(result_vec))
        } else {
            Err(RuntimeError::TypeError { 
                expected: "vector".to_string(), 
                actual: list.type_name().to_string(), 
                operation: "map".to_string() 
            })
        }
    }

    fn builtin_filter(&mut self, args: &[Value], env: &mut IrEnvironment) -> RuntimeResult<Value> {
        if args.len() != 2 {
            return Err(RuntimeError::ArityMismatch { 
                function: "filter".to_string(), 
                expected: "2".to_string(), 
                actual: args.len() 
            });
        }

        let func = &args[0];
        let list = &args[1];

        if let Value::Vector(vec) = list {
            let mut result_vec = Vec::new();
            for item in vec {
                let result = self.call_function(func.clone(), &[item.clone()], env)?;
                if result.is_truthy() {
                    result_vec.push(item.clone());
                }
            }
            Ok(Value::Vector(result_vec))
        } else {
            Err(RuntimeError::TypeError { 
                expected: "vector".to_string(), 
                actual: list.type_name().to_string(), 
                operation: "filter".to_string() 
            })
        }
    }

    fn builtin_reduce(&mut self, args: &[Value], env: &mut IrEnvironment) -> RuntimeResult<Value> {
        if args.len() < 2 || args.len() > 3 {
            return Err(RuntimeError::ArityMismatch {
                function: "reduce".to_string(),
                expected: "2 or 3".to_string(),
                actual: args.len(),
            });
        }

        let func = &args[0];
        let (initial_value, list) = if args.len() == 3 {
            (Some(args[1].clone()), &args[2])
        } else {
            (None, &args[1])
        };

        if let Value::Vector(vec) = list {
            if vec.is_empty() {
                return Ok(initial_value.unwrap_or(Value::Nil));
            }

            let mut accumulator;
            let start_index;

            if let Some(init) = initial_value {
                accumulator = init;
                start_index = 0;
            } else {
                accumulator = vec[0].clone();
                start_index = 1;
            }

            for item in &vec[start_index..] {
                accumulator = self.call_function(func.clone(), &[accumulator, item.clone()], env)?;
            }
            Ok(accumulator)
        } else {
            Err(RuntimeError::TypeError {
                expected: "vector".to_string(),
                actual: list.type_name().to_string(),
                operation: "reduce".to_string(),
            })
        }
    }
}

impl RuntimeError {
    pub fn with_call_stack(self, _call_stack: &[CallFrame]) -> Self {
        // Enhanced error reporting with call stack
        // TODO: Implement enhanced error types
        self
    }

    /// Create a stack overflow error
    pub fn stack_overflow(message: String) -> Self {
        RuntimeError::InternalError(format!("Stack overflow: {}", message))
    }
}
