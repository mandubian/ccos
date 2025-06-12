// IR Runtime - Efficient execution engine for typed RTFS IR
// This runtime leverages type information and pre-resolved bindings for performance

use std::collections::HashMap;
use std::rc::Rc;
use crate::ir::*;
use crate::runtime::{Value, RuntimeError, RuntimeResult, Environment};
use crate::runtime::values::{Function, Arity, ResourceHandle, ResourceState, ErrorValue};
use crate::runtime::stdlib::StandardLibrary;
use crate::ast::{Keyword, MapKey};

/// IR-based runtime executor
pub struct IrRuntime {
    global_env: Rc<Environment>,
    node_cache: HashMap<NodeId, Value>, // Cache for pure expressions
    call_stack: Vec<CallFrame>,
}

/// Call frame for debugging and error reporting
#[derive(Debug, Clone)]
pub struct CallFrame {
    pub node_id: NodeId,
    pub function_name: Option<String>,
    pub source_location: Option<SourceLocation>,
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
    
    pub fn update(&mut self, binding_id: NodeId, value: Value) -> bool {
        if self.bindings.contains_key(&binding_id) {
            self.bindings.insert(binding_id, value);
            true
        } else {
            false
        }
    }
}

impl IrRuntime {
    /// Create a new IR runtime with standard library
    pub fn new() -> Self {
        let global_env = StandardLibrary::create_global_environment();
        IrRuntime {
            global_env: Rc::new(global_env),
            node_cache: HashMap::new(),
            call_stack: Vec::new(),
        }
    }
    
    /// Execute an IR program
    pub fn execute_program(&mut self, program: &IrNode) -> RuntimeResult<Value> {
        match program {
            IrNode::Program { forms, .. } => {
                let mut last_value = Value::Nil;
                for form in forms {
                    last_value = self.execute_node(form, &mut IrEnvironment::new())?;
                }
                Ok(last_value)
            }
            _ => Err(RuntimeError::InvalidProgram("Expected Program node".to_string())),
        }
    }
    
    /// Execute a single IR node
    pub fn execute_node(&mut self, node: &IrNode, env: &mut IrEnvironment) -> RuntimeResult<Value> {
        // Check cache for pure expressions
        if self.is_pure_expression(node) {
            if let Some(cached_value) = self.node_cache.get(&node.id()) {
                return Ok(cached_value.clone());
            }
        }
        
        let result = self.execute_node_uncached(node, env)?;
        
        // Cache pure expressions
        if self.is_pure_expression(node) {
            self.node_cache.insert(node.id(), result.clone());
        }
        
        Ok(result)
    }
    
    /// Execute node without caching
    fn execute_node_uncached(&mut self, node: &IrNode, env: &mut IrEnvironment) -> RuntimeResult<Value> {
        match node {
            IrNode::Literal { value, .. } => self.execute_literal(value),
            
            IrNode::VariableRef { binding_id, name, .. } => {
                match env.lookup(*binding_id) {
                    Some(value) => Ok(value.clone()),
                    None => {
                        // Fallback to global environment lookup by name
                        let mut global_env = Environment::with_parent(self.global_env.clone());
                        global_env.lookup(&crate::ast::Symbol(name.clone()))
                    }
                }
            }
            
            IrNode::Apply { function, arguments, .. } => {
                self.execute_apply(function, arguments, env)
            }
            
            IrNode::If { condition, then_branch, else_branch, .. } => {
                self.execute_if(condition, then_branch, else_branch.as_deref(), env)
            }
            
            IrNode::Let { bindings, body, .. } => {
                self.execute_let(bindings, body, env)
            }
            
            IrNode::Do { expressions, .. } => {
                self.execute_do(expressions, env)
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
            
            IrNode::FunctionDef { name, lambda, .. } => {
                let function_value = self.execute_node(lambda, env)?;
                env.define(node.id(), function_value.clone());
                Ok(function_value)
            }
            
            IrNode::VariableDef { name, init_expr, .. } => {
                let value = self.execute_node(init_expr, env)?;
                env.define(node.id(), value.clone());
                Ok(value)
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
    
    /// Execute function application with optimized dispatch
    fn execute_apply(&mut self, function: &IrNode, arguments: &[IrNode], env: &mut IrEnvironment) -> RuntimeResult<Value> {
        // Add call frame for debugging
        self.call_stack.push(CallFrame {
            node_id: function.id(),
            function_name: None, // TODO: Extract function name from IR
            source_location: function.source_location().cloned(),
        });
        
        let func_value = self.execute_node(function, env)?;
        let mut arg_values = Vec::new();
        
        for arg in arguments {
            arg_values.push(self.execute_node(arg, env)?);
        }
        
        let result = self.call_function(func_value, &arg_values, env);
        self.call_stack.pop();
        result
    }
    
    /// Call a function value (similar to AST runtime but with IR context)
    fn call_function(&mut self, func: Value, args: &[Value], env: &mut IrEnvironment) -> RuntimeResult<Value> {
        match func {
            Value::Function(Function::Builtin { func, arity, .. }) => {
                self.check_arity(&arity, args.len())?;
                func(args)
            }
            Value::Function(Function::UserDefined { params, body, closure, .. }) => {
                self.call_user_function(params, None, body, closure, args, env)
            }
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
                if let crate::ast::Pattern::Symbol(sym) = &param.pattern {
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
    
    /// Execute if expression with type-aware short-circuiting
    fn execute_if(
        &mut self,
        condition: &IrNode,
        then_branch: &IrNode,
        else_branch: Option<&IrNode>,
        env: &mut IrEnvironment,
    ) -> RuntimeResult<Value> {
        let condition_value = self.execute_node(condition, env)?;
        
        if condition_value.is_truthy() {
            self.execute_node(then_branch, env)
        } else if let Some(else_node) = else_branch {
            self.execute_node(else_node, env)
        } else {
            Ok(Value::Nil)
        }
    }
    
    /// Execute let binding with optimized scoping
    fn execute_let(&mut self, bindings: &[IrLetBinding], body: &[IrNode], env: &mut IrEnvironment) -> RuntimeResult<Value> {
        // Create new environment for let scope
        let mut let_env = IrEnvironment::with_parent(Rc::new(env.clone()));
        
        // Process bindings in order
        for binding in bindings {
            let value = self.execute_node(&binding.init_expr, &mut let_env)?;
            
            // Bind the value using the pattern's binding ID
            if let IrNode::VariableBinding { id, .. } = &binding.pattern {
                let_env.define(*id, value);
            }
            // TODO: Handle complex destructuring patterns
        }
        
        // Execute body
        let mut result = Value::Nil;
        for expr in body {
            result = self.execute_node(expr, &mut let_env)?;
        }
        
        Ok(result)
    }
    
    /// Execute do block
    fn execute_do(&mut self, expressions: &[IrNode], env: &mut IrEnvironment) -> RuntimeResult<Value> {
        let mut result = Value::Nil;
        for expr in expressions {
            result = self.execute_node(expr, env)?;
        }
        Ok(result)
    }
    
    /// Execute lambda creation with closure capture
    fn execute_lambda(
        &mut self,
        _params: &[IrNode],
        _body: &[IrNode],
        captures: &[IrCapture],
        env: &mut IrEnvironment,
    ) -> RuntimeResult<Value> {        // Capture free variables from current environment
        let mut captured_env = Environment::new();
        for capture in captures {
            if let Some(value) = env.lookup(capture.binding_id) {
                captured_env.define(&crate::ast::Symbol(capture.name.clone()), value.clone());
            }
        }
        
        // Create user-defined function - simplified for now
        let func = Function::UserDefined {
            params: vec![], // Would convert from IR params
            variadic_param: None,
            body: vec![], // Would convert from IR body
            closure: captured_env,
        };
        
        Ok(Value::Function(func))
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
    
    fn execute_log_step(
        &mut self,
        level: &Keyword,
        values: &[IrNode],
        location: Option<&str>,
        env: &mut IrEnvironment,
    ) -> RuntimeResult<Value> {
        // Execute log step
        let mut log_values = Vec::new();
        for value_node in values {
            log_values.push(self.execute_node(value_node, env)?);
        }
        
        // Simple logging implementation
        println!("[{}] {}: {:?}", 
            level.0,
            location.unwrap_or("unknown"),
            log_values
        );
        
        Ok(Value::Nil)
    }
    
    fn execute_task_context_access(&self, field_name: &Keyword) -> RuntimeResult<Value> {
        // TODO: Implement task context access
        // For now, return nil
        println!("Task context access: @{}", field_name.0);
        Ok(Value::Nil)
    }
    
    /// Check if an expression is pure (no side effects) for caching
    fn is_pure_expression(&self, node: &IrNode) -> bool {
        match node {
            IrNode::Literal { .. } => true,
            IrNode::VariableRef { .. } => true, // Assuming immutable bindings
            IrNode::Apply { function, arguments, .. } => {
                // Check if function is pure and all arguments are pure
                self.is_pure_expression(function) && arguments.iter().all(|arg| self.is_pure_expression(arg))
            }
            IrNode::If { condition, then_branch, else_branch, .. } => {
                self.is_pure_expression(condition) 
                && self.is_pure_expression(then_branch)
                && else_branch.as_ref().map_or(true, |e| self.is_pure_expression(e))
            }
            // Most other constructs have side effects or create bindings
            _ => false,
        }
    }
      /// Check function arity
    fn check_arity(&self, arity: &Arity, provided: usize) -> RuntimeResult<()> {
        match arity {
            Arity::Exact(expected) => {
                if provided != *expected {
                    Err(RuntimeError::ArityMismatch {
                        function: "unknown".to_string(),
                        expected: expected.to_string(),
                        actual: provided,
                    })
                } else {
                    Ok(())
                }
            }
            Arity::AtLeast(min) => {
                if provided < *min {
                    Err(RuntimeError::ArityMismatch {
                        function: "unknown".to_string(),
                        expected: format!("at least {}", min),
                        actual: provided,
                    })
                } else {
                    Ok(())
                }
            }
            Arity::Range(min, max) => {
                if provided < *min || provided > *max {
                    Err(RuntimeError::ArityMismatch {
                        function: "unknown".to_string(),
                        expected: format!("{}-{}", min, max),
                        actual: provided,
                    })
                } else {
                    Ok(())
                }
            }
            Arity::Any => Ok(()),
        }
    }
    
    /// Get current call stack for debugging
    pub fn call_stack(&self) -> &[CallFrame] {
        &self.call_stack
    }
}

/// Runtime error extensions for IR runtime
impl RuntimeError {    pub fn with_call_stack(self, _call_stack: &[CallFrame]) -> Self {
        // Enhanced error reporting with call stack
        // TODO: Implement enhanced error types
        self
    }
}
