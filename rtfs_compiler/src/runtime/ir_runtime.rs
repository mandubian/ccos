// IR Runtime - Efficient execution engine for typed RTFS IR
// This runtime leverages type information and pre-resolved bindings for performance

use std::rc::Rc;
use std::cell::RefCell;
use std::collections::HashMap;
use crate::ir::*;
use crate::runtime::{Value, RuntimeError, RuntimeResult};
use crate::runtime::environment::IrEnvironment;
use crate::runtime::module_runtime::{ModuleRegistry};
use crate::runtime::values::{Function, Arity};
use crate::ast::{self, Keyword, MapKey};

/// Represents a single frame in the call stack for debugging.
#[derive(Debug, Clone)]
pub struct CallFrame {
    pub node_id: NodeId,
    pub function_name: Option<String>,
    pub source_location: Option<SourceLocation>,
}

/// Runtime for executing IR
#[derive(Debug)]
pub struct IrRuntime {
    /// Environment for storing bindings
    environment: IrEnvironment,
    /// Node cache for memoizing pure function calls
    node_cache: HashMap<NodeId, Value>,
    /// Call stack for debugging
    call_stack: Vec<CallFrame>,
    /// Recursion depth counter to prevent stack overflow
    recursion_depth: u32,
    /// Maximum recursion depth
    max_recursion_depth: u32,
}

impl IrRuntime {
    /// Create a new IR runtime
    pub fn new() -> Self {
        IrRuntime {
            environment: IrEnvironment::new(),
            node_cache: HashMap::new(),
            call_stack: Vec::new(),
            recursion_depth: 0,
            max_recursion_depth: 1000, // Default max recursion depth
        }
    }

    /// Execute a program
    pub fn execute_program(&mut self, program: &IrNode, module_registry: &ModuleRegistry) -> RuntimeResult<Value> {
        match program {
            IrNode::Program { forms, .. } => {
                let mut last_value = Value::Nil;
                for form in forms {
                    last_value = self.execute_node(form, &mut IrEnvironment::new(), false, module_registry)?;
                }
                Ok(last_value)
            }
            _ => Err(RuntimeError::InvalidProgram("Expected Program node".to_string())),
        }
    }
    
    /// Execute a single IR node
    pub fn execute_node(&mut self, node: &IrNode, env: &mut IrEnvironment, is_tail: bool, module_registry: &ModuleRegistry) -> RuntimeResult<Value> {
        // Check cache for pure expressions
        if self.is_pure_expression(node) {
            if let Some(cached_value) = self.node_cache.get(&node.id()) {
                return Ok(cached_value.clone());
            }
        }
        
        let result = self.execute_node_uncached(node, env, is_tail, module_registry)?;
        
        // Cache pure expressions
        if self.is_pure_expression(node) {
            self.node_cache.insert(node.id(), result.clone());
        }
        
        Ok(result)
    }
    
    /// Execute node without caching
    fn execute_node_uncached(&mut self, node: &IrNode, env: &mut IrEnvironment, is_tail: bool, module_registry: &ModuleRegistry) -> RuntimeResult<Value> {
        match node {
            IrNode::Literal { value, .. } => self.execute_literal(value),
            
            IrNode::Vector { elements, .. } => {
                self.execute_vector(elements, env, module_registry)
            }
            
            IrNode::Map { entries, .. } => {
                self.execute_map(entries, env, module_registry)
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
                            module_registry.resolve_qualified_symbol(name)
                        } else {
                            // Fallback to global environment lookup by name
                            Err(RuntimeError::UndefinedSymbol(ast::Symbol(name.clone())))
                        }
                    }
                }
            }
            
            IrNode::QualifiedSymbolRef { module, symbol, .. } => {
                let qualified_name = format!("{}/{}", module, symbol);
                module_registry.resolve_qualified_symbol(&qualified_name)
            }
            
            IrNode::Apply { function, arguments, .. } => {
                self.execute_apply(function, arguments, env, is_tail, module_registry)
            }
            
            IrNode::If { condition, then_branch, else_branch, .. } => {
                self.execute_if(condition, then_branch, else_branch.as_deref(), env, is_tail, module_registry)
            }
            
            IrNode::Let { bindings, body, .. } => {
                self.execute_let(bindings, body, env, is_tail, module_registry)
            }
            
            IrNode::Do { expressions, .. } => {
                self.execute_do(expressions, env, is_tail, module_registry)
            }
            
            IrNode::Lambda { params, variadic_param, body, captures, .. } => {
                self.execute_lambda(params, variadic_param.as_deref(), body, captures, env)
            }
            
            IrNode::Match { expression, clauses, .. } => {
                self.execute_match(expression, clauses, env, module_registry)
            }
            
            IrNode::TryCatch { try_body, catch_clauses, finally_body, .. } => {
                self.execute_try_catch(try_body, catch_clauses, finally_body.as_deref(), env, module_registry)
            }
            
            IrNode::Parallel { bindings, .. } => {
                self.execute_parallel(bindings, env, module_registry)
            }
            
            IrNode::WithResource { binding, init_expr, body, .. } => {
                self.execute_with_resource(binding, init_expr, body, env, module_registry)
            }
            
            IrNode::LogStep { level, values, location, .. } => {
                self.execute_log_step(&level.0, values, location.as_ref(), env, module_registry)
            }
            
            IrNode::FunctionDef { name: _name, lambda, .. } => {
                let function_value = self.execute_node(lambda, env, false, module_registry)?;
                env.define(node.id(), function_value.clone());
                Ok(function_value)
            }
            IrNode::VariableDef { name: _name, init_expr, .. } => {
                let value = self.execute_node(init_expr, env, false, module_registry)?;
                env.define(node.id(), value.clone());
                Ok(value)
            }
            
            IrNode::Module { name: _, .. } => {
                self.execute_module(node, env, module_registry)
            }
            
            IrNode::Import { module_name, .. } => {
                self.execute_import(module_name)
            }
            
            IrNode::VariableBinding { name, .. } => Err(RuntimeError::InternalError(format!(
                "Attempted to execute a VariableBinding node for '{}'. These are for binding patterns and should not be executed directly.",
                name
            ))),

            _ => {
                Err(RuntimeError::NotImplemented(format!("IR node type not implemented: {:?}", node)))
            }
        }
    }
    
    /// Execute a literal value
    fn execute_literal(&self, literal: &ast::Literal) -> RuntimeResult<Value> {
        match literal {
            ast::Literal::Integer(n) => Ok(Value::Integer(*n)),
            ast::Literal::Float(f) => Ok(Value::Float(*f)),
            ast::Literal::String(s) => Ok(Value::String(s.clone())),
            ast::Literal::Boolean(b) => Ok(Value::Boolean(*b)),
            ast::Literal::Keyword(k) => Ok(Value::Keyword(k.clone())),
            ast::Literal::Nil => Ok(Value::Nil),
        }
    }

    /// Execute vector creation
    fn execute_vector(&mut self, elements: &[IrNode], env: &mut IrEnvironment, module_registry: &ModuleRegistry) -> RuntimeResult<Value> {
        let mut values = Vec::new();
        for element in elements {
            values.push(self.execute_node(element, env, false, module_registry)?);
        }
        Ok(Value::Vector(values))
    }

    /// Execute map creation
    fn execute_map(&mut self, entries: &[IrMapEntry], env: &mut IrEnvironment, module_registry: &ModuleRegistry) -> RuntimeResult<Value> {
        let mut map = std::collections::HashMap::new();
        for entry in entries {
            let key_value = self.execute_node(&entry.key, env, false, module_registry)?;
            let value_value = self.execute_node(&entry.value, env, false, module_registry)?;
            
            // Convert key to MapKey
            let map_key = match key_value {
                Value::String(s) => ast::MapKey::String(s),
                Value::Keyword(k) => ast::MapKey::Keyword(k),
                Value::Integer(i) => ast::MapKey::Integer(i), 
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
    fn execute_apply(&mut self, function: &IrNode, arguments: &[IrNode], env: &mut IrEnvironment, is_tail: bool, module_registry: &ModuleRegistry) -> RuntimeResult<Value> {
        let func_value = self.execute_node(function, env, false, module_registry)?;
        let mut arg_values = Vec::new();
        
        for arg in arguments {
            arg_values.push(self.execute_node(arg, env, false, module_registry)?);
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
        
        let result = self.call_function(func_value, &arg_values, env, module_registry);
        self.call_stack.pop();
        result
    }

    /// Call a function value (similar to AST runtime but with IR context)
    fn call_function(&mut self, func: Value, args: &[Value], env: &mut IrEnvironment, module_registry: &ModuleRegistry) -> RuntimeResult<Value> {
        match func {
            Value::Function(Function::Builtin { name, func, arity, .. }) => {
                self.check_arity(&arity, args.len(), &name)?;
                
                // Special handling for higher-order functions that need runtime context
                match name.as_str() {
                    "map" => self.builtin_map(args, env, module_registry),
                    "map-fn" => self.builtin_map(args, env, module_registry),  // map-fn is an alias for map
                    "filter" => self.builtin_filter(args, env, module_registry),
                    "reduce" => self.builtin_reduce(args, env, module_registry),
                    _ => func(args)  // Regular built-in functions
                }
            }
            Value::Function(Function::BuiltinWithEvaluator { name, .. }) => {
                // BuiltinWithEvaluator functions need access to unevaluated expressions,
                // but in IR runtime we only have evaluated values.
                // Special handling for functions that were converted to BuiltinWithEvaluator
                match name.as_str() {
                    "map" => self.builtin_map(args, env, module_registry),
                    "filter" => self.builtin_filter(args, env, module_registry),
                    _ => Err(RuntimeError::InternalError(
                        format!("BuiltinWithEvaluator function '{}' not supported in IR runtime", name)
                    ))
                }
            }
            Value::Function(Function::UserDefined { .. }) => {
                Err(RuntimeError::InternalError("UserDefined functions are not supported in IrRuntime. They should be converted to IrLambda.".to_string()))
            }
            Value::Function(Function::IrLambda { params, variadic_param, body, closure_env }) => {
                let min_args = params.len();
                let expected_arity_str = if variadic_param.is_some() {
                    format!("at least {}", min_args)
                } else {
                    min_args.to_string()
                };

                let arity_ok = if variadic_param.is_some() {
                    args.len() >= min_args
                } else {
                    args.len() == min_args
                };

                if !arity_ok {
                    return Err(RuntimeError::ArityMismatch {
                        function: "lambda".to_string(),
                        expected: expected_arity_str,
                        actual: args.len(),
                    });
                }

                self.call_ir_lambda(&params, variadic_param.as_deref(), &body, &*closure_env, args.to_vec(), module_registry)
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
                        self.call_function(resolved_func, args, env, module_registry)
                    }
                }
            }
            Value::Keyword(keyword) => {
                // Keywords act as functions: (:key map) is equivalent to (get map :key)
                if args.len() == 1 {
                    match &args[0] {
                        Value::Map(map) => {
                            let map_key = ast::MapKey::Keyword(keyword);
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
                            let map_key = ast::MapKey::Keyword(keyword);
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
    
    /// Call IR lambda function with tail call optimization for both direct and mutual recursion
    fn call_ir_lambda(
        &mut self,
        params: &[IrNode],
        variadic_param: Option<&IrNode>,
        body: &[IrNode],
        closure_env: &IrEnvironment,
        args: Vec<Value>,
        module_registry: &ModuleRegistry,
    ) -> RuntimeResult<Value> {
        let mut current_params = params.to_vec();
        let mut current_variadic_param: Option<Box<IrNode>> = variadic_param.map(|n| Box::new(n.clone()));
        let mut current_body = body.to_vec();
        let mut current_closure_env = closure_env.clone();
        let mut current_args = args;

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
            let mut func_env = IrEnvironment::with_parent(Rc::new(current_closure_env.clone()));
            
            // Bind parameters to arguments
            let num_params = current_params.len();
            for (i, param) in current_params.iter().enumerate() {
                if let IrNode::Param { binding, .. } = param {
                    if let IrNode::VariableBinding { id, .. } = binding.as_ref() {
                        if i < current_args.len() {
                           func_env.define(*id, current_args[i].clone());
                        }
                    }
                }
            }

            // Bind variadic parameter if it exists
            if let Some(ref variadic_node) = current_variadic_param {
                if let IrNode::Param { binding, .. } = variadic_node.as_ref() {
                    if let IrNode::VariableBinding { id, .. } = binding.as_ref() {
                        let rest_args = if current_args.len() > num_params {
                            current_args[num_params..].to_vec()
                        } else {
                            vec![]
                        };
                        func_env.define(*id, Value::Vector(rest_args));
                    }
                }
            }
            
            // Execute function body with tail call optimization
            let mut result = Value::Nil;
            let body_len = current_body.len();
            for (i, expr) in current_body.iter().enumerate() {
                let is_tail = i == body_len - 1;
                match self.execute_node(expr, &mut func_env, is_tail, module_registry) {
                    Ok(val) => result = val,
                    Err(RuntimeError::TailCall { function, args: next_args }) => {
                        match function {
                            Value::Function(Function::IrLambda { 
                                params: next_params, 
                                variadic_param: next_variadic,
                                body: next_body, 
                                closure_env: next_closure_env 
                            }) => {
                                // This is a tail call to another IR lambda. We can optimize.
                                current_params = next_params;
                                current_variadic_param = next_variadic;
                                current_body = next_body;
                                current_closure_env = *next_closure_env;
                                current_args = next_args;
                                
                                self.recursion_depth -= 1; // we are not making a new stack frame
                                continue 'tco;
                            }
                            _ => {
                                // Tail call to a builtin or other function type. Cannot optimize with the loop.
                                self.recursion_depth -= 1;
                                return self.call_function(function, &next_args, &mut func_env, module_registry);
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
        module_registry: &ModuleRegistry,
    ) -> RuntimeResult<Value> {
        let cond_val = self.execute_node(condition, env, false, module_registry)?;
        if cond_val.is_truthy() {
            self.execute_node(then_branch, env, is_tail, module_registry)
        } else if let Some(else_branch) = else_branch {
            self.execute_node(else_branch, env, is_tail, module_registry)
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
        module_registry: &ModuleRegistry,
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
                let value = self.execute_node(&binding.init_expr, &mut local_env, false, module_registry)?;
                
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
            result = self.execute_node(expr, &mut local_env, is_last && is_tail, module_registry)?;
        }

        Ok(result)
    }

    fn execute_do(
        &mut self,
        expressions: &[IrNode],
        env: &mut IrEnvironment,
        is_tail: bool,
        module_registry: &ModuleRegistry,
    ) -> RuntimeResult<Value> {
        let mut result = Value::Nil;
        let num_expressions = expressions.len();
        for (i, expr) in expressions.iter().enumerate() {
            let is_last = i == num_expressions - 1;
            result = self.execute_node(expr, env, is_last && is_tail, module_registry)?;
        }
        Ok(result)
    }

    fn execute_lambda(
        &mut self,
        params: &[IrNode],
        variadic_param: Option<&IrNode>,
        body: &[IrNode],
        _captures: &[IrCapture],
        env: &mut IrEnvironment,
    ) -> RuntimeResult<Value> {
        let function = Function::IrLambda {
            params: params.to_vec(),
            variadic_param: variadic_param.map(|p| Box::new(p.clone())),
            body: body.to_vec(),
            closure_env: Box::new(env.clone()),
        };
        Ok(Value::Function(function))
    }

    fn execute_match(
        &mut self,
        expression: &IrNode,
        clauses: &[IrMatchClause],
        env: &mut IrEnvironment,
        module_registry: &ModuleRegistry,
    ) -> RuntimeResult<Value> {
        let value_to_match = self.execute_node(expression, env, false, module_registry)?;

        for clause in clauses {
            let mut local_env = IrEnvironment::with_parent(Rc::new(env.clone()));
            if self.match_pattern(&value_to_match, &clause.pattern, &mut local_env)? {
                // If there's a guard, evaluate it
                if let Some(guard) = &clause.guard {
                    let guard_result = self.execute_node(guard, &mut local_env, false, module_registry)?;
                    if !guard_result.is_truthy() {
                        continue; // Guard is false, try next clause
                    }
                }
                // Guard passed or no guard, execute the body
                return self.execute_node(&clause.body, &mut local_env, false, module_registry);
            }
        }

        Err(RuntimeError::MatchError(format!("No match for value {:?}", value_to_match)))
    }

    fn match_pattern(&self, value: &Value, pattern: &IrPattern, env: &mut IrEnvironment) -> RuntimeResult<bool> {
        match (pattern, value) {
            (IrPattern::Wildcard, _) => Ok(true),

            (IrPattern::Variable(name), _v) => {
                // The current IR does not support binding variables in match patterns.
                // The binding should happen in a let or function parameter.
                // This is a limitation of the current implementation, not a fundamental one.
                Err(RuntimeError::InternalError(format!(
                    "Binding variable '{}' in match is not supported by the current IR. Use a let binding inside the match clause instead.",
                    name
                )))
            }

            (IrPattern::Literal(p_val), v) => {
                let pattern_value = self.execute_literal(p_val)?;
                Ok(pattern_value == *v)
            }

            (IrPattern::Vector { elements, rest }, Value::Vector(v_elements)) => {
                if let Some(rest_name) = rest {
                    if v_elements.len() < elements.len() {
                        return Ok(false);
                    }
                    for (i, p_elem) in elements.iter().enumerate() {
                        if !self.match_pattern(&v_elements[i], p_elem, env)? {
                            return Ok(false);
                        }
                    }
                    // The current IR does not support binding rest variables in match patterns.
                    Err(RuntimeError::InternalError(format!(
                        "Binding rest variable '{}' in vector pattern is not supported by the current IR.",
                        rest_name
                    )))
                } else {
                    if elements.len() != v_elements.len() {
                        return Ok(false);
                    }
                    for (i, p_elem) in elements.iter().enumerate() {
                        if !self.match_pattern(&v_elements[i], p_elem, env)? {
                            return Ok(false);
                        }
                    }
                    Ok(true)
                }
            }

            (IrPattern::Map { entries, rest }, Value::Map(v_map)) => {
                for entry in entries {
                    if let Some(value) = v_map.get(&entry.key) {
                        if !self.match_pattern(value, &entry.pattern, env)? {
                            return Ok(false);
                        }
                    } else {
                        return Ok(false);
                    }
                }

                if let Some(rest_name) = rest {
                     // The current IR does not support binding rest variables in match patterns.
                    Err(RuntimeError::InternalError(format!(
                        "Binding rest variable '{}' in map pattern is not supported by the current IR.",
                        rest_name
                    )))
                } else {
                    // If no rest pattern, all keys must be matched.
                    if entries.len() != v_map.len() {
                        return Ok(false);
                    }
                    Ok(true)
                }
            }
            
            (IrPattern::Type(_type_name), _) => Err(RuntimeError::NotImplemented("Type patterns".to_string())),

            // Mismatch between pattern and value type
            _ => Ok(false),
        }
    }

    fn execute_try_catch(
        &mut self,
        try_body: &[IrNode],
        catch_clauses: &[IrCatchClause],
        finally_body: Option<&[IrNode]>,
        env: &mut IrEnvironment,
        module_registry: &ModuleRegistry,
    ) -> RuntimeResult<Value> {
        let result = (|| {
            let mut last_val = Value::Nil;
            for node in try_body {
                last_val = self.execute_node(node, env, false, module_registry)?;
            }
            Ok(last_val)
        })();

        let final_result = match result {
            Ok(value) => Ok(value),
            Err(e) => {
                // Convert runtime error to a Value so it can be matched
                let error_value = self.error_to_value(&e);
                let mut handled = false;
                let mut catch_result = Err(e);

                for clause in catch_clauses {
                    let mut local_env = IrEnvironment::with_parent(Rc::new(env.clone()));
                    if self.match_pattern(&error_value, &clause.error_pattern, &mut local_env)? {
                        if let Some(binding_name) = &clause.binding {
                             return Err(RuntimeError::InternalError(format!(
                                "Binding '{}' in catch is not supported by the current IR.",
                                binding_name
                            )));
                        }

                        let mut last_val = Value::Nil;
                        for expr in &clause.body {
                            last_val = self.execute_node(expr, &mut local_env, false, module_registry)?;
                        }
                        catch_result = Ok(last_val);
                        handled = true;
                        break;
                    }
                }

                if !handled {
                    // If no catch clause matched, propagate the original error
                    return catch_result;
                }
                catch_result
            }
        };

        if let Some(finally) = finally_body {
            // Execute finally block, its result does not override the try/catch result
            for node in finally {
                self.execute_node(node, env, false, module_registry)?;
            }
        }

        final_result
    }

    fn execute_parallel(
        &mut self,
        _bindings: &[IrParallelBinding],
        _env: &mut IrEnvironment,
        _module_registry: &ModuleRegistry,
    ) -> RuntimeResult<Value> {
        // Placeholder for parallel execution.
        // In a real implementation, this would involve threads or async tasks.
        Err(RuntimeError::NotImplemented("Parallel execution".to_string()))
    }

    fn execute_with_resource(
        &mut self,
        _binding: &IrNode,
        _init_expr: &IrNode,
        _body: &[IrNode],
        _env: &mut IrEnvironment,
        _module_registry: &ModuleRegistry,
    ) -> RuntimeResult<Value> {
        // Placeholder for resource management.
        Err(RuntimeError::NotImplemented("With-resource".to_string()))
    }

    fn execute_log_step(
        &mut self,
        level: &str,
        values: &[IrNode],
        location: Option<&String>,
        env: &mut IrEnvironment,
        module_registry: &ModuleRegistry,
    ) -> RuntimeResult<Value> {
        let mut evaluated_values = Vec::new();
        for value_node in values {
            evaluated_values.push(self.execute_node(value_node, env, false, module_registry)?);
        }

        let location_str = location.map_or("".to_string(), |loc| format!(" at {}", loc));
        println!("[LOG {}]{}: {:?}", level.to_uppercase(), location_str, evaluated_values);

        Ok(Value::Nil)
    }

    fn execute_module(&mut self, node: &IrNode, env: &mut IrEnvironment, module_registry: &ModuleRegistry) -> RuntimeResult<Value> {
        if let IrNode::Module { definitions, .. } = node {
            let mut last_value = Value::Nil;
            for def in definitions {
                last_value = self.execute_node(def, env, false, module_registry)?;
            }
            Ok(last_value)
        } else {
            Err(RuntimeError::InvalidProgram("Expected IrNode::Module".to_string()))
        }
    }

    fn execute_import(&self, module_name: &str) -> RuntimeResult<Value> {
        // Similar to Module, imports are resolved before execution.
        // This could be a no-op.
        Ok(Value::String(format!("Imported {}", module_name))) // Placeholder
    }

    fn is_pure_expression(&self, _node: &IrNode) -> bool {
        // A more sophisticated check would analyze the node type and its children.
        // For now, we assume most nodes are not pure to be safe.
        false
    }

    fn check_arity(&self, arity: &Arity, num_args: usize, function_name: &str) -> RuntimeResult<()> {
        match arity {
            Arity::Exact(n) => {
                if *n != num_args {
                    return Err(RuntimeError::ArityMismatch {
                        function: function_name.to_string(),
                        expected: n.to_string(),
                        actual: num_args,
                    });
                }
            }
            Arity::AtLeast(min) => {
                if num_args < *min {
                    return Err(RuntimeError::ArityMismatch {
                        function: function_name.to_string(),
                        expected: format!("at least {}", min),
                        actual: num_args,
                    });
                }
            }
            Arity::Range(min, max) => {
                if num_args < *min || num_args > *max {
                     return Err(RuntimeError::ArityMismatch {
                        function: function_name.to_string(),
                        expected: format!("between {} and {}", min, max),
                        actual: num_args,
                    });
                }
            }
            Arity::Any => {}
        }
        Ok(())
    }

    fn error_to_value(&self, error: &RuntimeError) -> Value {
        let mut map = std::collections::HashMap::new();
        let type_keyword = |s: &str| MapKey::Keyword(Keyword(s.to_string()));

        match error {
            RuntimeError::TypeError { expected, actual, operation } => {
                map.insert(type_keyword("type"), Value::Keyword(Keyword("type-error".to_string())));
                map.insert(type_keyword("expected"), Value::String(expected.clone()));
                map.insert(type_keyword("actual"), Value::String(actual.clone()));
                map.insert(type_keyword("operation"), Value::String(operation.clone()));
            }
            RuntimeError::UndefinedSymbol(s) => {
                map.insert(type_keyword("type"), Value::Keyword(Keyword("undefined-symbol".to_string())));
                map.insert(type_keyword("symbol"), Value::String(s.0.clone()));
            }
            RuntimeError::SymbolNotFound(s) => {
                map.insert(type_keyword("type"), Value::Keyword(Keyword("symbol-not-found".to_string())));
                map.insert(type_keyword("symbol"), Value::String(s.clone()));
            }
            RuntimeError::ModuleNotFound(s) => {
                map.insert(type_keyword("type"), Value::Keyword(Keyword("module-not-found".to_string())));
                map.insert(type_keyword("module"), Value::String(s.clone()));
            }
            RuntimeError::ArityMismatch { function, expected, actual } => {
                map.insert(type_keyword("type"), Value::Keyword(Keyword("arity-mismatch".to_string())));
                map.insert(type_keyword("function"), Value::String(function.clone()));
                map.insert(type_keyword("expected"), Value::String(expected.clone()));
                map.insert(type_keyword("actual"), Value::Integer(*actual as i64));
            }
            RuntimeError::DivisionByZero => {
                map.insert(type_keyword("type"), Value::Keyword(Keyword("division-by-zero".to_string())));
            }
            RuntimeError::IndexOutOfBounds { index, length } => {
                map.insert(type_keyword("type"), Value::Keyword(Keyword("index-out-of-bounds".to_string())));
                map.insert(type_keyword("index"), Value::Integer(*index));
                map.insert(type_keyword("length"), Value::Integer(*length as i64));
            }
            RuntimeError::KeyNotFound { key } => {
                map.insert(type_keyword("type"), Value::Keyword(Keyword("key-not-found".to_string())));
                map.insert(type_keyword("key"), Value::String(key.clone()));
            }
            RuntimeError::ResourceError { resource_type, message } => {
                map.insert(type_keyword("type"), Value::Keyword(Keyword("resource-error".to_string())));
                map.insert(type_keyword("resource-type"), Value::String(resource_type.clone()));
                map.insert(type_keyword("message"), Value::String(message.clone()));
            }
            RuntimeError::IoError(msg) => {
                map.insert(type_keyword("type"), Value::Keyword(Keyword("io-error".to_string())));
                map.insert(type_keyword("message"), Value::String(msg.clone()));
            }
            RuntimeError::ModuleError(msg) => {
                map.insert(type_keyword("type"), Value::Keyword(Keyword("module-error".to_string())));
                map.insert(type_keyword("message"), Value::String(msg.clone()));
            }
            RuntimeError::InvalidArgument(msg) => {
                map.insert(type_keyword("type"), Value::Keyword(Keyword("invalid-argument".to_string())));
                map.insert(type_keyword("message"), Value::String(msg.clone()));
            }
            RuntimeError::NetworkError(msg) => {
                map.insert(type_keyword("type"), Value::Keyword(Keyword("network-error".to_string())));
                map.insert(type_keyword("message"), Value::String(msg.clone()));
            }
            RuntimeError::JsonError(msg) => {
                map.insert(type_keyword("type"), Value::Keyword(Keyword("json-error".to_string())));
                map.insert(type_keyword("message"), Value::String(msg.clone()));
            }
            RuntimeError::MatchError(msg) => {
                map.insert(type_keyword("type"), Value::Keyword(Keyword("match-error".to_string())));
                map.insert(type_keyword("message"), Value::String(msg.clone()));
            }
            RuntimeError::AgentDiscoveryError { message, registry_uri } => {
                map.insert(type_keyword("type"), Value::Keyword(Keyword("agent-discovery-error".to_string())));
                map.insert(type_keyword("message"), Value::String(message.clone()));
                map.insert(type_keyword("registry-uri"), Value::String(registry_uri.clone()));
            }
            RuntimeError::AgentCommunicationError { message, agent_id, endpoint } => {
                map.insert(type_keyword("type"), Value::Keyword(Keyword("agent-communication-error".to_string())));
                map.insert(type_keyword("message"), Value::String(message.clone()));
                map.insert(type_keyword("agent-id"), Value::String(agent_id.clone()));
                map.insert(type_keyword("endpoint"), Value::String(endpoint.clone()));
            }
            RuntimeError::AgentProfileError { message, profile_uri } => {
                map.insert(type_keyword("type"), Value::Keyword(Keyword("agent-profile-error".to_string())));
                map.insert(type_keyword("message"), Value::String(message.clone()));
                if let Some(uri) = profile_uri {
                    map.insert(type_keyword("profile-uri"), Value::String(uri.clone()));
                }
            }
            RuntimeError::ApplicationError { error_type, message, data } => {
                map.insert(type_keyword("type"), Value::Keyword(Keyword("application-error".to_string())));
                map.insert(type_keyword("error-type"), Value::Keyword(error_type.clone()));
                map.insert(type_keyword("message"), Value::String(message.clone()));
                if let Some(d) = data {
                    map.insert(type_keyword("data"), d.clone());
                }
            }
            RuntimeError::InvalidProgram(msg) => {
                map.insert(type_keyword("type"), Value::Keyword(Keyword("invalid-program".to_string())));
                map.insert(type_keyword("message"), Value::String(msg.clone()));
            }
            RuntimeError::NotImplemented(msg) => {
                map.insert(type_keyword("type"), Value::Keyword(Keyword("not-implemented".to_string())));
                map.insert(type_keyword("message"), Value::String(msg.clone()));
            }
            RuntimeError::NotCallable(v) => {
                map.insert(type_keyword("type"), Value::Keyword(Keyword("not-callable".to_string())));
                map.insert(type_keyword("value"), Value::String(v.clone()));
            }
            RuntimeError::InternalError(msg) => {
                map.insert(type_keyword("type"), Value::Keyword(Keyword("internal-error".to_string())));
                map.insert(type_keyword("message"), Value::String(msg.clone()));
            }
            RuntimeError::TailCall { .. } => {
                map.insert(type_keyword("type"), Value::Keyword(Keyword("internal-error".to_string())));
                map.insert(type_keyword("message"), Value::String("Cannot catch tail call".to_string()));
            }
            RuntimeError::StackOverflow(msg) => {
                map.insert(type_keyword("type"), Value::Keyword(Keyword("stack-overflow".to_string())));
                map.insert(type_keyword("message"), Value::String(msg.clone()));
            }
        }
        Value::Map(map)
    }

    // --- Built-in function implementations ---

    fn builtin_map(&mut self, args: &[Value], env: &mut IrEnvironment, module_registry: &ModuleRegistry) -> RuntimeResult<Value> {
        if args.len() != 2 {
            return Err(RuntimeError::ArityMismatch {
                function: "map".to_string(),
                expected: "2".to_string(),
                actual: args.len(),
            });
        }
        let func = &args[0];
        let collection = &args[1];

        match collection {
            Value::Vector(vec) => {
                let mut result_vec = Vec::new();
                for item in vec {
                    let result = self.call_function(func.clone(), &[item.clone()], env, module_registry)?;
                    result_vec.push(result);
                }
                Ok(Value::Vector(result_vec))
            }
            _ => Err(RuntimeError::TypeError {
                expected: "vector".to_string(),
                actual: collection.type_name().to_string(),
                operation: "map".to_string(),
            }),
        }
    }

    fn builtin_filter(&mut self, args: &[Value], env: &mut IrEnvironment, module_registry: &ModuleRegistry) -> RuntimeResult<Value> {
        if args.len() != 2 {
            return Err(RuntimeError::ArityMismatch {
                function: "filter".to_string(),
                expected: "2".to_string(),
                actual: args.len(),
            });
        }
        let predicate = &args[0];
        let collection = &args[1];

        match collection {
            Value::Vector(vec) => {
                let mut result_vec = Vec::new();
                for item in vec {
                    let result = self.call_function(predicate.clone(), &[item.clone()], env, module_registry)?;
                    if result.is_truthy() {
                        result_vec.push(item.clone());
                    }
                }
                Ok(Value::Vector(result_vec))
            }
            _ => Err(RuntimeError::TypeError {
                expected: "vector".to_string(),
                actual: collection.type_name().to_string(),
                operation: "filter".to_string(),
            }),
        }
    }

    fn builtin_reduce(&mut self, args: &[Value], env: &mut IrEnvironment, module_registry: &ModuleRegistry) -> RuntimeResult<Value> {
        if args.len() != 3 {
            return Err(RuntimeError::ArityMismatch {
                function: "reduce".to_string(),
                expected: "3".to_string(),
                actual: args.len(),
            });
        }
        let func = &args[0];
        let initial_value = &args[1];
        let collection = &args[2];

        match collection {
            Value::Vector(vec) => {
                let mut accumulator = initial_value.clone();
                for item in vec {
                    accumulator = self.call_function(func.clone(), &[accumulator, item.clone()], env, module_registry)?;
                }
                Ok(accumulator)
            }
            _ => Err(RuntimeError::TypeError {
                expected: "vector".to_string(),
                actual: collection.type_name().to_string(),
                operation: "reduce".to_string(),
            }),
        }
    }
}

impl Default for IrRuntime {
    fn default() -> Self {
        Self::new()
    }
}
