// RTFS Evaluator - Executes parsed AST nodes

use std::collections::HashMap;
use std::rc::Rc;
use crate::ast::*;
use crate::runtime::{Value, RuntimeError, RuntimeResult, Environment};
use crate::runtime::values::{Function, Arity};
use crate::runtime::stdlib::StandardLibrary;
use crate::agent::{AgentDiscovery, NoOpAgentDiscovery, SimpleDiscoveryQuery, SimpleDiscoveryOptions, SimpleAgentCard};
// Add RefCell for the placeholder strategy
use std::cell::RefCell;

pub struct Evaluator {
    global_env: Rc<Environment>,
    agent_discovery: Box<dyn AgentDiscovery>,
    recursion_depth: usize,
    max_recursion_depth: usize,
    pub task_context: Option<Value>,
}

// Helper function to check if two values are equivalent
// This is a simplified version for the fixpoint algorithm
fn values_equivalent(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Nil, Value::Nil) => true,
        (Value::Function(Function::UserDefined { params: p1, body: b1, .. }), 
         Value::Function(Function::UserDefined { params: p2, body: b2, .. })) => {
            p1 == p2 && b1 == b2
        },
        (Value::Function(Function::Builtin { name: n1, .. }), 
         Value::Function(Function::Builtin { name: n2, .. })) => {
            n1 == n2
        },
        _ => false, // Different types or can't compare
    }
}

impl Evaluator {
    /// Create a new evaluator with standard library loaded and default agent discovery
    pub fn new() -> Self {
        let global_env = StandardLibrary::create_global_environment();
        Evaluator {
            global_env: Rc::new(global_env),
            agent_discovery: Box::new(NoOpAgentDiscovery),
            recursion_depth: 0,
            max_recursion_depth: 1000,
            task_context: None,
        }
    }

    /// Create a new evaluator with custom agent discovery service
    pub fn with_agent_discovery(agent_discovery: Box<dyn AgentDiscovery>) -> Self {
        let global_env = StandardLibrary::create_global_environment();
        Evaluator {
            global_env: Rc::new(global_env),
            agent_discovery,
            recursion_depth: 0,
            max_recursion_depth: 1000,
            task_context: None,
        }
    }

    /// Set the task context for the evaluator
    pub fn set_task_context(&mut self, context: Value) {
        self.task_context = Some(context);
    }    /// Evaluate an expression in a given environment
    pub fn eval_expr(&self, expr: &Expression, env: &mut Environment) -> RuntimeResult<Value> {
        match expr {
            Expression::Literal(lit) => self.eval_literal(lit),            Expression::Symbol(sym) => env.lookup(sym),            Expression::List(exprs) => {
                // Empty list evaluates to empty list
                if exprs.is_empty() {
                    return Ok(Value::Vector(vec![]));
                }
                
                // First element should be a function
                let func_expr = &exprs[0];
                let func_value = self.eval_expr(func_expr, env)?;
                
                // Evaluate arguments
                let args: Result<Vec<Value>, RuntimeError> = exprs[1..]
                    .iter()
                    .map(|e| self.eval_expr(e, env))
                    .collect();
                let args = args?;
                
                self.call_function(func_value, &args, env)
            },
            Expression::Vector(exprs) => {
                let values: Result<Vec<Value>, RuntimeError> = exprs
                    .iter()
                    .map(|e| self.eval_expr(e, env))
                    .collect();
                Ok(Value::Vector(values?))
            },
            Expression::Map(map) => {
                let mut result = HashMap::new();
                for (key, value_expr) in map {
                    let value = self.eval_expr(value_expr, env)?;
                    result.insert(key.clone(), value);
                }
                Ok(Value::Map(result))
            },              Expression::FunctionCall { callee, arguments } => {
                let func_value = self.eval_expr(callee, env)?;
                
                // Check if this is a builtin that needs unevaluated arguments
                match &func_value {
                    Value::Function(Function::BuiltinWithEvaluator { name, arity, func }) => {
                        // Check arity
                        if !self.check_arity(&arity, arguments.len()) {
                            return Err(RuntimeError::ArityMismatch {
                                function: name.clone(),
                                expected: self.arity_to_string(&arity),
                                actual: arguments.len(),
                            });
                        }
                        
                        // Call with unevaluated arguments
                        func(arguments, self, env)
                    },
                    _ => {
                        // Evaluate arguments and call normally
                        let args: Result<Vec<Value>, RuntimeError> = arguments
                            .iter()
                            .map(|e| self.eval_expr(e, env))
                            .collect();
                        let args = args?;
                        
                        self.call_function(func_value, &args, env)
                    }
                }
            },
            Expression::If(if_expr) => self.eval_if(if_expr, env),
            Expression::Let(let_expr) => self.eval_let(let_expr, env),
            Expression::Do(do_expr) => self.eval_do(do_expr, env),
            Expression::Match(match_expr) => self.eval_match(match_expr, env),
            Expression::LogStep(log_expr) => self.eval_log_step(log_expr, env),
            Expression::TryCatch(try_expr) => self.eval_try_catch(try_expr, env),
            Expression::Fn(fn_expr) => self.eval_fn(fn_expr, env),
            Expression::WithResource(with_expr) => self.eval_with_resource(with_expr, env),
            Expression::Parallel(parallel_expr) => self.eval_parallel(parallel_expr, env),
            Expression::Def(def_expr) => self.eval_def(def_expr, env),
            Expression::Defn(defn_expr) => self.eval_defn(defn_expr, env),
            Expression::DiscoverAgents(discover_expr) => self.eval_discover_agents(discover_expr, env),
            // Expression::TaskContext(task_context) => self.eval_task_context(task_context, env),
        }
    }
      /// Evaluate an expression in the global environment
    pub fn evaluate(&self, expr: &Expression) -> RuntimeResult<Value> {
        let mut env = Environment::with_parent(self.global_env.clone());
        self.eval_expr(expr, &mut env)
    }
    
    /// Evaluate an expression with a provided environment
    pub fn evaluate_with_env(&self, expr: &Expression, env: &mut Environment) -> RuntimeResult<Value> {
        self.eval_expr(expr, env)
    }
    
    fn eval_literal(&self, lit: &Literal) -> RuntimeResult<Value> {
        match lit {
            Literal::Integer(n) => Ok(Value::Integer(*n)),
            Literal::Float(f) => Ok(Value::Float(*f)),
            Literal::String(s) => Ok(Value::String(s.clone())),
            Literal::Boolean(b) => Ok(Value::Boolean(*b)),
            Literal::Keyword(k) => Ok(Value::Keyword(k.clone())),
            Literal::Nil => Ok(Value::Nil),
        }
    }
      pub fn call_function(&self, func_value: Value, args: &[Value], env: &mut Environment) -> RuntimeResult<Value> {
        match func_value {
            Value::FunctionPlaceholder(cell) => {
                let actual_function = cell.borrow().clone();
                // Safeguard against unresolved or circular placeholders
                match actual_function {
                    Value::Nil => { // Assuming Nil is the initial placeholder state
                        return Err(RuntimeError::InternalError(
                            "Recursive function placeholder is not resolved (points to Nil)".to_string()
                        ));
                    }
                    Value::FunctionPlaceholder(_) => {
                         return Err(RuntimeError::InternalError(
                            "Recursive function placeholder points to another placeholder. This indicates a bug in resolution.".to_string()
                        ));
                    }
                    _ => {} // Proceed if it's a real function or other callable value
                }
                // Call the resolved function. The 'env' here is the caller's environment for argument evaluation.
                self.call_function(actual_function, args, env)            },            Value::Function(Function::Builtin { name, arity, func }) => {
                // Check arity
                if !self.check_arity(&arity, args.len()) {
                    return Err(RuntimeError::ArityMismatch {
                        function: name,
                        expected: self.arity_to_string(&arity),
                        actual: args.len(),
                    });
                }
                
                func(args)
            },            Value::Function(Function::BuiltinWithEvaluator { name, arity: _, func: _ }) => {
                return Err(RuntimeError::InternalError(
                    format!("BuiltinWithEvaluator function '{}' called through call_function instead of direct function call evaluation", name)
                ));
            },
            Value::Function(Function::UserDefined { params, variadic_param, body, closure }) => {
                // Create new environment for function execution, parented by the captured closure
                let mut func_env = Environment::with_parent(Rc::new(closure.clone()));
                
                let required_params = params.len();
                let has_variadic = variadic_param.is_some();
                
                if !has_variadic && args.len() != required_params {
                    return Err(RuntimeError::ArityMismatch {
                        function: "#<user-function>".to_string(),
                        expected: required_params.to_string(),
                        actual: args.len(),
                    });
                } else if has_variadic && args.len() < required_params {
                    return Err(RuntimeError::ArityMismatch {
                        function: "#<user-function>".to_string(),
                        expected: format!("at least {}", required_params),
                        actual: args.len(),
                    });
                }
                
                // Bind required parameters
                for (i, param) in params.iter().enumerate() {
                    self.bind_pattern(&param.pattern, &args[i], &mut func_env)?;
                }
                
                // Bind variadic parameter if present
                if let Some(variadic) = &variadic_param {
                    let variadic_args = args[required_params..].to_vec();
                    self.bind_pattern(&variadic.pattern, &Value::Vector(variadic_args), &mut func_env)?;
                }
                  // Execute function body with dynamic lookup support for recursive calls
                self.eval_do_body(&body, &mut func_env)
            },
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
            _ => Err(RuntimeError::TypeError {
                expected: "function or function placeholder".to_string(), // Updated expected types
                actual: func_value.type_name().to_string(),
                operation: "function call".to_string(),
            }),
        }
    }
    
    fn check_arity(&self, arity: &Arity, arg_count: usize) -> bool {
        match arity {
            Arity::Exact(n) => arg_count == *n,
            Arity::AtLeast(n) => arg_count >= *n,
            Arity::Range(min, max) => arg_count >= *min && arg_count <= *max,
            Arity::Any => true,
        }
    }
    
    fn arity_to_string(&self, arity: &Arity) -> String {
        match arity {
            Arity::Exact(n) => n.to_string(),
            Arity::AtLeast(n) => format!("at least {}", n),
            Arity::Range(min, max) => format!("{}-{}", min, max),
            Arity::Any => "any number".to_string(),
        }
    }
    
    fn eval_if(&self, if_expr: &IfExpr, env: &mut Environment) -> RuntimeResult<Value> {
        let condition = self.eval_expr(&if_expr.condition, env)?;
        
        if condition.is_truthy() {
            self.eval_expr(&if_expr.then_branch, env)
        } else if let Some(else_branch) = &if_expr.else_branch {
            self.eval_expr(else_branch, env)
        } else {
            Ok(Value::Nil)
        }
    }    fn eval_let(&self, let_expr: &LetExpr, env: &mut Environment) -> RuntimeResult<Value> {
        // Create new scope for let bindings, parented by the current environment
        let mut let_env = Environment::with_parent(Rc::new(env.clone()));
        
        let mut function_bindings_to_resolve = Vec::new();
        let mut other_bindings = Vec::new();

        // Pass 1: Identify functions, create placeholders, and separate other bindings
        for binding in &let_expr.bindings {
            // We currently only support Symbol patterns for let-bound functions for simplicity in letrec
            if let crate::ast::Pattern::Symbol(symbol) = &binding.pattern {
                if matches!(binding.value.as_ref(), Expression::Fn(_)) {
                    // Create a placeholder cell, initialized to Nil (or a dedicated Unresolved variant)
                    // This assumes Value::Nil is a safe temporary placeholder.
                    // A dedicated Value::UnresolvedPlaceholder would be more robust.
                    let placeholder_cell = Rc::new(RefCell::new(Value::Nil)); 
                    
                    // Define the placeholder in let_env immediately.
                    // This placeholder will be captured in the closures of functions defined in this block.
                    let_env.define(symbol, Value::FunctionPlaceholder(placeholder_cell.clone()));
                    
                    // Store for resolution in Pass 2
                    function_bindings_to_resolve.push((symbol.clone(), binding.value.clone(), placeholder_cell));
                } else {
                    other_bindings.push(binding);
                }
            } else {
                // Non-symbol patterns are treated as other bindings
                other_bindings.push(binding);
            }
        }
        
        // Evaluate and bind non-function bindings. These are evaluated in the let_env
        // which already contains placeholders for any functions.
        for binding in other_bindings {
            let value = self.eval_expr(&binding.value, &mut let_env)?;
            self.bind_pattern(&binding.pattern, &value, &mut let_env)?;
        }
        
        // Pass 2: Resolve function placeholders.
        // Create actual function values and update their corresponding placeholders.
        for (symbol, fn_expr_ast, placeholder_cell) in function_bindings_to_resolve {
            if let Expression::Fn(fn_expr_params_body) = fn_expr_ast.as_ref() {
                // The closure for the function is a clone of the current let_env.
                // This let_env contains all FunctionPlaceholders for sibling functions,
                // allowing them to be mutually recursive.
                let user_defined_function = Function::UserDefined {
                    params: fn_expr_params_body.params.clone(),
                    variadic_param: fn_expr_params_body.variadic_param.clone(),
                    body: fn_expr_params_body.body.clone(),
                    closure: let_env.clone(), 
                };
                let function_value = Value::Function(user_defined_function);
                
                // Update the placeholder cell to point to the actual function value.
                *placeholder_cell.borrow_mut() = function_value;

            } else {
                // This case should not be reached if matches! above was correct
                return Err(RuntimeError::InternalError(format!(
                    "Expected Expression::Fn for symbol '{}' in letrec resolution pass.",
                    symbol.0
                )));
            }
        }
        
        // Evaluate the body of the let expression in the let_env.
        // This environment now has non-function bindings resolved, and function symbols
        // pointing to FunctionPlaceholders which in turn point to the fully resolved functions.
        self.eval_do_body(&let_expr.body, &mut let_env)
    }
    
    fn eval_do(&self, do_expr: &DoExpr, env: &mut Environment) -> RuntimeResult<Value> {
        self.eval_do_body(&do_expr.expressions, env)
    }
    
    fn eval_do_body(&self, exprs: &[Expression], env: &mut Environment) -> RuntimeResult<Value> {
        if exprs.is_empty() {
            return Ok(Value::Nil);
        }
        
        let mut result = Value::Nil;
        for expr in exprs {
            result = self.eval_expr(expr, env)?;
        }
        Ok(result)
    }
    
    fn eval_match(&self, match_expr: &MatchExpr, env: &mut Environment) -> RuntimeResult<Value> {
        let value = self.eval_expr(&match_expr.expression, env)?;
        
        for clause in &match_expr.clauses {
            let mut match_env = Environment::with_parent(Rc::new(env.clone()));
            
            if self.match_match_pattern(&clause.pattern, &value, &mut match_env)? {
                // Check guard if present
                if let Some(guard) = &clause.guard {
                    let guard_result = self.eval_expr(guard, &mut match_env)?;
                    if !guard_result.is_truthy() {
                        continue;
                    }
                }
                
                // Execute clause body
                return self.eval_expr(&clause.body, &mut match_env);
            }
        }
        
        Err(RuntimeError::MatchError(format!("No matching clause for value: {}", value.to_string())))
    }
    
    fn eval_log_step(&self, log_expr: &LogStepExpr, env: &mut Environment) -> RuntimeResult<Value> {
        let level = log_expr.level.as_ref()
            .map(|k| k.0.clone())
            .unwrap_or_else(|| "info".to_string());
        
        let values: Result<Vec<Value>, RuntimeError> = log_expr.values
            .iter()
            .map(|e| self.eval_expr(e, env))
            .collect();
        let values = values?;
        
        let message = values.iter()
            .map(|v| v.to_string())
            .collect::<Vec<String>>()
            .join(" ");
        
        let location = log_expr.location.as_ref()
            .map(|s| format!(" [{}]", s))
            .unwrap_or_default();
        
        println!("[{}]{}: {}", level.to_uppercase(), location, message);
        
        // Return the last value or nil
        Ok(values.last().cloned().unwrap_or(Value::Nil))
    }
    
    fn eval_try_catch(&self, try_expr: &TryCatchExpr, env: &mut Environment) -> RuntimeResult<Value> {
        // Execute try body
        let try_result = self.eval_do_body(&try_expr.try_body, env);
        
        match try_result {
            Ok(value) => {
                // If we have a finally block, execute it
                if let Some(finally_body) = &try_expr.finally_body {
                    self.eval_do_body(finally_body, env)?;
                }
                Ok(value)
            },
            Err(error) => {
                // Try to match error against catch clauses
                let error_value = error.to_value();
                
                for catch_clause in &try_expr.catch_clauses {
                    if self.match_catch_pattern_actual(&catch_clause.pattern, &error_value)? {
                        let mut catch_env = Environment::with_parent(Rc::new(env.clone()));
                        catch_env.define(&catch_clause.binding, error_value);
                        
                        let result = self.eval_do_body(&catch_clause.body, &mut catch_env);
                        
                        // Execute finally block
                        if let Some(finally_body) = &try_expr.finally_body {
                            self.eval_do_body(finally_body, env)?;
                        }
                        
                        return result;
                    }
                }
                
                // Execute finally block even if no catch matched
                if let Some(finally_body) = &try_expr.finally_body {
                    self.eval_do_body(finally_body, env)?;
                }
                
                // Re-throw the error
                Err(error)
            }
        }
    }
    
    fn eval_fn(&self, fn_expr: &FnExpr, env: &mut Environment) -> RuntimeResult<Value> {
        Ok(Value::Function(Function::UserDefined {
            params: fn_expr.params.clone(),
            variadic_param: fn_expr.variadic_param.clone(),
            body: fn_expr.body.clone(),
            closure: env.clone(),
        }))
    }
      fn eval_with_resource(&self, with_expr: &WithResourceExpr, env: &mut Environment) -> RuntimeResult<Value> {
        // Evaluate the resource initialization expression
        let resource_value = self.eval_expr(&with_expr.resource_init, env)?;
        
        // Ensure the resource is a Resource handle
        if let Value::Resource(mut handle) = resource_value {
            // Mark resource as active
            handle.state = crate::runtime::values::ResourceState::Active;
            
            // Create new environment with resource binding
            let mut resource_env = Environment::with_parent(Rc::new(env.clone()));
            resource_env.define(&with_expr.resource_symbol, Value::Resource(handle.clone()));
            
            // Execute body and handle cleanup
            let body_result = self.eval_do_body(&with_expr.body, &mut resource_env);
            
            // Always attempt cleanup, regardless of body success/failure
            self.cleanup_resource(&mut handle)?;
            
            // Return original result or error
            body_result
        } else {
            Err(RuntimeError::TypeError {
                expected: "resource handle".to_string(),
                actual: resource_value.type_name().to_string(),
                operation: "with-resource".to_string(),
            })
        }
    }    fn eval_parallel(&self, parallel_expr: &ParallelExpr, env: &mut Environment) -> RuntimeResult<Value> {
        // For true parallel execution, we'd need to make the evaluator thread-safe
        // For now, implement structured concurrency simulation
        
        let mut result_map = std::collections::HashMap::new();
        
        // Execute each binding and collect results in a map
        for binding in &parallel_expr.bindings {
            let value = self.eval_expr(&binding.expression, env)?;
            
            // Use the binding symbol as the map key (as a keyword)
            let key = crate::ast::MapKey::Keyword(crate::ast::Keyword(binding.symbol.0.clone()));
            result_map.insert(key, value);
        }
        
        Ok(Value::Map(result_map))
    }
    
    fn eval_def(&self, def_expr: &DefExpr, env: &mut Environment) -> RuntimeResult<Value> {
        let mut value = self.eval_expr(&def_expr.value, env)?;
        
        // Apply type checking and coercion if type annotation is present
        if let Some(type_annotation) = &def_expr.type_annotation {
            value = self.coerce_value_to_type(value, type_annotation)?;
        }
        
        env.define(&def_expr.symbol, value.clone());
        Ok(value)
    }
    
    fn eval_defn(&self, defn_expr: &DefnExpr, env: &mut Environment) -> RuntimeResult<Value> {
        let function = Value::Function(Function::UserDefined {
            params: defn_expr.params.clone(),
            variadic_param: defn_expr.variadic_param.clone(),
            body: defn_expr.body.clone(),
            closure: env.clone(),
        });
        
        env.define(&defn_expr.name, function.clone());
        Ok(function)
    }
    
    /// Clean up a resource handle by calling its appropriate cleanup function
    fn cleanup_resource(&self, handle: &mut crate::runtime::values::ResourceHandle) -> RuntimeResult<()> {
        // Check if already released
        if handle.state == crate::runtime::values::ResourceState::Released {
            return Ok(());
        }
        
        // Determine cleanup function based on resource type
        let cleanup_result = match handle.resource_type.as_str() {
            "FileHandle" => {
                // Call tool:close-file or similar cleanup
                // For now, just log the cleanup
                println!("Cleaning up FileHandle: {}", handle.id);
                Ok(Value::Nil)
            },
            "DatabaseConnectionHandle" => {
                println!("Cleaning up DatabaseConnectionHandle: {}", handle.id);
                Ok(Value::Nil)
            },
            _ => {
                println!("Cleaning up generic resource: {} ({})", handle.resource_type, handle.id);
                Ok(Value::Nil)
            }
        };
        
        // Mark as released regardless of cleanup success
        handle.state = crate::runtime::values::ResourceState::Released;
        
        match cleanup_result {
            Ok(_) => Ok(()),
            Err(e) => Err(e),
        }
    }
      /// Check if a resource handle is valid for use
    #[allow(dead_code)]
    fn check_resource_state(&self, handle: &crate::runtime::values::ResourceHandle) -> RuntimeResult<()> {
        match handle.state {
            crate::runtime::values::ResourceState::Active => Ok(()),
            crate::runtime::values::ResourceState::Released => {
                Err(RuntimeError::ResourceError {
                    resource_type: handle.resource_type.clone(),
                    message: "Attempted to use released resource handle".to_string(),
                })
            }
        }
    }    /// Evaluate a discover-agents expression
    fn eval_discover_agents(&self, discover_expr: &crate::ast::DiscoverAgentsExpr, env: &mut Environment) -> RuntimeResult<Value> {
        // Evaluate the criteria expression to get a map
        let criteria_value = self.eval_expr(discover_expr.criteria.as_ref(), env)?;
        
        // Parse criteria map into SimpleDiscoveryQuery
        let query = match criteria_value {
            Value::Map(criteria_map) => {
                self.parse_criteria_to_query(&criteria_map)?
            },
            _ => return Err(RuntimeError::TypeError {
                expected: "Map".to_string(),
                actual: format!("{:?}", criteria_value),
                operation: "discover-agents criteria".to_string(),
            }),
        };
        
        // Parse options if provided
        let options = if let Some(options_expr) = &discover_expr.options {
            let options_value = self.eval_expr(options_expr.as_ref(), env)?;
            match options_value {
                Value::Map(options_map) => Some(self.parse_options_to_query(&options_map)?),
                _ => return Err(RuntimeError::TypeError {
                    expected: "Map".to_string(),
                    actual: format!("{:?}", options_value),
                    operation: "discover-agents options".to_string(),
                }),
            }
        } else {
            None
        };
        
        // Use the injected agent discovery service
        match self.agent_discovery.discover_agents(&query, options.as_ref()) {
            Ok(discovered_agents) => {
                // Convert to RTFS Vector value
                let agent_values: Vec<Value> = discovered_agents.into_iter().map(|agent| {
                    self.simple_agent_card_to_value(agent)
                }).collect();
                
                Ok(Value::Vector(agent_values))
            },
            Err(agent_error) => {
                Err(agent_error.into())
            }
        }
    }
      /// Parse a map of criteria into SimpleDiscoveryQuery
    fn parse_criteria_to_query(&self, criteria_map: &std::collections::HashMap<crate::ast::MapKey, Value>) -> RuntimeResult<SimpleDiscoveryQuery> {
        use crate::ast::{MapKey, Keyword};
        
        let mut query = SimpleDiscoveryQuery {
            capability_id: None,
            version_constraint: None,
            agent_id: None,
            discovery_tags: None,
            discovery_query: None,
            limit: None,
        };
        
        for (key, value) in criteria_map {
            match key {
                MapKey::Keyword(Keyword(keyword_name)) => {
                    match keyword_name.as_str() {
                        "capabilities" => {
                            let capabilities = self.parse_capabilities_list(value)?;
                            if !capabilities.is_empty() {
                                query.capability_id = Some(capabilities[0].clone());
                            }
                        },
                        "capability-id" | "capability_id" => {
                            query.capability_id = Some(self.parse_string_value(value)?);
                        },
                        "agent-id" | "agent_id" => {
                            query.agent_id = Some(self.parse_string_value(value)?);
                        },
                        "version" | "version-constraint" | "version_constraint" => {
                            query.version_constraint = Some(self.parse_string_value(value)?);
                        },
                        "tags" | "discovery-tags" | "discovery_tags" => {
                            query.discovery_tags = Some(self.parse_capabilities_list(value)?);
                        },
                        "limit" | "max-results" | "max_results" => {
                            match value {
                                Value::Integer(i) => {
                                    query.limit = Some(*i as u32);
                                },
                                _ => return Err(RuntimeError::TypeError {
                                    expected: "Integer".to_string(),
                                    actual: format!("{:?}", value),
                                    operation: "parsing limit".to_string(),
                                }),
                            }
                        },
                        _ => {
                            // Ignore unknown keys for now
                        }
                    }
                },
                _ => {
                    // Ignore non-keyword keys for now
                }
            }
        }
        
        Ok(query)
    }

    /// Parse discovery options from a map
    fn parse_options_to_query(&self, options_map: &std::collections::HashMap<crate::ast::MapKey, Value>) -> RuntimeResult<SimpleDiscoveryOptions> {
        use crate::ast::{MapKey, Keyword};
        
        let mut options = SimpleDiscoveryOptions {
            timeout_ms: None,
            cache_policy: None,
            include_offline: None,
            max_results: None,
        };
        
        for (key, value) in options_map {
            match key {
                MapKey::Keyword(Keyword(keyword_name)) => {
                    match keyword_name.as_str() {
                        "timeout" | "timeout-ms" | "timeout_ms" => {
                            match value {
                                Value::Integer(ms) => {
                                    options.timeout_ms = Some(*ms as u64);
                                },
                                _ => return Err(RuntimeError::TypeError {
                                    expected: "Integer".to_string(),
                                    actual: format!("{:?}", value),
                                    operation: "parsing timeout".to_string(),
                                }),
                            }
                        },
                        "cache" | "cache-policy" | "cache_policy" => {
                            match value {
                                Value::String(policy) => {
                                    use crate::agent::SimpleCachePolicy;
                                    options.cache_policy = Some(match policy.as_str() {
                                        "use-cache" | "use_cache" => SimpleCachePolicy::UseCache,
                                        "no-cache" | "no_cache" => SimpleCachePolicy::NoCache,
                                        "refresh-cache" | "refresh_cache" => SimpleCachePolicy::RefreshCache,
                                        _ => SimpleCachePolicy::UseCache,
                                    });
                                },
                                _ => return Err(RuntimeError::TypeError {
                                    expected: "String".to_string(),
                                    actual: format!("{:?}", value),
                                    operation: "parsing cache policy".to_string(),
                                }),
                            }
                        },
                        "include-offline" | "include_offline" => {
                            match value {
                                Value::Boolean(include) => {
                                    options.include_offline = Some(*include);
                                },
                                _ => return Err(RuntimeError::TypeError {
                                    expected: "Boolean".to_string(),
                                    actual: format!("{:?}", value),
                                    operation: "parsing include-offline".to_string(),
                                }),
                            }
                        },
                        "max-results" | "max_results" => {
                            match value {
                                Value::Integer(max) => {
                                    options.max_results = Some(*max as u32);
                                },
                                _ => return Err(RuntimeError::TypeError {
                                    expected: "Integer".to_string(),
                                    actual: format!("{:?}", value),
                                    operation: "parsing max-results".to_string(),
                                }),
                            }
                        },
                        _ => {
                            // Ignore unknown keys
                        }
                    }
                },
                _ => {
                    // Ignore non-keyword keys
                }
            }
        }
        
        Ok(options)
    }

    /// Convert a SimpleAgentCard to an RTFS Value
    fn simple_agent_card_to_value(&self, agent_card: SimpleAgentCard) -> Value {
        use std::collections::HashMap;
        
        let mut map = HashMap::new();
        
        // Add agent ID
        map.insert(
            crate::ast::MapKey::Keyword(crate::ast::Keyword("agent-id".to_string())),
            Value::String(agent_card.agent_id)
        );
        
        // Add name if present
        if let Some(name) = agent_card.name {
            map.insert(
                crate::ast::MapKey::Keyword(crate::ast::Keyword("name".to_string())),
                Value::String(name)
            );
        }
        
        // Add version if present
        if let Some(version) = agent_card.version {
            map.insert(
                crate::ast::MapKey::Keyword(crate::ast::Keyword("version".to_string())),
                Value::String(version)
            );
        }
        
        // Add capabilities
        let capabilities: Vec<Value> = agent_card.capabilities.into_iter()
            .map(|cap| Value::String(cap))
            .collect();
        map.insert(
            crate::ast::MapKey::Keyword(crate::ast::Keyword("capabilities".to_string())),
            Value::Vector(capabilities)
        );
        
        // Add endpoint if present
        if let Some(endpoint) = agent_card.endpoint {
            map.insert(
                crate::ast::MapKey::Keyword(crate::ast::Keyword("endpoint".to_string())),
                Value::String(endpoint)
            );
        }
        
        // Add metadata as a JSON string for now
        map.insert(
            crate::ast::MapKey::Keyword(crate::ast::Keyword("metadata".to_string())),
            Value::String(agent_card.metadata.to_string())
        );
        
        Value::Map(map)
    }

    /// Helper function to parse capabilities list from a value
    fn parse_capabilities_list(&self, value: &Value) -> RuntimeResult<Vec<String>> {
        match value {
            Value::Vector(vec) => {
                let mut capabilities = Vec::new();
                for item in vec {
                    match item {
                        Value::String(s) => capabilities.push(s.clone()),
                        _ => return Err(RuntimeError::TypeError {
                            expected: "String".to_string(),
                            actual: format!("{:?}", item),
                            operation: "parsing capability".to_string(),
                        }),
                    }
                }
                Ok(capabilities)
            },
            Value::String(s) => Ok(vec![s.clone()]),
            _ => Err(RuntimeError::TypeError {
                expected: "Vector or String".to_string(),
                actual: format!("{:?}", value),
                operation: "parsing capabilities".to_string(),
            }),
        }
    }    /// Helper function to parse a string value
    fn parse_string_value(&self, value: &Value) -> RuntimeResult<String> {
        match value {
            Value::String(s) => Ok(s.clone()),
            _ => Err(RuntimeError::TypeError {
                expected: "String".to_string(),
                actual: format!("{:?}", value),
                operation: "parsing string value".to_string(),
            }),
        }
    }
      /// Match a match pattern against a value (placeholder implementation)
    fn match_match_pattern(&self, pattern: &crate::ast::MatchPattern, value: &Value, env: &mut Environment) -> RuntimeResult<bool> {
        match pattern {
            crate::ast::MatchPattern::Symbol(symbol) => {
                env.define(symbol, value.clone());
                Ok(true)
            },
            crate::ast::MatchPattern::Wildcard => {
                Ok(true) // Wildcard always matches
            },
            crate::ast::MatchPattern::Literal(lit_pattern) => {
                let lit_value = self.eval_literal(lit_pattern)?;
                Ok(lit_value == *value)
            },
            crate::ast::MatchPattern::Keyword(keyword_pattern) => {
                Ok(*value == Value::Keyword(keyword_pattern.clone()))
            },
            crate::ast::MatchPattern::Vector { elements, rest } => {
                if let Value::Vector(values) = value {
                    if let Some(rest_symbol) = rest {
                        // Pattern with a rest part (e.g., [a b ..c])
                        if values.len() < elements.len() {
                            return Ok(false); // Not enough values to match fixed part
                        }
                        // Match fixed elements
                        let mut temp_env = env.clone();
                        let mut all_matched = true;
                        for (p, v) in elements.iter().zip(values.iter()) {
                            if !self.match_match_pattern(p, v, &mut temp_env)? {
                                all_matched = false;
                                break;
                            }
                        }

                        if all_matched {
                            *env = temp_env;
                            // Bind rest
                            let rest_values = values.iter().skip(elements.len()).cloned().collect();
                            env.define(rest_symbol, Value::Vector(rest_values));
                            Ok(true)
                        } else {
                            Ok(false)
                        }
                    } else {
                        // Fixed-length vector pattern
                        if elements.len() != values.len() {
                            return Ok(false);
                        }
                        
                        let mut temp_env = env.clone();
                        let mut all_matched = true;
                        for (p, v) in elements.iter().zip(values.iter()) {
                            if !self.match_match_pattern(p, v, &mut temp_env)? {
                                all_matched = false;
                                break;
                            }
                        }

                        if all_matched {
                            *env = temp_env;
                            Ok(true)
                        } else {
                            Ok(false)
                        }
                    }
                } else {
                    Ok(false) // Pattern is a vector, but value is not
                }
            },
            _ => Err(RuntimeError::NotImplemented(format!("Complex match pattern matching not yet implemented for: {:?}", pattern))),
        }
    }
    
    /// Match a catch pattern against an error value (placeholder implementation)
    fn match_catch_pattern_actual(&self, pattern: &crate::ast::CatchPattern, _error_value: &Value) -> RuntimeResult<bool> {
        match pattern {
            crate::ast::CatchPattern::Symbol(_symbol) => {
                // Symbols always match in catch clauses
                Ok(true)
            },
            _ => Err(RuntimeError::NotImplemented("Complex catch pattern matching not yet implemented".to_string())),
        }
    }
    
    /// Coerce a value to a specific type (placeholder implementation) 
    fn coerce_value_to_type(&self, value: Value, _type_annotation: &crate::ast::TypeExpr) -> RuntimeResult<Value> {
        // For now, just return the value as-is
        // TODO: Implement actual type coercion logic
        Ok(value)
    }

    /// Bind a pattern to a value in an environment (placeholder implementation)
    fn bind_pattern(&self, pattern: &crate::ast::Pattern, value: &Value, env: &mut Environment) -> RuntimeResult<()> {
        match pattern {
            crate::ast::Pattern::Symbol(symbol) => {
                env.define(symbol, value.clone());
                Ok(())
            },
            crate::ast::Pattern::Wildcard => {
                // Wildcard does nothing, successfully "matches" any value.
                Ok(())
            },
            _ => Err(RuntimeError::NotImplemented("Complex patterns not yet supported".to_string()))
        }
    }
}

impl Default for Evaluator {    fn default() -> Self {
        Self::new()
    }
}
