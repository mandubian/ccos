// RTFS Evaluator - Executes parsed AST nodes

use std::collections::HashMap;
use std::rc::Rc;
use crate::ast::*;
use crate::runtime::{Value, RuntimeError, RuntimeResult, Environment};
use crate::runtime::values::{Function, Arity};
use crate::runtime::stdlib::StandardLibrary;

pub struct Evaluator {
    global_env: Rc<Environment>,
}

impl Evaluator {
    /// Create a new evaluator with standard library loaded
    pub fn new() -> Self {
        let global_env = StandardLibrary::create_global_environment();
        Evaluator {
            global_env: Rc::new(global_env),
        }
    }
    
    /// Evaluate an expression in the global environment
    pub fn evaluate(&self, expr: &Expression) -> RuntimeResult<Value> {
        let mut env = Environment::with_parent(self.global_env.clone());
        self.eval_expr(expr, &mut env)
    }
    
    /// Evaluate an expression in a given environment
    pub fn eval_expr(&self, expr: &Expression, env: &mut Environment) -> RuntimeResult<Value> {
        match expr {
            Expression::Literal(lit) => self.eval_literal(lit),
            Expression::Symbol(sym) => env.lookup(sym),
            Expression::List(exprs) => {
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
            },
            Expression::FunctionCall { callee, arguments } => {
                let func_value = self.eval_expr(callee, env)?;
                let args: Result<Vec<Value>, RuntimeError> = arguments
                    .iter()
                    .map(|e| self.eval_expr(e, env))
                    .collect();
                let args = args?;
                
                self.call_function(func_value, &args, env)
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
        }
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
    
    fn call_function(&self, func_value: Value, args: &[Value], env: &mut Environment) -> RuntimeResult<Value> {
        match func_value {
            Value::Function(Function::Builtin { name, arity, func }) => {
                // Check arity
                if !self.check_arity(&arity, args.len()) {
                    return Err(RuntimeError::ArityMismatch {
                        function: name,
                        expected: self.arity_to_string(&arity),
                        actual: args.len(),
                    });
                }
                
                func(args)
            },
            Value::Function(Function::UserDefined { params, variadic_param, body, closure }) => {
                // Create new environment for function execution
                let mut func_env = Environment::with_parent(Rc::new(closure.clone()));
                
                // Bind parameters
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
                
                // Execute function body
                self.eval_do_body(&body, &mut func_env)
            },
            _ => Err(RuntimeError::TypeError {
                expected: "function".to_string(),
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
    }
    
    fn eval_let(&self, let_expr: &LetExpr, env: &mut Environment) -> RuntimeResult<Value> {
        // Create new scope for let bindings
        let mut let_env = Environment::with_parent(Rc::new(env.clone()));
        
        // Process bindings sequentially
        for binding in &let_expr.bindings {
            let value = self.eval_expr(&binding.value, &mut let_env)?;
            self.bind_pattern(&binding.pattern, &value, &mut let_env)?;
        }
        
        // Evaluate body in the new environment
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
            
            if self.match_pattern(&clause.pattern, &value, &mut match_env)? {
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
                    if self.match_catch_pattern(&catch_clause.pattern, &error_value)? {
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
        let value = self.eval_expr(&def_expr.value, env)?;
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
    }
    
    // Pattern matching helpers
    fn bind_pattern(&self, pattern: &Pattern, value: &Value, env: &mut Environment) -> RuntimeResult<()> {
        match pattern {
            Pattern::Symbol(symbol) => {
                env.define(symbol, value.clone());
                Ok(())
            },
            Pattern::Wildcard => Ok(()), // Wildcard binds nothing
            Pattern::VectorDestructuring { elements, rest, as_symbol } => {
                if let Some(as_sym) = as_symbol {
                    env.define(as_sym, value.clone());
                }
                
                match value {
                    Value::Vector(vec) => {
                        // Bind elements
                        for (i, elem_pattern) in elements.iter().enumerate() {
                            if let Some(elem_value) = vec.get(i) {
                                self.bind_pattern(elem_pattern, elem_value, env)?;
                            } else {
                                self.bind_pattern(elem_pattern, &Value::Nil, env)?;
                            }
                        }
                        
                        // Bind rest if present
                        if let Some(rest_symbol) = rest {
                            let rest_values = vec[elements.len()..].to_vec();
                            env.define(rest_symbol, Value::Vector(rest_values));
                        }
                        
                        Ok(())
                    },
                    _ => Err(RuntimeError::TypeError {
                        expected: "vector".to_string(),
                        actual: value.type_name().to_string(),
                        operation: "vector destructuring".to_string(),
                    }),
                }
            },
            Pattern::MapDestructuring { entries, rest, as_symbol } => {
                if let Some(as_sym) = as_symbol {
                    env.define(as_sym, value.clone());
                }
                
                match value {
                    Value::Map(map) => {
                        for entry in entries {
                            match entry {
                                MapDestructuringEntry::KeyBinding { key, pattern } => {
                                    let entry_value = map.get(key).cloned().unwrap_or(Value::Nil);
                                    self.bind_pattern(pattern, &entry_value, env)?;
                                },
                                MapDestructuringEntry::Keys(symbols) => {
                                    for symbol in symbols {
                                        let key = MapKey::Keyword(Keyword(symbol.0.clone()));
                                        let entry_value = map.get(&key).cloned().unwrap_or(Value::Nil);
                                        env.define(symbol, entry_value);
                                    }
                                },
                            }
                        }
                        
                        // TODO: Handle rest binding
                        if let Some(_rest_symbol) = rest {
                            // Implementation needed for rest destructuring
                        }
                        
                        Ok(())
                    },
                    _ => Err(RuntimeError::TypeError {
                        expected: "map".to_string(),
                        actual: value.type_name().to_string(),
                        operation: "map destructuring".to_string(),
                    }),
                }
            },
        }
    }
    
    fn match_pattern(&self, pattern: &MatchPattern, value: &Value, env: &mut Environment) -> RuntimeResult<bool> {
        match pattern {
            MatchPattern::Literal(lit) => {
                let lit_value = self.eval_literal(lit)?;
                Ok(lit_value == *value)
            },
            MatchPattern::Symbol(symbol) => {
                env.define(symbol, value.clone());
                Ok(true)
            },
            MatchPattern::Keyword(keyword) => {
                Ok(matches!(value, Value::Keyword(k) if k == keyword))
            },
            MatchPattern::Wildcard => Ok(true),
            MatchPattern::Type(_type_expr, binding) => {
                // TODO: Implement proper type matching
                if let Some(symbol) = binding {
                    env.define(symbol, value.clone());
                }
                Ok(true) // Placeholder - always matches for now
            },
            MatchPattern::Vector { elements, rest } => {
                match value {
                    Value::Vector(vec) => {
                        if vec.len() < elements.len() {
                            return Ok(false);
                        }
                        
                        // Match elements
                        for (i, elem_pattern) in elements.iter().enumerate() {
                            if !self.match_pattern(elem_pattern, &vec[i], env)? {
                                return Ok(false);
                            }
                        }
                        
                        // Bind rest if present
                        if let Some(rest_symbol) = rest {
                            let rest_values = vec[elements.len()..].to_vec();
                            env.define(rest_symbol, Value::Vector(rest_values));
                        }
                        
                        Ok(true)
                    },
                    _ => Ok(false),
                }
            },
            MatchPattern::Map { entries, rest } => {
                match value {
                    Value::Map(map) => {
                        // Match all required entries
                        for entry in entries {
                            if let Some(entry_value) = map.get(&entry.key) {
                                if !self.match_pattern(&entry.pattern, entry_value, env)? {
                                    return Ok(false);
                                }
                            } else {
                                return Ok(false);
                            }
                        }
                        
                        // TODO: Handle rest binding
                        if let Some(_rest_symbol) = rest {
                            // Implementation needed
                        }
                        
                        Ok(true)
                    },
                    _ => Ok(false),
                }
            },
            MatchPattern::As(symbol, inner_pattern) => {
                if self.match_pattern(inner_pattern, value, env)? {
                    env.define(symbol, value.clone());
                    Ok(true)
                } else {
                    Ok(false)
                }
            },
        }
    }
    
    fn match_catch_pattern(&self, pattern: &CatchPattern, error_value: &Value) -> RuntimeResult<bool> {
        match pattern {
            CatchPattern::Keyword(keyword) => {
                if let Value::Error(err) = error_value {
                    Ok(err.error_type == *keyword)
                } else {
                    Ok(false)
                }
            },
            CatchPattern::Type(_type_expr) => {
                // TODO: Implement proper type matching for catch patterns
                Ok(true) // Placeholder
            },
            CatchPattern::Symbol(_symbol) => {
                // Symbol patterns match any error (catch-all)
                Ok(matches!(error_value, Value::Error(_)))
            },
        }
    }
}

impl Default for Evaluator {
    fn default() -> Self {
        Self::new()
    }
}
