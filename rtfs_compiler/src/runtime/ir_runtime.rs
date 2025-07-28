// IR Runtime - Efficient execution engine for typed RTFS IR
// This runtime leverages type information and pre-resolved bindings for performance

use super::environment::IrEnvironment;
use super::error::RuntimeError;
use super::module_runtime::ModuleRegistry;
use super::values::{Function, Value, BuiltinFunctionWithContext};
use crate::ast::{Expression, Keyword, MapKey};
use crate::ccos::delegation::{CallContext, DelegationEngine, ExecTarget, ModelRegistry};
use crate::ir::converter::IrConverter;
use crate::ir::core::{IrNode, IrPattern};
use crate::runtime::RuntimeStrategy;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use crate::ccos::delegation_l4::L4AwareDelegationEngine;
use crate::ccos::caching::l4_content_addressable::L4CacheClient;
use crate::bytecode::{WasmExecutor, BytecodeExecutor};

/// A `RuntimeStrategy` that uses the `IrRuntime`.
/// It owns both the runtime and the module registry, breaking the dependency cycle.
#[derive(Clone, Debug)]
pub struct IrStrategy {
    runtime: IrRuntime,
    module_registry: ModuleRegistry,
    delegation_engine: Arc<dyn DelegationEngine>,
}

impl IrStrategy {
    pub fn new(mut module_registry: ModuleRegistry) -> Self {
        // Load stdlib into the module registry if not already loaded
        let _ = crate::runtime::stdlib::load_stdlib(&mut module_registry);
        let inner = crate::ccos::delegation::StaticDelegationEngine::new(HashMap::new());
        let l4_client = L4CacheClient::new();
        let wrapped = L4AwareDelegationEngine::new(l4_client, inner);
        let delegation_engine: Arc<dyn crate::ccos::delegation::DelegationEngine> = Arc::new(wrapped);
        Self {
            runtime: IrRuntime::new(delegation_engine.clone()),
            module_registry,
            delegation_engine,
        }
    }

    pub fn with_delegation_engine(mut module_registry: ModuleRegistry, delegation_engine: Arc<dyn DelegationEngine>) -> Self {
        // Load stdlib into the module registry if not already loaded
        let _ = crate::runtime::stdlib::load_stdlib(&mut module_registry);
        Self {
            runtime: IrRuntime::new(delegation_engine.clone()),
            module_registry,
            delegation_engine,
        }
    }
}

impl RuntimeStrategy for IrStrategy {
    fn run(&mut self, program: &Expression) -> Result<Value, RuntimeError> {
        let mut converter = IrConverter::with_module_registry(&self.module_registry);
        let ir_node = converter
            .convert_expression(program.clone())
            .map_err(|e| RuntimeError::Generic(format!("IR conversion error: {:?}", e)))?;

        // Create a program node from the single expression
        let program_node = IrNode::Program {
            id: converter.next_id(),
            version: "1.0".to_string(),
            forms: vec![ir_node],
            source_location: None,
        };

        self.runtime
            .execute_program(&program_node, &mut self.module_registry)
    }

    fn clone_box(&self) -> Box<dyn RuntimeStrategy> {
        Box::new(self.clone())
    }
}

/// The Intermediate Representation (IR) runtime.
/// Executes a program represented in IR form.
#[derive(Clone, Debug)]
pub struct IrRuntime {
    delegation_engine: Arc<dyn DelegationEngine>,
    model_registry: Arc<ModelRegistry>,
}

impl IrRuntime {
    /// Creates a new IR runtime.
    pub fn new(delegation_engine: Arc<dyn DelegationEngine>) -> Self {
        let model_registry = Arc::new(ModelRegistry::with_defaults());
        IrRuntime { 
            delegation_engine,
            model_registry,
        }
    }

    /// Executes a program by running its top-level forms.
    pub fn execute_program(
        &mut self,
        program_node: &IrNode,
        module_registry: &mut ModuleRegistry,
    ) -> Result<Value, RuntimeError> {
        let forms = match program_node {
            IrNode::Program { forms, .. } => forms,
            _ => return Err(RuntimeError::new("Expected Program node")),
        };

        let mut env = IrEnvironment::with_stdlib(module_registry)?;
        let mut result = Value::Nil;

        for node in forms {
            result = self.execute_node(node, &mut env, false, module_registry)?;
        }

        Ok(result)
    }

    /// Executes a single node in the IR graph.
    pub fn execute_node(
        &mut self,
        node: &IrNode,
        env: &mut IrEnvironment,
        is_tail_call: bool,
        module_registry: &mut ModuleRegistry,
    ) -> Result<Value, RuntimeError> {
        match node {
            IrNode::Literal { value, .. } => Ok(value.clone().into()),
            IrNode::VariableRef { name, .. } => env
                .get(name)
                .ok_or_else(|| RuntimeError::Generic(format!("Undefined variable: {}", name))),
            IrNode::VariableDef {
                name, init_expr, ..
            } => {
                let value_to_assign = self.execute_node(init_expr, env, false, module_registry)?;
                env.define(name.clone(), value_to_assign);
                Ok(Value::Nil)
            }
            IrNode::Lambda {
                params,
                variadic_param,
                body,
                ..
            } => {
                let function = Value::Function(Function::new_ir_lambda(
                    params.clone(),
                    variadic_param.clone(),
                    body.clone(),
                    Box::new(env.clone()),
                ));
                Ok(function)
            }
            IrNode::FunctionDef { name, lambda, .. } => {
                let function_val = self.execute_node(lambda, env, false, module_registry)?;
                env.define(name.clone(), function_val.clone());
                Ok(function_val)
            }
            IrNode::Apply {
                function,
                arguments,
                ..
            } => self.execute_call(function, arguments, env, is_tail_call, module_registry),
            IrNode::QualifiedSymbolRef { module, symbol, .. } => {
                let qualified_name = format!("{}/{}", module, symbol);
                module_registry.resolve_qualified_symbol(&qualified_name)
            }
            IrNode::If {
                condition,
                then_branch,
                else_branch,
                ..
            } => {
                let cond_value = self.execute_node(condition, env, false, module_registry)?;

                if cond_value.is_truthy() {
                    self.execute_node(then_branch, env, is_tail_call, module_registry)
                } else if let Some(alternative) = else_branch {
                    self.execute_node(alternative, env, is_tail_call, module_registry)
                } else {
                    Ok(Value::Nil)
                }
            }
            IrNode::Import { module_name, .. } => {
                self.execute_import(module_name, env, module_registry)
            }
            IrNode::Module { definitions, .. } => {
                for def in definitions {
                    self.execute_node(def, env, false, module_registry)?;
                }
                Ok(Value::Nil)
            }
            IrNode::Do { expressions, .. } => {
                let mut result = Value::Nil;
                for expr in expressions {
                    result = self.execute_node(expr, env, false, module_registry)?;
                }
                Ok(result)
            }
            IrNode::Let { bindings, body, .. } => {
                // Two-pass letrec logic: first insert placeholders for function bindings
                let mut placeholders = Vec::new();
                // First pass: create placeholders for all function bindings
                for binding in bindings {
                    if let IrNode::VariableBinding { name, .. } = &binding.pattern {
                        if let IrNode::Lambda { .. } = &binding.init_expr {
                            let placeholder_cell = Rc::new(RefCell::new(Value::Nil));
                            env.define(
                                name.clone(),
                                Value::FunctionPlaceholder(placeholder_cell.clone()),
                            );
                            placeholders.push((name.clone(), &binding.init_expr, placeholder_cell));
                        }
                    }
                }
                // Second pass: evaluate all function bindings and update placeholders
                for (name, lambda_node, placeholder_cell) in &placeholders {
                    let value = self.execute_node(lambda_node, env, false, module_registry)?;
                    if matches!(value, Value::Function(_)) {
                        *placeholder_cell.borrow_mut() = value;
                    } else {
                        return Err(RuntimeError::Generic(format!(
                            "letrec: expected function for {}",
                            name
                        )));
                    }
                }
                // Now handle non-function bindings as usual
                for binding in bindings {
                    match &binding.pattern {
                        IrNode::VariableBinding { name, .. } => {
                            if !placeholders.iter().any(|(n, _, _)| n == name) {
                                let value = self.execute_node(
                                    &binding.init_expr,
                                    env,
                                    false,
                                    module_registry,
                                )?;
                                env.define(name.clone(), value);
                            }
                        }
                        IrNode::Destructure { pattern, .. } => {
                            let value =
                                self.execute_node(&binding.init_expr, env, false, module_registry)?;
                            self.execute_destructure(pattern, &value, env, module_registry)?;
                        }
                        _ => {
                            // For other pattern types, just evaluate the init_expr
                            let value =
                                self.execute_node(&binding.init_expr, env, false, module_registry)?;
                            // Could add more specific handling here if needed
                        }
                    }
                }
                // Execute body
                let mut result = Value::Nil;
                for expr in body {
                    result = self.execute_node(expr, env, is_tail_call, module_registry)?;
                }
                Ok(result)
            }
            IrNode::Vector { elements, .. } => {
                let values: Result<Vec<Value>, RuntimeError> = elements
                    .iter()
                    .map(|elem| self.execute_node(elem, env, false, module_registry))
                    .collect();
                Ok(Value::Vector(values?))
            }
            IrNode::Map { entries, .. } => {
                let mut map = HashMap::new();
                for entry in entries {
                    let key = self.execute_node(&entry.key, env, false, module_registry)?;
                    let value = self.execute_node(&entry.value, env, false, module_registry)?;
                    // Convert key to MapKey format
                    let map_key = match key {
                        Value::Keyword(k) => MapKey::Keyword(k),
                        Value::String(s) => MapKey::String(s),
                        Value::Integer(i) => MapKey::Integer(i),
                        _ => return Err(RuntimeError::Generic("Invalid map key type".to_string())),
                    };
                    map.insert(map_key, value);
                }
                Ok(Value::Map(map))
            }
            IrNode::Match {
                expression,
                clauses,
                ..
            } => {
                let value = self.execute_node(expression, env, false, module_registry)?;
                // For now, implement basic pattern matching
                for clause in clauses {
                    // Simple implementation - just check if pattern matches
                    if self.pattern_matches(&clause.pattern, &value)? {
                        if let Some(guard) = &clause.guard {
                            let guard_result =
                                self.execute_node(guard, env, false, module_registry)?;
                            if !guard_result.is_truthy() {
                                continue;
                            }
                        }
                        return self.execute_node(&clause.body, env, is_tail_call, module_registry);
                    }
                }
                Err(RuntimeError::Generic(
                    "No matching pattern found".to_string(),
                ))
            }
            IrNode::TryCatch {
                try_body,
                catch_clauses,
                finally_body,
                ..
            } => {
                // Execute try body
                let mut result = Value::Nil;
                for expr in try_body {
                    result = self.execute_node(expr, env, false, module_registry)?;
                }

                // If no exception, execute finally and return result
                if let Some(finally) = finally_body {
                    for expr in finally {
                        self.execute_node(expr, env, false, module_registry)?;
                    }
                }

                Ok(result)
                // Note: Exception handling would be implemented here
            }
            IrNode::Parallel { bindings, .. } => {
                // For now, execute bindings sequentially
                let mut results = HashMap::new();
                for binding in bindings {
                    let value =
                        self.execute_node(&binding.init_expr, env, false, module_registry)?;
                    if let IrNode::VariableBinding { name, .. } = &binding.binding {
                        results.insert(MapKey::Keyword(Keyword(name.clone())), value);
                    }
                }
                Ok(Value::Map(results))
            }
            IrNode::WithResource {
                binding,
                init_expr,
                body,
                ..
            } => {
                let resource = self.execute_node(init_expr, env, false, module_registry)?;
                // For now, just execute body (resource cleanup would be implemented here)
                let mut result = Value::Nil;
                for expr in body {
                    result = self.execute_node(expr, env, false, module_registry)?;
                }
                Ok(result)
            }
            IrNode::LogStep { level, values, .. } => {
                // Execute values and log them
                let mut log_values = Vec::new();
                for value_expr in values {
                    let value = self.execute_node(value_expr, env, false, module_registry)?;
                    log_values.push(value);
                }
                // For now, just return the last value
                Ok(log_values.last().cloned().unwrap_or(Value::Nil))
            }
            IrNode::TaskContextAccess { field_name, .. } => {
                // For now, return a placeholder value
                Ok(Value::String(format!("@{}", field_name.0)))
            }
            IrNode::DiscoverAgents { criteria, .. } => {
                // Execute criteria and return empty vector for now
                self.execute_node(criteria, env, false, module_registry)?;
                Ok(Value::Vector(vec![]))
            }
            IrNode::ResourceRef { name, .. } => {
                // For now, return the resource name as a string
                Ok(Value::String(format!("@{}", name)))
            }
            IrNode::Task { .. } => {
                // Task execution is complex - for now return Nil
                Ok(Value::Nil)
            }
            IrNode::Destructure { pattern, value, .. } => {
                let value = self.execute_node(value, env, false, module_registry)?;
                self.execute_destructure(pattern, &value, env, module_registry)?;
                Ok(Value::Nil)
            }
            _ => Err(RuntimeError::Generic(format!(
                "Execution for IR node {:?} is not yet implemented",
                node.id()
            ))),
        }
    }

    fn execute_import(
        &mut self,
        module_name: &str,
        env: &mut IrEnvironment,
        module_registry: &mut ModuleRegistry,
    ) -> Result<Value, RuntimeError> {
        let module = module_registry.load_module(module_name, self)?;

        for (name, value) in module.exports.borrow().iter() {
            env.define(name.clone(), value.value.clone());
        }

        Ok(Value::Nil)
    }

    fn execute_call(
        &mut self,
        callee_node: &IrNode,
        arg_nodes: &[IrNode],
        env: &mut IrEnvironment,
        is_tail_call: bool,
        module_registry: &mut ModuleRegistry,
    ) -> Result<Value, RuntimeError> {
        let callee_val = self.execute_node(callee_node, env, false, module_registry)?;

        let args: Vec<Value> = arg_nodes
            .iter()
            .map(|arg_node| self.execute_node(arg_node, env, false, module_registry))
            .collect::<Result<_, _>>()?;

        self.apply_function(callee_val, &args, env, is_tail_call, module_registry)
    }

    fn apply_function(
        &mut self,
        function: Value,
        args: &[Value],
        env: &mut IrEnvironment,
        _is_tail_call: bool,
        module_registry: &mut ModuleRegistry,
    ) -> Result<Value, RuntimeError> {
        match function {
            Value::FunctionPlaceholder(cell) => {
                let actual = cell.borrow().clone();
                self.apply_function(actual, args, env, _is_tail_call, module_registry)
            }
            Value::Function(ref f) => {
                // Check delegation for IR functions
                if let Function::Ir(_) = f {
                    // Try to find the function name by looking up the function value in the environment
                    let fn_symbol = env.find_function_name(&function).unwrap_or("unknown-function");
                    let ctx = CallContext {
                        fn_symbol,
                        arg_type_fingerprint: 0, // TODO: hash argument types
                        runtime_context_hash: 0, // TODO: hash runtime context
                        semantic_hash: None,
                        metadata: None,
                    };
                    match self.delegation_engine.decide(&ctx) {
                        ExecTarget::LocalPure => {
                            // Normal in-process execution (fall through)
                        }
                        ExecTarget::LocalModel(id) | ExecTarget::RemoteModel(id) => {
                            return self.execute_model_call(&id, args, env);
                        }
                        ExecTarget::L4CacheHit { storage_pointer, .. } => {
                            if let Some(cache) = module_registry.l4_cache() {
                                if let Some(_blob) = cache.get_blob(&storage_pointer) {
                                    let executor = WasmExecutor::new();
                                    return executor.execute_module(&_blob, fn_symbol, args);
                                } else {
                                    return Err(RuntimeError::Generic(format!(
                                        "L4 cache blob '{}' not found",
                                        storage_pointer
                                    )));
                                }
                            } else {
                                return Err(RuntimeError::Generic(
                                    "Module registry has no attached L4 cache".to_string(),
                                ));
                            }
                        }
                    }
                }

                match f {
                    Function::Native(native_fn) => (native_fn.func)(args.to_vec()),
                    Function::Builtin(builtin_fn) => {
                        // Special handling for map function to support user-defined functions
                        if builtin_fn.name == "map" && args.len() == 2 {
                            return self.handle_map_with_user_functions(
                                &args[0],
                                &args[1],
                                env,
                                module_registry,
                            );
                        }
                        (builtin_fn.func)(args.to_vec())
                    }
                    Function::BuiltinWithContext(builtin_fn) => {
                        // Implement BuiltinWithContext functions in IR runtime
                        // These functions need access to the execution context to handle user-defined functions
                        self.execute_builtin_with_context(builtin_fn, args.to_vec(), env, module_registry)
                    }
                    Function::Ir(ir_fn) => {
                        let param_names: Vec<String> = ir_fn
                            .params
                            .iter()
                            .map(|p| match p {
                                IrNode::VariableBinding { name, .. } => Ok(name.clone()),
                                IrNode::Param { binding, .. } => {
                                    if let IrNode::VariableBinding { name, .. } = binding.as_ref() {
                                        Ok(name.clone())
                                    } else {
                                        Err(RuntimeError::new("Expected VariableBinding inside Param"))
                                    }
                                }
                                _ => Err(RuntimeError::new("Expected symbol in lambda parameters")),
                            })
                            .collect::<Result<Vec<String>, RuntimeError>>()?;

                        let mut new_env = ir_fn.closure_env.new_child_for_ir(
                            &param_names,
                            args,
                            ir_fn.variadic_param.is_some(),
                        )?;
                        let mut result = Value::Nil;
                        for node in &ir_fn.body {
                            result = self.execute_node(node, &mut new_env, false, module_registry)?;
                        }
                        Ok(result)
                    }
                    _ => Err(RuntimeError::new(
                        "Calling this type of function from the IR runtime is not currently supported.",
                    )),
                }
            },
            Value::Keyword(keyword) => {
                // Keywords act as functions: (:key map) is equivalent to (get map :key)
                if args.len() == 1 {
                    match &args[0] {
                        Value::Map(map) => {
                            let map_key = crate::ast::MapKey::Keyword(keyword.clone());
                            Ok(map.get(&map_key).cloned().unwrap_or(Value::Nil))
                        }
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
                            let map_key = crate::ast::MapKey::Keyword(keyword.clone());
                            Ok(map.get(&map_key).cloned().unwrap_or(args[1].clone()))
                        }
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
            }
            _ => Err(RuntimeError::Generic(format!(
                "Not a function: {}",
                function.to_string()
            ))),
        }
    }

    fn execute_model_call(
        &self,
        model_id: &str,
        args: &[Value],
        _env: &mut IrEnvironment,
    ) -> Result<Value, RuntimeError> {
        // Convert arguments to a prompt string
        let prompt = self.args_to_prompt(args)?;
        
        // Look up the model provider
        let provider = self.model_registry.get(model_id)
            .ok_or_else(|| RuntimeError::NotImplemented(
                format!("Model provider '{}' not found", model_id)
            ))?;
        
        // Call the model
        let response = provider.infer(&prompt)
            .map_err(|e| RuntimeError::NotImplemented(
                format!("Model inference failed: {}", e)
            ))?;
        
        // Convert response back to RTFS value
        Ok(Value::String(response))
    }

    fn args_to_prompt(&self, args: &[Value]) -> Result<String, RuntimeError> {
        let mut prompt_parts = Vec::new();
        
        for (i, arg) in args.iter().enumerate() {
            let arg_str = match arg {
                Value::String(s) => s.clone(),
                Value::Integer(n) => n.to_string(),
                Value::Float(f) => f.to_string(),
                Value::Boolean(b) => b.to_string(),
                Value::Nil => "nil".to_string(),
                Value::Vector(v) => {
                    let elements: Vec<String> = v.iter()
                        .map(|v| match v {
                            Value::String(s) => s.clone(),
                            Value::Integer(n) => n.to_string(),
                            Value::Float(f) => f.to_string(),
                            Value::Boolean(b) => b.to_string(),
                            Value::Nil => "nil".to_string(),
                            _ => format!("{:?}", v),
                        })
                        .collect();
                    format!("[{}]", elements.join(" "))
                }
                _ => format!("{:?}", arg),
            };
            prompt_parts.push(format!("arg{}: {}", i, arg_str));
        }
        
        Ok(prompt_parts.join("; "))
    }

    fn handle_map_with_user_functions(
        &mut self,
        function: &Value,
        collection: &Value,
        env: &mut IrEnvironment,
        module_registry: &mut ModuleRegistry,
    ) -> Result<Value, RuntimeError> {
        let collection_vec = match collection {
            Value::Vector(v) => v.clone(),
            _ => {
                return Err(RuntimeError::TypeError {
                    expected: "vector".to_string(),
                    actual: collection.type_name().to_string(),
                    operation: "map".to_string(),
                })
            }
        };

        let mut result = Vec::new();
        for item in collection_vec {
            match function {
                Value::Function(Function::Builtin(builtin_func)) => {
                    // Use the stdlib implementation for builtin functions
                    let func_args = vec![item];
                    let mapped_value = (builtin_func.func)(func_args)?;
                    result.push(mapped_value);
                }
                Value::Function(Function::Closure(closure)) => {
                    // Call user-defined functions using the IR runtime
                    let func_args = vec![item];
                    let mapped_value = self.apply_function(
                        Value::Function(Function::Closure(closure.clone())),
                        &func_args,
                        env,
                        false,
                        module_registry,
                    )?;
                    result.push(mapped_value);
                }
                Value::Function(Function::Native(native_func)) => {
                    // Call native functions
                    let func_args = vec![item];
                    let mapped_value = (native_func.func)(func_args)?;
                    result.push(mapped_value);
                }
                Value::Function(Function::Ir(ir_func)) => {
                    // Call IR functions
                    let func_args = vec![item];
                    let mapped_value = self.apply_function(
                        Value::Function(Function::Ir(ir_func.clone())),
                        &func_args,
                        env,
                        false,
                        module_registry,
                    )?;
                    result.push(mapped_value);
                }
                _ => {
                    return Err(RuntimeError::TypeError {
                        expected: "function".to_string(),
                        actual: function.type_name().to_string(),
                        operation: "map".to_string(),
                    })
                }
            }
        }
        Ok(Value::Vector(result))
    }

    /// Execute BuiltinWithContext functions in IR runtime
    /// These functions need execution context to handle user-defined functions
    fn execute_builtin_with_context(
        &mut self,
        builtin_fn: &BuiltinFunctionWithContext,
        args: Vec<Value>,
        env: &mut IrEnvironment,
        module_registry: &mut ModuleRegistry,
    ) -> Result<Value, RuntimeError> {
        match builtin_fn.name.as_str() {
            "map" => self.ir_map_with_context(args, env, module_registry),
            "filter" => self.ir_filter_with_context(args, env, module_registry),
            "reduce" => self.ir_reduce_with_context(args, env, module_registry),
            _ => {
                // For other BuiltinWithContext functions, we need a proper evaluator
                // For now, return an error
                Err(RuntimeError::Generic(format!(
                    "BuiltinWithContext function '{}' not yet implemented in IR runtime",
                    builtin_fn.name
                )))
            }
        }
    }

    /// IR runtime implementation of map with context
    fn ir_map_with_context(
        &mut self,
        args: Vec<Value>,
        env: &mut IrEnvironment,
        module_registry: &mut ModuleRegistry,
    ) -> Result<Value, RuntimeError> {
        if args.len() != 2 {
            return Err(RuntimeError::ArityMismatch {
                function: "map".to_string(),
                expected: "2".to_string(),
                actual: args.len(),
            });
        }
        let function = &args[0];
        let collection = &args[1];
        
        let collection_vec = match collection {
            Value::Vector(v) => v.clone(),
            _ => {
                return Err(RuntimeError::TypeError {
                    expected: "vector".to_string(),
                    actual: collection.type_name().to_string(),
                    operation: "map".to_string(),
                })
            }
        };

        let mut result = Vec::new();
        for item in collection_vec {
            let mapped_value = match function {
                Value::Function(Function::Builtin(builtin_func)) => {
                    let func_args = vec![item];
                    (builtin_func.func)(func_args)?
                }
                Value::Function(Function::BuiltinWithContext(builtin_func)) => {
                    let func_args = vec![item];
                    self.execute_builtin_with_context(builtin_func, func_args, env, module_registry)?
                }
                Value::Function(func) => {
                    let func_args = vec![item];
                    self.apply_function(
                        Value::Function(func.clone()),
                        &func_args,
                        env,
                        false,
                        module_registry,
                    )?
                }
                _ => {
                    return Err(RuntimeError::TypeError {
                        expected: "function".to_string(),
                        actual: function.type_name().to_string(),
                        operation: "map".to_string(),
                    });
                }
            };
            result.push(mapped_value);
        }
        Ok(Value::Vector(result))
    }

    /// IR runtime implementation of filter with context
    fn ir_filter_with_context(
        &mut self,
        args: Vec<Value>,
        env: &mut IrEnvironment,
        module_registry: &mut ModuleRegistry,
    ) -> Result<Value, RuntimeError> {
        if args.len() != 2 {
            return Err(RuntimeError::ArityMismatch {
                function: "filter".to_string(),
                expected: "2".to_string(),
                actual: args.len(),
            });
        }
        let function = &args[0];
        let collection = &args[1];
        
        let collection_vec = match collection {
            Value::Vector(v) => v.clone(),
            _ => {
                return Err(RuntimeError::TypeError {
                    expected: "vector".to_string(),
                    actual: collection.type_name().to_string(),
                    operation: "filter".to_string(),
                })
            }
        };

        let mut result = Vec::new();
        for item in collection_vec {
            let keep = match function {
                Value::Function(Function::Builtin(builtin_func)) => {
                    let func_args = vec![item.clone()];
                    let v = (builtin_func.func)(func_args)?;
                    v.is_truthy()
                }
                Value::Function(Function::BuiltinWithContext(builtin_func)) => {
                    let func_args = vec![item.clone()];
                    let v = self.execute_builtin_with_context(builtin_func, func_args, env, module_registry)?;
                    v.is_truthy()
                }
                Value::Function(func) => {
                    let func_args = vec![item.clone()];
                    let v = self.apply_function(
                        Value::Function(func.clone()),
                        &func_args,
                        env,
                        false,
                        module_registry,
                    )?;
                    v.is_truthy()
                }
                _ => {
                    return Err(RuntimeError::TypeError {
                        expected: "function".to_string(),
                        actual: function.type_name().to_string(),
                        operation: "filter".to_string(),
                    });
                }
            };
            if keep {
                result.push(item);
            }
        }
        Ok(Value::Vector(result))
    }

    /// IR runtime implementation of reduce with context
    fn ir_reduce_with_context(
        &mut self,
        args: Vec<Value>,
        env: &mut IrEnvironment,
        module_registry: &mut ModuleRegistry,
    ) -> Result<Value, RuntimeError> {
        if args.len() < 2 || args.len() > 3 {
            return Err(RuntimeError::ArityMismatch {
                function: "reduce".to_string(),
                expected: "2 or 3".to_string(),
                actual: args.len(),
            });
        }
        let function = &args[0];
        let collection_arg_index = args.len() - 1;
        let collection = &args[collection_arg_index];
        let init_value = if args.len() == 3 { Some(&args[1]) } else { None };
        
        let collection_vec = match collection {
            Value::Vector(v) => v.clone(),
            _ => {
                return Err(RuntimeError::TypeError {
                    expected: "vector".to_string(),
                    actual: collection.type_name().to_string(),
                    operation: "reduce".to_string(),
                })
            }
        };

        if collection_vec.is_empty() {
            return if let Some(init) = init_value {
                Ok(init.clone())
            } else {
                Err(RuntimeError::Generic(
                    "reduce on empty collection with no initial value".to_string()
                ))
            };
        }

        let (mut accumulator, rest) = if let Some(init) = init_value {
            (init.clone(), collection_vec.as_slice())
        } else {
            (collection_vec[0].clone(), &collection_vec[1..])
        };
        
        for item in rest {
            accumulator = match function {
                Value::Function(Function::Builtin(builtin_func)) => {
                    let func_args = vec![accumulator, item.clone()];
                    (builtin_func.func)(func_args)?
                }
                Value::Function(Function::BuiltinWithContext(builtin_func)) => {
                    let func_args = vec![accumulator, item.clone()];
                    self.execute_builtin_with_context(builtin_func, func_args, env, module_registry)?
                }
                Value::Function(func) => {
                    let func_args = vec![accumulator, item.clone()];
                    self.apply_function(
                        Value::Function(func.clone()),
                        &func_args,
                        env,
                        false,
                        module_registry,
                    )?
                }
                _ => {
                    return Err(RuntimeError::TypeError {
                        expected: "function".to_string(),
                        actual: function.type_name().to_string(),
                        operation: "reduce".to_string(),
                    });
                }
            };
        }
        Ok(accumulator)
    }

    /// Check if a pattern matches a value
    fn pattern_matches(&self, pattern: &IrPattern, value: &Value) -> Result<bool, RuntimeError> {
        match pattern {
            IrPattern::Literal(lit) => {
                let pattern_value: Value = lit.clone().into();
                Ok(pattern_value == *value)
            }
            IrPattern::Variable(_name) => {
                // Variable patterns always match
                Ok(true)
            }
            IrPattern::Wildcard => {
                // Wildcard patterns always match
                Ok(true)
            }
            IrPattern::Vector { elements, rest } => {
                if let Value::Vector(vec_elements) = value {
                    if elements.len() > vec_elements.len() {
                        return Ok(false);
                    }
                    if rest.is_none() && elements.len() != vec_elements.len() {
                        return Ok(false);
                    }
                    // For now, just check if we have enough elements
                    Ok(true)
                } else {
                    Ok(false)
                }
            }
            IrPattern::Map { entries, rest } => {
                if let Value::Map(map) = value {
                    if entries.len() > map.len() {
                        return Ok(false);
                    }
                    if rest.is_none() && entries.len() != map.len() {
                        return Ok(false);
                    }
                    // For now, just check if we have enough entries
                    Ok(true)
                } else {
                    Ok(false)
                }
            }
            IrPattern::Type(_type_expr) => {
                // For now, all values match any type
                Ok(true)
            }
        }
    }

    /// Execute destructuring pattern and bind variables in the environment
    fn execute_destructure(
        &self,
        pattern: &IrPattern,
        value: &Value,
        env: &mut IrEnvironment,
        _module_registry: &mut ModuleRegistry,
    ) -> Result<(), RuntimeError> {
        match pattern {
            IrPattern::Literal(_) => {
                // Literal patterns don't bind anything
                Ok(())
            }
            IrPattern::Variable(name) => {
                // Simple variable binding
                env.define(name.clone(), value.clone());
                Ok(())
            }
            IrPattern::Wildcard => {
                // Wildcard patterns don't bind anything
                Ok(())
            }
            IrPattern::Vector { elements, rest } => {
                if let Value::Vector(vec_elements) = value {
                    // Bind each element to its corresponding pattern
                    for (i, element_pattern) in elements.iter().enumerate() {
                        if i < vec_elements.len() {
                            self.execute_destructure(
                                element_pattern,
                                &vec_elements[i],
                                env,
                                _module_registry,
                            )?;
                        }
                    }
                    // Bind rest pattern if present
                    if let Some(rest_name) = rest {
                        let rest_elements = vec_elements
                            .iter()
                            .skip(elements.len())
                            .cloned()
                            .collect::<Vec<_>>();
                        env.define(rest_name.clone(), Value::Vector(rest_elements));
                    }
                    Ok(())
                } else {
                    Err(RuntimeError::TypeError {
                        expected: "vector".to_string(),
                        actual: value.type_name().to_string(),
                        operation: "destructure".to_string(),
                    })
                }
            }
            IrPattern::Map { entries, rest } => {
                if let Value::Map(map) = value {
                    // Bind each entry to its corresponding pattern
                    for entry in entries {
                        if let Some(map_value) = map.get(&entry.key) {
                            self.execute_destructure(
                                &entry.pattern,
                                map_value,
                                env,
                                _module_registry,
                            )?;
                        }
                    }
                    // Bind rest pattern if present
                    if let Some(rest_name) = rest {
                        let mut rest_map = HashMap::new();
                        for (key, val) in map {
                            let key_matches = entries.iter().any(|entry| &entry.key == key);
                            if !key_matches {
                                rest_map.insert(key.clone(), val.clone());
                            }
                        }
                        env.define(rest_name.clone(), Value::Map(rest_map));
                    }
                    Ok(())
                } else {
                    Err(RuntimeError::TypeError {
                        expected: "map".to_string(),
                        actual: value.type_name().to_string(),
                        operation: "destructure".to_string(),
                    })
                }
            }
            IrPattern::Type(_) => {
                // Type patterns don't bind anything for now
                Ok(())
            }
        }
    }
}
