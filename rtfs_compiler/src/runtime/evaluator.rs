// RTFS Evaluator - Executes parsed AST nodes

use crate::agent::{SimpleAgentCard, SimpleDiscoveryOptions, SimpleDiscoveryQuery};
use crate::ast::{CatchPattern, DefExpr, DefnExpr, DoExpr, Expression, FnExpr, IfExpr, Keyword, LetExpr,
    Literal, LogStepExpr, MapKey, MatchExpr, ParallelExpr, Symbol, TopLevel, TryCatchExpr,
    WithResourceExpr};
use crate::runtime::environment::Environment;
use crate::runtime::error::{RuntimeError, RuntimeResult};
use crate::runtime::host_interface::HostInterface;
use crate::runtime::module_runtime::ModuleRegistry;
use crate::runtime::values::{Arity, Function, Value};
use crate::runtime::security::RuntimeContext;
use crate::runtime::type_validator::{TypeValidator, TypeCheckingConfig, ValidationLevel, VerificationContext};
use crate::ccos::types::ExecutionResult;
use crate::ccos::execution_context::{ContextManager, IsolationLevel};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use crate::ccos::delegation::{DelegationEngine, ExecTarget, CallContext, ModelRegistry};
use std::sync::{Arc, Mutex};
use crate::ccos::delegation::StaticDelegationEngine;
use crate::bytecode::{WasmExecutor, BytecodeExecutor};

type SpecialFormHandler = fn(&Evaluator, &[Expression], &mut Environment) -> RuntimeResult<Value>;

#[derive(Clone, Debug)]
pub struct Evaluator {
    module_registry: Rc<ModuleRegistry>,
    pub env: Environment,
    recursion_depth: usize,
    max_recursion_depth: usize,
    pub delegation_engine: Arc<dyn DelegationEngine>,
    pub model_registry: Arc<ModelRegistry>,
    /// Security context for capability execution
    pub security_context: RuntimeContext,
    /// Host interface for CCOS interactions
    pub host: Arc<dyn HostInterface>,
    /// Dispatch table for special forms
    special_forms: HashMap<String, SpecialFormHandler>,
    /// Type validator for hybrid validation
    pub type_validator: Arc<TypeValidator>,
    /// Type checking configuration for optimization
    pub type_config: TypeCheckingConfig,
    /// Context manager for hierarchical execution context
    pub context_manager: RefCell<ContextManager>,
}

// Helper function to check if two values are in equivalent
// This is a simplified version for the fixpoint algorithm
fn values_equivalent(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Nil, Value::Nil) => true,
        (Value::Function(Function::Closure(c1)), Value::Function(Function::Closure(c2))) => {
            c1.body == c2.body
        }
        (Value::Function(Function::Builtin(b1)), Value::Function(Function::Builtin(b2))) => {
            b1.name == b2.name
        }
        _ => false, // Different types or can't compare
    }
}

impl Evaluator {
    fn default_special_forms() -> HashMap<String, SpecialFormHandler> {
        let mut special_forms: HashMap<String, SpecialFormHandler> = HashMap::new();
        special_forms.insert("step".to_string(), Self::eval_step_form);
        special_forms.insert("step-if".to_string(), Self::eval_step_if_form);
        special_forms.insert("step-loop".to_string(), Self::eval_step_loop_form);
        special_forms.insert("step-parallel".to_string(), Self::eval_step_parallel_form);
        // Add other evaluator-level special forms here in the future
        
        // LLM execution bridge (M1)
        special_forms.insert("llm-execute".to_string(), Self::eval_llm_execute_form);

        special_forms
    }

    /// Create a new evaluator with secure environment and default security context
    pub fn new(
        module_registry: Rc<ModuleRegistry>, 
        delegation_engine: Arc<dyn DelegationEngine>,
        security_context: RuntimeContext,
        host: Arc<dyn HostInterface>,
    ) -> Self {
        let env = crate::runtime::stdlib::StandardLibrary::create_global_environment();
        let model_registry = Arc::new(ModelRegistry::with_defaults());

        Evaluator {
            module_registry,
            env,
            recursion_depth: 0,
            max_recursion_depth: 1000,
            delegation_engine,
            model_registry,
            security_context,
        host,
            special_forms: Self::default_special_forms(),
            type_validator: Arc::new(TypeValidator::new()),
            type_config: TypeCheckingConfig::default(),
            context_manager: RefCell::new(ContextManager::new()),
        }
    }

    /// Create a new evaluator with task context and security


    /// Create a new evaluator with default security context (pure)
    pub fn new_with_defaults(
        module_registry: Rc<ModuleRegistry>, 
        delegation_engine: Arc<dyn DelegationEngine>,
        host: Arc<dyn HostInterface>,
    ) -> Self {
        Self::new(
            module_registry,
            delegation_engine,
            RuntimeContext::pure(),
        host,
        )
    }



    /// Configure type checking behavior
    pub fn set_type_checking_config(&mut self, config: TypeCheckingConfig) {
        self.type_config = config;
    }

    /// Get current type checking configuration
    pub fn get_type_checking_config(&self) -> &TypeCheckingConfig {
        &self.type_config
    }

    /// Create evaluator with optimized type checking for production
    pub fn new_optimized(
        module_registry: Rc<ModuleRegistry>,
        delegation_engine: Arc<dyn DelegationEngine>,
        security_context: RuntimeContext,
        host: Arc<dyn HostInterface>,
    ) -> Self {
        let mut evaluator = Self::new(module_registry, delegation_engine, security_context, host);
        evaluator.type_config = TypeCheckingConfig {
            skip_compile_time_verified: true,
            enforce_capability_boundaries: true,
            validate_external_data: true,
            validation_level: ValidationLevel::Standard,
        };
        evaluator
    }

    /// Create evaluator with strict type checking for development
    pub fn new_strict(
        module_registry: Rc<ModuleRegistry>,
        delegation_engine: Arc<dyn DelegationEngine>,
        security_context: RuntimeContext,
        host: Rc<dyn HostInterface>,
    ) -> Self {
        let mut evaluator = Self::new(module_registry, delegation_engine, security_context, host);
        evaluator.type_config = TypeCheckingConfig {
            skip_compile_time_verified: false,
            enforce_capability_boundaries: true,
            validate_external_data: true,
            validation_level: ValidationLevel::Strict,
        };
        evaluator
    }

    /// Create verification context for local expression evaluation
    fn create_local_verification_context(&self, compile_time_verified: bool) -> VerificationContext {
        VerificationContext {
            compile_time_verified,
            is_capability_boundary: false,
            is_external_data: false,
            source_location: None,
            trust_level: if compile_time_verified { 
                crate::runtime::type_validator::TrustLevel::Trusted 
            } else { 
                crate::runtime::type_validator::TrustLevel::Verified 
            },
        }
    }

    /// Validate a value against an expected type with current configuration
    fn validate_expression_result(
        &self, 
        value: &Value, 
        expected_type: &crate::ast::TypeExpr,
        compile_time_verified: bool,
    ) -> RuntimeResult<()> {
        let context = self.create_local_verification_context(compile_time_verified);
        self.type_validator.validate_with_config(value, expected_type, &self.type_config, &context)
            .map_err(|e| RuntimeError::TypeValidationError(e.to_string()))
    }

    /// Validate arguments for known builtin functions
    fn validate_builtin_function_args(&self, function_name: &str, args: &[Value]) -> RuntimeResult<()> {
        // Only validate if we're in strict mode or if this is a security-critical function
        if !self.should_validate_function_call(function_name) {
            return Ok(());
        }

        // Validate arguments for well-known functions with type information
        match function_name {
            // Arithmetic functions - require numbers
            "+" | "-" | "*" | "/" | "mod" => {
                for (i, arg) in args.iter().enumerate() {
                    if !matches!(arg, Value::Integer(_) | Value::Float(_)) {
                        let expected_type = crate::ast::TypeExpr::Union(vec![
                            crate::ast::TypeExpr::Primitive(crate::ast::PrimitiveType::Int),
                            crate::ast::TypeExpr::Primitive(crate::ast::PrimitiveType::Float),
                        ]);
                        let context = self.create_local_verification_context(false); // Runtime values
                        return self.type_validator.validate_with_config(arg, &expected_type, &self.type_config, &context)
                            .map_err(|e| RuntimeError::TypeValidationError(
                                format!("Function '{}' argument {}: {}", function_name, i, e)
                            ));
                    }
                }
            }
            // Note: Standard library functions (str, string-length, count, etc.) are NOT optimized here
            // They should be validated through their own function signatures, not hardcoded in the evaluator
            // Only true language primitives should be optimized in this validation
            _ => {
                // Unknown function - no specific validation
            }
        }
        Ok(())
    }

    /// Validate results for known builtin functions
    fn validate_builtin_function_result(&self, function_name: &str, result: &Value, _args: &[Value]) -> RuntimeResult<()> {
        // Only validate if we're in strict mode
        if !self.should_validate_function_result(function_name) {
            return Ok(());
        }

        // Validate return types for well-known functions
        match function_name {
            // Arithmetic functions return numbers
            "+" | "-" | "*" | "/" | "mod" => {
                if !matches!(result, Value::Integer(_) | Value::Float(_)) {
                    let expected_type = crate::ast::TypeExpr::Union(vec![
                        crate::ast::TypeExpr::Primitive(crate::ast::PrimitiveType::Int),
                        crate::ast::TypeExpr::Primitive(crate::ast::PrimitiveType::Float),
                    ]);
                    let context = self.create_local_verification_context(false);
                    return self.type_validator.validate_with_config(result, &expected_type, &self.type_config, &context)
                        .map_err(|e| RuntimeError::TypeValidationError(
                            format!("Function '{}' should return number: {}", function_name, e)
                        ));
                }
            }
            // Note: Standard library functions (str, string-length, count, etc.) are NOT optimized here
            // They should be validated through their own function signatures, not hardcoded in the evaluator
            // Only true language primitives should be optimized in this result validation
            _ => {
                // Unknown function - no specific validation
            }
        }
        Ok(())
    }

    /// Determine if we should validate arguments for this function call
    fn should_validate_function_call(&self, function_name: &str) -> bool {
        // Always validate in strict mode
        if self.type_config.validation_level == ValidationLevel::Strict {
            return true;
        }

        // In standard mode, validate security-critical functions
        if self.type_config.validation_level == ValidationLevel::Standard {
            return matches!(function_name, "eval" | "load" | "exec" | "read-file" | "write-file");
        }

        // In basic mode, skip validation unless it's a known unsafe function
        false
    }

    /// Determine if we should validate results for this function call  
    fn should_validate_function_result(&self, function_name: &str) -> bool {
        // Only validate results in strict mode for demonstration
        self.type_config.validation_level == ValidationLevel::Strict 
            && !matches!(function_name, "print" | "println") // Skip output functions
    }

    pub fn eval_toplevel(&mut self, program: &[TopLevel]) -> RuntimeResult<Value> {
        let mut env = self.env.clone();
        let mut last_value = Value::Nil;
        for toplevel in program {
            match toplevel {
                TopLevel::Expression(expr) => {
                    last_value = self.eval_expr(expr, &mut env)?;
                }
                TopLevel::Intent(intent) => {
                    // Evaluate intent properties and return intent metadata
                    let mut intent_metadata = HashMap::new();
                    for property in &intent.properties {
                        let key = crate::ast::MapKey::String(property.key.0.clone());
                        let value = self.eval_expr(&property.value, &mut env)?;
                        intent_metadata.insert(key, value);
                    }
                    last_value = Value::Map(intent_metadata);
                }
                TopLevel::Plan(plan) => {
                    // Evaluate plan properties and return plan metadata
                    let mut plan_metadata = HashMap::new();
                    for property in &plan.properties {
                        let key = crate::ast::MapKey::String(property.key.0.clone());
                        let value = self.eval_expr(&property.value, &mut env)?;
                        plan_metadata.insert(key, value);
                    }
                    last_value = Value::Map(plan_metadata);
                }
                TopLevel::Action(action) => {
                    // Evaluate action properties and return action metadata
                    let mut action_metadata = HashMap::new();
                    for property in &action.properties {
                        let key = crate::ast::MapKey::String(property.key.0.clone());
                        let value = self.eval_expr(&property.value, &mut env)?;
                        action_metadata.insert(key, value);
                    }
                    last_value = Value::Map(action_metadata);
                }
                TopLevel::Capability(capability) => {
                    // Evaluate capability properties and return capability metadata
                    let mut capability_metadata = HashMap::new();
                    for property in &capability.properties {
                        let key = crate::ast::MapKey::String(property.key.0.clone());
                        let value = self.eval_expr(&property.value, &mut env)?;
                        capability_metadata.insert(key, value);
                    }
                    last_value = Value::Map(capability_metadata);
                }
                TopLevel::Resource(resource) => {
                    // Evaluate resource properties and return resource metadata
                    let mut resource_metadata = HashMap::new();
                    for property in &resource.properties {
                        let key = crate::ast::MapKey::String(property.key.0.clone());
                        let value = self.eval_expr(&property.value, &mut env)?;
                        resource_metadata.insert(key, value);
                    }
                    last_value = Value::Map(resource_metadata);
                }
                TopLevel::Module(module) => {
                    // Evaluate module properties and return module metadata
                    let mut module_metadata = HashMap::new();
                    // Add module name
                    module_metadata.insert(
                        crate::ast::MapKey::String("name".to_string()),
                        Value::String(module.name.0.clone()),
                    );
                    // Add docstring if present
                    if let Some(docstring) = &module.docstring {
                        module_metadata.insert(
                            crate::ast::MapKey::String("docstring".to_string()),
                            Value::String(docstring.clone()),
                        );
                    }
                    // Add exports if present
                    if let Some(exports) = &module.exports {
                        let export_values: Vec<Value> =
                            exports.iter().map(|e| Value::String(e.0.clone())).collect();
                        module_metadata.insert(
                            crate::ast::MapKey::String("exports".to_string()),
                            Value::Vector(export_values),
                        );
                    }
                    last_value = Value::Map(module_metadata);
                }
            }
        }
        self.env = env;
        Ok(last_value)
    }

    /// Evaluate an expression in a given environment
    pub fn eval_expr(&self, expr: &Expression, env: &mut Environment) -> RuntimeResult<Value> {

        match expr {
            Expression::Literal(lit) => self.eval_literal(lit),
            Expression::Symbol(sym) => env
                .lookup(sym)
                .ok_or_else(|| RuntimeError::UndefinedSymbol(sym.clone())),
            Expression::List(list) => {
                if list.is_empty() {
                    return Ok(Value::Vector(vec![]));
                }

                if let Expression::Symbol(s) = &list[0] {
                    if let Some(handler) = self.special_forms.get(&s.0) {
                        return handler(self, &list[1..], env);
                    }
                }

                // It's a regular function call
                let func_expr = &list[0];
                let func_value = self.eval_expr(func_expr, env)?;

                let args: Result<Vec<Value>, RuntimeError> =
                    list[1..].iter().map(|e| self.eval_expr(e, env)).collect();
                let args = args?;

                self.call_function(func_value, &args, env)
            }
            Expression::Vector(exprs) => {
                let values: Result<Vec<Value>, RuntimeError> =
                    exprs.iter().map(|e| self.eval_expr(e, env)).collect();
                Ok(Value::Vector(values?))
            }
            Expression::Map(map) => {
                let mut result = HashMap::new();
                for (key, value_expr) in map {
                    let value = self.eval_expr(value_expr, env)?;
                    result.insert(key.clone(), value);
                }
                Ok(Value::Map(result))
            }
            Expression::FunctionCall { callee, arguments } => {
                // Check if this is a special form before evaluating the callee
                if let Expression::Symbol(s) = &**callee {
                    if let Some(handler) = self.special_forms.get(&s.0) {
                        return handler(self, arguments, env);
                    }
                }
                
                let func_value = self.eval_expr(callee, env)?;

                match &func_value {
                    Value::Function(Function::Builtin(f)) if f.name == "quote" => {
                        // Handle quote specially
                        if arguments.len() != 1 {
                            return Err(RuntimeError::ArityMismatch {
                                function: "quote".to_string(),
                                expected: "1".to_string(),
                                actual: arguments.len(),
                            });
                        }
                        // Return the argument unevaluated
                        return Ok(Value::from(arguments[0].clone()));
                    }
                    _ => {
                        // Evaluate arguments and call normally
                        let args: RuntimeResult<Vec<Value>> =
                            arguments.iter().map(|e| self.eval_expr(e, env)).collect();
                        let args = args?;

                        self.call_function(func_value, &args, env)
                    }
                }
            }
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
            Expression::DiscoverAgents(discover_expr) => {
                self.eval_discover_agents(discover_expr, env)
            }
            Expression::ResourceRef(s) => Ok(Value::String(s.clone())),
        }
    }

    fn eval_step_form(&self, args: &[Expression], env: &mut Environment) -> RuntimeResult<Value> {
        // 1. Validate arguments: "name" ...body
        if args.is_empty() { 
            return Err(RuntimeError::InvalidArguments {
                expected: "at least 1 (a string name)".to_string(),
                actual: "0".to_string(),
            });
        }

        let step_name = match &args[0] {
            Expression::Literal(crate::ast::Literal::String(s)) => s.clone(),
            _ => return Err(RuntimeError::InvalidArguments {
                expected: "a string for the step name".to_string(),
                actual: format!("{:?}", args[0]),
            }),
        };

        // 2. Enter step context
        let mut context_manager = self.context_manager.borrow_mut();
        let _ = context_manager.enter_step(&step_name, IsolationLevel::Inherit)?;
        drop(context_manager); // Release borrow before calling host

        // 3. Notify host that step has started
        let step_action_id = self.host.notify_step_started(&step_name)?;

        // 4. Evaluate the body of the step
        let body_exprs = &args[1..];
        let mut last_result = Ok(Value::Nil);

        for expr in body_exprs {
            last_result = self.eval_expr(expr, env);
            if let Err(e) = &last_result {
                // On failure, notify host and propagate the error
                self.host.notify_step_failed(&step_action_id, &e.to_string())?;
                
                // Exit step context on failure
                let mut context_manager = self.context_manager.borrow_mut();
                context_manager.exit_step()?;
                return last_result;
            }
        }

        // 5. Notify host of successful completion
        let final_value = last_result?;
        let exec_result = ExecutionResult {
            success: true,
            value: final_value.clone(),
            metadata: Default::default(),
        };
        self.host.notify_step_completed(&step_action_id, &exec_result)?;

        // 6. Exit step context
        let mut context_manager = self.context_manager.borrow_mut();
        context_manager.exit_step()?;

        Ok(final_value)
    }

    fn eval_step_if_form(&self, args: &[Expression], env: &mut Environment) -> RuntimeResult<Value> {
        // Validate arguments: condition then-branch [else-branch]
        if args.len() < 2 {
            return Err(RuntimeError::InvalidArguments {
                expected: "at least 2 (condition then-branch [else-branch])".to_string(),
                actual: args.len().to_string(),
            });
        }

        // Extract condition and branches
        let condition_expr = &args[0];
        let then_branch = &args[1];
        let else_branch = args.get(2);

        // 1. Notify host that step-if has started
        let step_name = "step-if";
        let step_action_id = self.host.notify_step_started(step_name)?;

        // 2. Evaluate the condition
        let condition_value = match self.eval_expr(condition_expr, env) {
            Ok(value) => value,
            Err(e) => {
                // On failure, notify host and propagate the error
                self.host.notify_step_failed(&step_action_id, &e.to_string())?;
                return Err(e);
            }
        };
        
        // Convert condition to boolean
        let condition_bool = match condition_value {
            Value::Boolean(b) => b,
            Value::Nil => false,
            Value::Integer(0) => false,
            Value::Float(f) if f == 0.0 => false,
            Value::String(s) if s.is_empty() => false,
            Value::Vector(v) if v.is_empty() => false,
            Value::Map(m) if m.is_empty() => false,
            _ => true, // Any other value is considered true
        };

        // 3. Execute the appropriate branch
        let branch_to_execute = if condition_bool { then_branch } else { else_branch.unwrap_or(&Expression::Literal(crate::ast::Literal::Nil)) };
        
        let result = match self.eval_expr(branch_to_execute, env) {
            Ok(value) => value,
            Err(e) => {
                // On failure, notify host and propagate the error
                self.host.notify_step_failed(&step_action_id, &e.to_string())?;
                return Err(e);
            }
        };

        // 4. Notify host of successful completion
        let exec_result = ExecutionResult {
            success: true,
            value: result.clone(),
            metadata: Default::default(),
        };
        self.host.notify_step_completed(&step_action_id, &exec_result)?;

        Ok(result)
    }

    fn eval_step_loop_form(&self, args: &[Expression], env: &mut Environment) -> RuntimeResult<Value> {
        // Validate arguments: condition body
        if args.len() != 2 {
            return Err(RuntimeError::InvalidArguments {
                expected: "exactly 2 (condition body)".to_string(),
                actual: args.len().to_string(),
            });
        }

        let condition_expr = &args[0];
        let body_expr = &args[1];
        
        // 1. Notify host that step-loop has started
        let step_name = "step-loop";
        let step_action_id = self.host.notify_step_started(step_name)?;
        
        let mut last_result = Value::Nil;
        let mut iteration_count = 0;
        const MAX_ITERATIONS: usize = 10000; // Safety limit to prevent infinite loops

        loop {
            // Check iteration limit
            if iteration_count >= MAX_ITERATIONS {
                let error_msg = format!("Loop exceeded maximum iterations ({})", MAX_ITERATIONS);
                self.host.notify_step_failed(&step_action_id, &error_msg)?;
                return Err(RuntimeError::Generic(error_msg));
            }

            // Evaluate the condition
            let condition_value = match self.eval_expr(condition_expr, env) {
                Ok(value) => value,
                Err(e) => {
                    // On failure, notify host and propagate the error
                    self.host.notify_step_failed(&step_action_id, &e.to_string())?;
                    return Err(e);
                }
            };
            
            // Convert condition to boolean
            let condition_bool = match condition_value {
                Value::Boolean(b) => b,
                Value::Nil => false,
                Value::Integer(0) => false,
                Value::Float(f) if f == 0.0 => false,
                Value::String(s) if s.is_empty() => false,
                Value::Vector(v) if v.is_empty() => false,
                Value::Map(m) if m.is_empty() => false,
                _ => true, // Any other value is considered true
            };

            // If condition is false, break the loop
            if !condition_bool {
                break;
            }

            // Execute the body
            last_result = match self.eval_expr(body_expr, env) {
                Ok(value) => value,
                Err(e) => {
                    // On failure, notify host and propagate the error
                    self.host.notify_step_failed(&step_action_id, &e.to_string())?;
                    return Err(e);
                }
            };
            iteration_count += 1;
        }

        // 2. Notify host of successful completion
        let exec_result = ExecutionResult {
            success: true,
            value: last_result.clone(),
            metadata: Default::default(),
        };
        self.host.notify_step_completed(&step_action_id, &exec_result)?;

        Ok(last_result)
    }

    fn eval_step_parallel_form(&self, args: &[Expression], env: &mut Environment) -> RuntimeResult<Value> {
        // Validate arguments: at least one expression to execute in parallel
        if args.is_empty() {
            return Err(RuntimeError::InvalidArguments {
                expected: "at least 1 expression to execute in parallel".to_string(),
                actual: "0".to_string(),
            });
        }

        // 1. Notify host that step-parallel has started
        let step_name = "step-parallel";
        let step_action_id = self.host.notify_step_started(step_name)?;

        // 2. Parse optional keyword arguments (e.g., :merge-policy :overwrite)
        use crate::ast::Literal as Lit;
        let mut i: usize = 0;
        let mut merge_policy = crate::ccos::execution_context::ConflictResolution::KeepExisting;
        while i + 1 < args.len() {
            match &args[i] {
                Expression::Literal(Lit::Keyword(k)) => {
                    let key = k.0.as_str();
                    // Read value expression
                    let val_expr = &args[i + 1];
                    // Evaluate simple literal/string/keyword values only
                    let val = match val_expr {
                        Expression::Literal(Lit::Keyword(kw)) => Value::Keyword(kw.clone()),
                        Expression::Literal(Lit::String(s)) => Value::String(s.clone()),
                        other => {
                            // Stop parsing options if non-literal encountered to avoid
                            // consuming actual branch expressions
                            if i == 0 {
                                // Unknown option style; treat as invalid args
                                return Err(RuntimeError::InvalidArguments {
                                    expected: "keyword-value pairs (e.g., :merge-policy :overwrite) followed by branch expressions".to_string(),
                                    actual: format!("{:?}", other),
                                });
                            }
                            break;
                        }
                    };

                    if key == "merge-policy" || key == "merge_policy" {
                        // Map value to ConflictResolution
                        merge_policy = match val {
                            Value::Keyword(crate::ast::Keyword(s)) | Value::String(s) => {
                                match s.as_str() {
                                    "keep-existing" | "keep_existing" | "parent-wins" | "parent_wins" => crate::ccos::execution_context::ConflictResolution::KeepExisting,
                                    "overwrite" | "child-wins" | "child_wins" => crate::ccos::execution_context::ConflictResolution::Overwrite,
                                    "merge" => crate::ccos::execution_context::ConflictResolution::Merge,
                                    other => {
                                        return Err(RuntimeError::InvalidArguments {
                                            expected: ":keep-existing | :overwrite | :merge".to_string(),
                                            actual: other.to_string(),
                                        });
                                    }
                                }
                            }
                            _ => crate::ccos::execution_context::ConflictResolution::KeepExisting,
                        };
                        i += 2;
                        continue;
                    } else {
                        // Unknown option - stop parsing and treat as branch start
                        break;
                    }
                }
                _ => break,
            }
        }

        // 3. Create and use isolated contexts per branch on demand

        // Sequential execution with isolation, plus deterministic merging after each branch
        let mut results: Vec<Value> = Vec::with_capacity(args.len().saturating_sub(i));
        let mut last_error: Option<RuntimeError> = None;
        for (rel_index, expr) in args[i..].iter().enumerate() {
            let index = i + rel_index;
            // Begin isolated child context for this branch (also switches into it)
            let child_id = {
                let mut mgr = self.context_manager.borrow_mut();
                mgr.begin_isolated(&format!("parallel-{}", index))?
            };
            match self.eval_expr(expr, env) {
                Ok(v) => {
                    results.push(v);
                    // Merge child to parent with selected policy and switch back to parent
                    let mut mgr = self.context_manager.borrow_mut();
                    let _ = mgr.end_isolated(&child_id, merge_policy);
                }
                Err(e) => {
                    if last_error.is_none() {
                        last_error = Some(e);
                    }
                }
            }
        }
        if let Some(err) = last_error {
            self.host.notify_step_failed(&step_action_id, &err.to_string())?;
            return Err(err);
        }

        // 3. Notify host of successful completion
        let final_result = Value::Vector(results);
        let exec_result = ExecutionResult {
            success: true,
            value: final_result.clone(),
            metadata: Default::default(),
        };
        self.host.notify_step_completed(&step_action_id, &exec_result)?;

        Ok(final_result)
    }

    /// LLM execution bridge special form
    /// Usage:
    ///   (llm-execute "model-id" "prompt")
    ///   (llm-execute :model "model-id" :prompt "prompt text" [:system "system prompt"]) 
    fn eval_llm_execute_form(&self, args: &[Expression], env: &mut Environment) -> RuntimeResult<Value> {
        // Enforce security policy
        let capability_id = "ccos.ai.llm-execute";
        if !self.security_context.is_capability_allowed(capability_id) {
            return Err(RuntimeError::SecurityViolation {
                operation: "llm-execute".to_string(),
                capability: capability_id.to_string(),
                context: "capability not allowed in current RuntimeContext".to_string(),
            });
        }

        // Parse arguments
        let mut model_id: Option<String> = None;
        let mut prompt: Option<String> = None;
        let mut system_prompt: Option<String> = None;

        if args.len() == 2 {
            let m = self.eval_expr(&args[0], env)?;
            let p = self.eval_expr(&args[1], env)?;
            match (m, p) {
                (Value::String(mstr), Value::String(pstr)) => {
                    model_id = Some(mstr);
                    prompt = Some(pstr);
                }
                (mv, pv) => {
                    return Err(RuntimeError::InvalidArguments {
                        expected: "(llm-execute \"model-id\" \"prompt\") with both arguments as strings".to_string(),
                        actual: format!("model={:?}, prompt={:?}", mv, pv),
                    });
                }
            }
        } else if !args.is_empty() {
            // Parse keyword arguments: :model, :prompt, optional :system
            let mut i = 0;
            while i < args.len() {
                // Expect a keyword
                let key = match &args[i] {
                    Expression::Literal(Literal::Keyword(k)) => k.0.clone(),
                    other => {
                        return Err(RuntimeError::InvalidArguments {
                            expected: "keyword-value pairs (e.g., :model \"id\" :prompt \"text\")".to_string(),
                            actual: format!("{:?}", other),
                        });
                    }
                };
                i += 1;
                if i >= args.len() {
                    return Err(RuntimeError::InvalidArguments {
                        expected: format!("a value after keyword {}", key),
                        actual: "end-of-list".to_string(),
                    });
                }
                let val = self.eval_expr(&args[i], env)?;
                match (key.as_str(), val) {
                    ("model", Value::String(s)) => model_id = Some(s),
                    ("prompt", Value::String(s)) => prompt = Some(s),
                    ("system", Value::String(s)) => system_prompt = Some(s),
                    (k, v) => {
                        return Err(RuntimeError::InvalidArguments {
                            expected: format!("string value for {}", k),
                            actual: format!("{:?}", v),
                        });
                    }
                }
                i += 1;
            }
        } else {
            return Err(RuntimeError::InvalidArguments {
                expected: "either 2 positional args or keyword args (:model, :prompt)".to_string(),
                actual: "0".to_string(),
            });
        }

        let model_id = model_id.unwrap_or_else(|| "echo-model".to_string());
        let prompt = match prompt {
            Some(p) => p,
            None => {
                return Err(RuntimeError::InvalidArguments {
                    expected: "a :prompt string".to_string(),
                    actual: "missing".to_string(),
                })
            }
        };

        // Notify host that llm-execute has started
        let step_action_id = self.host.notify_step_started("llm-execute")?;

        // Compose final prompt
        let final_prompt = if let Some(sys) = system_prompt {
            format!("System:\n{}\n\nUser:\n{}", sys, prompt)
        } else {
            prompt.clone()
        };

        // Resolve provider and execute
        let provider = self.model_registry.get(&model_id).ok_or_else(|| {
            RuntimeError::UnknownCapability(format!("Model provider not found: {}", model_id))
        })?;

        match provider.infer(&final_prompt) {
            Ok(output) => {
                let value = Value::String(output);
                let exec_result = ExecutionResult { success: true, value: value.clone(), metadata: Default::default() };
                self.host.notify_step_completed(&step_action_id, &exec_result)?;
                Ok(value)
            }
            Err(e) => {
                let msg = format!("LLM provider '{}' error: {}", model_id, e);
                self.host.notify_step_failed(&step_action_id, &msg)?;
                Err(RuntimeError::Generic(msg))
            }
        }
    }

    /// Evaluate an expression in the global environment
    pub fn evaluate(&self, expr: &Expression) -> RuntimeResult<Value> {
        let mut env = self.env.clone();
        self.eval_expr(expr, &mut env)
    }

    /// Evaluate an expression with a provided environment
    pub fn evaluate_with_env(
        &self,
        expr: &Expression,
        env: &mut Environment,
    ) -> RuntimeResult<Value> {
        self.eval_expr(expr, env)
    }

    fn eval_literal(&self, lit: &Literal) -> RuntimeResult<Value> {
        // Literals are compile-time verified, so we can create optimized verification context
        let value = match lit {
            Literal::Integer(n) => Value::Integer(*n),
            Literal::Float(f) => Value::Float(*f),
            Literal::String(s) => Value::String(s.clone()),
            Literal::Boolean(b) => Value::Boolean(*b),
            Literal::Keyword(k) => Value::Keyword(k.clone()),
            Literal::Nil => Value::Nil,
            Literal::Timestamp(ts) => Value::String(ts.clone()),
            Literal::Uuid(uuid) => Value::String(uuid.clone()),
            Literal::ResourceHandle(handle) => Value::String(handle.clone()),
        };

        // Demonstrate the optimization system for literal values
        // Note: In a real implementation, you'd have type annotations from the parser
        if self.type_config.skip_compile_time_verified {
            // This is the fast path - skip validation for compile-time verified literals
            // The type was already verified when the literal was parsed
            Ok(value)
        } else {
            // Development/debug mode - validate even compile-time verified values
            // For demonstration, we'll create basic type expressions for literals
            let inferred_type = match lit {
                Literal::Integer(_) => crate::ast::TypeExpr::Primitive(crate::ast::PrimitiveType::Int),
                Literal::Float(_) => crate::ast::TypeExpr::Primitive(crate::ast::PrimitiveType::Float),
                Literal::String(_) => crate::ast::TypeExpr::Primitive(crate::ast::PrimitiveType::String),
                Literal::Boolean(_) => crate::ast::TypeExpr::Primitive(crate::ast::PrimitiveType::Bool),
                Literal::Keyword(_) => crate::ast::TypeExpr::Primitive(crate::ast::PrimitiveType::Keyword),
                Literal::Nil => crate::ast::TypeExpr::Primitive(crate::ast::PrimitiveType::Nil),
                Literal::Timestamp(_) | Literal::Uuid(_) | Literal::ResourceHandle(_) => 
                    crate::ast::TypeExpr::Primitive(crate::ast::PrimitiveType::String),
            };

            // Validate using the optimization system
            let context = self.create_local_verification_context(true); // compile_time_verified = true
            self.type_validator.validate_with_config(&value, &inferred_type, &self.type_config, &context)
                .map_err(|e| RuntimeError::TypeValidationError(e.to_string()))?;

            Ok(value)
        }
    }

    pub fn call_function(
        &self,
        func_value: Value,
        args: &[Value],
        env: &mut Environment,
    ) -> RuntimeResult<Value> {
        match func_value {
            Value::FunctionPlaceholder(cell) => {
                let f = cell.borrow().clone();
                if let Value::Function(f) = f {
                    self.call_function(Value::Function(f), args, env)
                } else {
                    Err(RuntimeError::InternalError(
                        "Function placeholder not resolved".to_string(),
                    ))
                }
            }
            Value::Function(Function::Builtin(func)) => {
                // Special handling for map function to support user-defined functions
                if func.name == "map" && args.len() == 2 {
                    return self.handle_map_with_user_functions(&args[0], &args[1], env);
                }

                // Check arity
                if !self.check_arity(&func.arity, args.len()) {
                    return Err(RuntimeError::ArityMismatch {
                        function: func.name,
                        expected: self.arity_to_string(&func.arity),
                        actual: args.len(),
                    });
                }

                // Validate function arguments with known signatures (demonstration)
                self.validate_builtin_function_args(&func.name, args)?;

                // Call the function
                let result = (func.func)(args.to_vec())?;

                // Validate function result for known signatures
                self.validate_builtin_function_result(&func.name, &result, args)?;

                Ok(result)
            }
            Value::Function(Function::BuiltinWithContext(func)) => {
                // Check arity
                if !self.check_arity(&func.arity, args.len()) {
                    return Err(RuntimeError::ArityMismatch {
                        function: func.name,
                        expected: self.arity_to_string(&func.arity),
                        actual: args.len(),
                    });
                }

                (func.func)(args.to_vec(), self, env)
            }
            Value::Function(Function::Closure(ref closure)) => {
                // Delegation fast-path: if the function carries a delegation hint we act on it
                if let Some(hint) = &closure.delegation_hint {
                    use crate::ast::DelegationHint as DH;
                    match hint {
                        DH::LocalPure => {
                            // Normal in-process execution (fall through)
                        }
                        DH::LocalModel(id) | DH::RemoteModel(id) => {
                            return self.execute_model_call(id, args, env);
                        }
                    }
                } else {
                    // No hint: consult DelegationEngine
                    // Try to find the function name by looking up the function value in the environment
                    let fn_symbol = env.find_function_name(&func_value).unwrap_or("unknown-function");
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
                            if let Some(cache) = self.module_registry.l4_cache() {
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
                // Create new environment for function execution, parented by the captured closure
                let mut func_env = Environment::with_parent(closure.env.clone());

                // Bind arguments to parameters
                for (param, arg) in closure.params.iter().zip(args.iter()) {
                    func_env.define(param, arg.clone());
                }

                // Execute function body
                self.eval_expr(&closure.body, &mut func_env)
            }
            Value::Keyword(keyword) => {
                // Keywords act as functions: (:key map) is equivalent to (get map :key)
                if args.len() == 1 {
                    match &args[0] {
                        Value::Map(map) => {
                            let map_key = crate::ast::MapKey::Keyword(keyword);
                            Ok(map.get(&map_key).cloned().unwrap_or(Value::Nil))
                        }
                        _ => Err(RuntimeError::TypeError {
                            expected: "map".to_string(),
                            actual: match &args[0] {
                                Value::Nil => "Nil".to_string(),
                                Value::Boolean(_) => "Boolean".to_string(),
                                Value::Integer(_) => "Integer".to_string(),
                                Value::Float(_) => "Float".to_string(),
                                Value::String(_) => "String".to_string(),
                                Value::Timestamp(_) => "Timestamp".to_string(),
                                Value::Uuid(_) => "Uuid".to_string(),
                                Value::ResourceHandle(_) => "ResourceHandle".to_string(),
                                Value::Symbol(_) => "Symbol".to_string(),
                                Value::Keyword(_) => "Keyword".to_string(),
                                Value::Vector(_) => "Vector".to_string(),
                                Value::List(_) => "List".to_string(),
                                Value::Map(_) => "Map".to_string(),
                                Value::Function(_) => "Function".to_string(),
                                Value::FunctionPlaceholder(_) => "FunctionPlaceholder".to_string(),
                                Value::Error(_) => "Error".to_string(),
                            },
                            operation: "keyword lookup".to_string(),
                        }),
                    }
                } else if args.len() == 2 {
                    // (:key map default) is equivalent to (get map :key default)
                    match &args[0] {
                        Value::Map(map) => {
                            let map_key = crate::ast::MapKey::Keyword(keyword);
                            Ok(map.get(&map_key).cloned().unwrap_or(args[1].clone()))
                        }
                        _ => Err(RuntimeError::TypeError {
                            expected: "map".to_string(),
                            actual: match &args[0] {
                                Value::Nil => "Nil".to_string(),
                                Value::Boolean(_) => "Boolean".to_string(),
                                Value::Integer(_) => "Integer".to_string(),
                                Value::Float(_) => "Float".to_string(),
                                Value::String(_) => "String".to_string(),
                                Value::Timestamp(_) => "Timestamp".to_string(),
                                Value::Uuid(_) => "Uuid".to_string(),
                                Value::ResourceHandle(_) => "ResourceHandle".to_string(),
                                Value::Symbol(_) => "Symbol".to_string(),
                                Value::Keyword(_) => "Keyword".to_string(),
                                Value::Vector(_) => "Vector".to_string(),
                                Value::List(_) => "List".to_string(),
                                Value::Map(_) => "Map".to_string(),
                                Value::Function(_) => "Function".to_string(),
                                Value::FunctionPlaceholder(_) => "FunctionPlaceholder".to_string(),
                                Value::Error(_) => "Error".to_string(),
                            },
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
            _ => Err(RuntimeError::TypeError {
                expected: "function".to_string(),
                actual: func_value.type_name().to_string(),
                operation: "function call".to_string(),
            }),
        }
    }

    fn check_arity(&self, arity: &Arity, arg_count: usize) -> bool {
        match arity {
            Arity::Fixed(n) => arg_count == *n,
            Arity::Variadic(n) => arg_count >= *n,
            Arity::Range(min, max) => arg_count >= *min && arg_count <= *max,
        }
    }

    fn arity_to_string(&self, arity: &Arity) -> String {
        match arity {
            Arity::Fixed(n) => n.to_string(),
            Arity::Variadic(n) => format!("at least {}", n),
            Arity::Range(min, max) => format!("between {} and {}", min, max),
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
        // Only use recursion detection for function bindings
        let all_bindings_are_functions = let_expr
            .bindings
            .iter()
            .all(|binding| matches!(&*binding.value, Expression::Fn(_) | Expression::Defn(_)));
        if all_bindings_are_functions && self.detect_recursion_in_let(&let_expr.bindings) {
            self.eval_let_with_recursion(let_expr, env)
        } else {
            self.eval_let_simple(let_expr, env)
        }
    }

    fn detect_recursion_in_let(&self, bindings: &[crate::ast::LetBinding]) -> bool {
        // Collect all binding names
        let binding_names: std::collections::HashSet<&str> = bindings
            .iter()
            .filter_map(|b| {
                if let crate::ast::Pattern::Symbol(s) = &b.pattern {
                    Some(s.0.as_str())
                } else {
                    None
                }
            })
            .collect();

        // Check if any binding value references other binding names
        for binding in bindings {
            if self.expr_references_symbols(&binding.value, &binding_names) {
                return true;
            }
        }
        false
    }

    fn expr_references_symbols(
        &self,
        expr: &Expression,
        symbols: &std::collections::HashSet<&str>,
    ) -> bool {
        match expr {
            Expression::Symbol(s) => symbols.contains(s.0.as_str()),
            Expression::FunctionCall { callee, arguments } => {
                // Check function name
                if let Expression::Symbol(s) = &**callee {
                    if symbols.contains(s.0.as_str()) {
                        return true;
                    }
                }
                // Check arguments
                for arg in arguments {
                    if self.expr_references_symbols(arg, symbols) {
                        return true;
                    }
                }
                false
            }
            Expression::Let(let_expr) => {
                // Check bindings and body
                for binding in &let_expr.bindings {
                    if self.expr_references_symbols(&binding.value, symbols) {
                        return true;
                    }
                }
                for body_expr in &let_expr.body {
                    if self.expr_references_symbols(body_expr, symbols) {
                        return true;
                    }
                }
                false
            }
            Expression::If(if_expr) => {
                self.expr_references_symbols(&if_expr.condition, symbols)
                    || self.expr_references_symbols(&if_expr.then_branch, symbols)
                    || if_expr.else_branch.as_ref().map_or(false, |else_expr| {
                        self.expr_references_symbols(else_expr, symbols)
                    })
            }
            Expression::Do(do_expr) => do_expr
                .expressions
                .iter()
                .any(|expr| self.expr_references_symbols(expr, symbols)),
            Expression::Fn(fn_expr) => fn_expr
                .body
                .iter()
                .any(|expr| self.expr_references_symbols(expr, symbols)),
            Expression::Def(def_expr) => self.expr_references_symbols(&def_expr.value, symbols),
            Expression::Defn(defn_expr) => defn_expr
                .body
                .iter()
                .any(|expr| self.expr_references_symbols(expr, symbols)),
            Expression::Match(match_expr) => {
                self.expr_references_symbols(&match_expr.expression, symbols)
                    || match_expr.clauses.iter().any(|clause| {
                        clause
                            .guard
                            .as_ref()
                            .map_or(false, |guard| self.expr_references_symbols(guard, symbols))
                            || self.expr_references_symbols(&clause.body, symbols)
                    })
            }
            Expression::TryCatch(try_expr) => {
                try_expr
                    .try_body
                    .iter()
                    .any(|expr| self.expr_references_symbols(expr, symbols))
                    || try_expr.catch_clauses.iter().any(|clause| {
                        clause
                            .body
                            .iter()
                            .any(|expr| self.expr_references_symbols(expr, symbols))
                    })
            }
            Expression::WithResource(with_expr) => {
                self.expr_references_symbols(&with_expr.resource_init, symbols)
                    || with_expr
                        .body
                        .iter()
                        .any(|expr| self.expr_references_symbols(expr, symbols))
            }
            Expression::Parallel(parallel_expr) => parallel_expr
                .bindings
                .iter()
                .any(|binding| self.expr_references_symbols(&binding.expression, symbols)),
            Expression::DiscoverAgents(discover_expr) => {
                self.expr_references_symbols(&discover_expr.criteria, symbols)
                    || discover_expr.options.as_ref().map_or(false, |options| {
                        self.expr_references_symbols(options, symbols)
                    })
            }
            Expression::LogStep(log_expr) => log_expr
                .values
                .iter()
                .any(|expr| self.expr_references_symbols(expr, symbols)),
            // These don't reference symbols
            Expression::Literal(_)
            | Expression::Vector(_)
            | Expression::Map(_)
            | Expression::List(_)
            | Expression::ResourceRef(_) => false,
        }
    }

    fn eval_let_simple(&self, let_expr: &LetExpr, env: &mut Environment) -> RuntimeResult<Value> {
        let mut let_env = Environment::with_parent(Rc::new(env.clone()));

        for binding in &let_expr.bindings {
            // Evaluate each binding value in the accumulated let environment
            // This allows sequential bindings to reference previous bindings
            let value = self.eval_expr(&binding.value, &mut let_env)?;
            self.bind_pattern(&binding.pattern, &value, &mut let_env)?;
        }

        self.eval_do_body(&let_expr.body, &mut let_env)
    }

    fn eval_let_with_recursion(
        &self,
        let_expr: &LetExpr,
        env: &mut Environment,
    ) -> RuntimeResult<Value> {
        let mut letrec_env = Environment::with_parent(Rc::new(env.clone()));
        let mut placeholders = Vec::new();

        // First pass: create placeholders for all function bindings
        for binding in &let_expr.bindings {
            if let crate::ast::Pattern::Symbol(symbol) = &binding.pattern {
                let placeholder_cell = Rc::new(RefCell::new(Value::Nil));
                letrec_env.define(symbol, Value::FunctionPlaceholder(placeholder_cell.clone()));
                placeholders.push((symbol.clone(), binding.value.clone(), placeholder_cell));
            } else {
                return Err(RuntimeError::NotImplemented(
                    "Complex patterns not yet supported in recursive let".to_string(),
                ));
            }
        }

        // Second pass: evaluate all bindings with placeholders available
        for (symbol, value_expr, placeholder_cell) in placeholders {
            let value = self.eval_expr(&value_expr, &mut letrec_env)?;
            if matches!(value, Value::Function(_)) {
                *placeholder_cell.borrow_mut() = value;
            } else {
                return Err(RuntimeError::TypeError {
                    expected: "function".to_string(),
                    actual: value.type_name().to_string(),
                    operation: format!("binding {} in recursive let", symbol.0),
                });
            }
        }

        self.eval_do_body(&let_expr.body, &mut letrec_env)
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
        let value_to_match = self.eval_expr(&match_expr.expression, env)?;

        for clause in &match_expr.clauses {
            let mut clause_env = Environment::with_parent(Rc::new(env.clone()));
            if self.match_match_pattern(&clause.pattern, &value_to_match, &mut clause_env)? {
                if let Some(guard) = &clause.guard {
                    let guard_result = self.eval_expr(guard, &mut clause_env)?;
                    if !guard_result.is_truthy() {
                        continue;
                    }
                }
                return self.eval_expr(&clause.body, &mut clause_env);
            }
        }

        Err(RuntimeError::MatchError("No matching clause".to_string()))
    }

    fn eval_log_step(&self, log_expr: &LogStepExpr, env: &mut Environment) -> RuntimeResult<Value> {
        let level = log_expr
            .level
            .as_ref()
            .map(|k| k.0.as_str())
            .unwrap_or("info");
        let mut messages = Vec::new();
        for expr in &log_expr.values {
            messages.push(self.eval_expr(expr, env)?.to_string());
        }
        println!("[{}] {}", level, messages.join(" "));
        Ok(Value::Nil)
    }

    fn eval_try_catch(
        &self,
        try_expr: &TryCatchExpr,
        env: &mut Environment,
    ) -> RuntimeResult<Value> {
        match self.eval_do_body(&try_expr.try_body, env) {
            Ok(value) => Ok(value),
            Err(e) => {
                for catch_clause in &try_expr.catch_clauses {
                    let mut catch_env = Environment::with_parent(Rc::new(env.clone()));
                    if self.match_catch_pattern(
                        &catch_clause.pattern,
                        &e.to_value(),
                        &mut catch_env,
                        Some(&catch_clause.binding),
                    )? {
                        return self.eval_do_body(&catch_clause.body, &mut catch_env);
                    }
                }
                Err(e)
            }
        }
    }

    fn eval_fn(&self, fn_expr: &FnExpr, env: &mut Environment) -> RuntimeResult<Value> {
        Ok(Value::Function(Function::new_closure(
            fn_expr
                .params
                .iter()
                .map(|p| self.extract_param_symbol(&p.pattern))
                .collect(),
            Box::new(Expression::Do(DoExpr {
                expressions: fn_expr.body.clone(),
            })),
            Rc::new(env.clone()),
            fn_expr.delegation_hint.clone(),
        )))
    }

    fn eval_with_resource(
        &self,
        with_expr: &WithResourceExpr,
        env: &mut Environment,
    ) -> RuntimeResult<Value> {
        let resource = self.eval_expr(&with_expr.resource_init, env)?;
        let mut resource_env = Environment::with_parent(Rc::new(env.clone()));
        resource_env.define(&with_expr.resource_symbol, resource);
        self.eval_do_body(&with_expr.body, &mut resource_env)
    }
    fn eval_parallel(
        &self,
        parallel_expr: &ParallelExpr,
        env: &mut Environment,
    ) -> RuntimeResult<Value> {
        // TODO: ARCHITECTURAL NOTE - True parallel execution requires Arc<T> migration
        // Current implementation: Sequential execution with parallel semantics
        // This maintains correctness while preparing for future parallelism
        
        let mut results = HashMap::new();
        
        // Process each binding in isolation to simulate parallel execution
        // Each binding gets its own environment clone to avoid interference
        for binding in &parallel_expr.bindings {
            // Clone environment for each binding to simulate parallel isolation
            let mut isolated_env = env.clone();
            
            // Evaluate expression in isolated environment
            let value = self.eval_expr(&binding.expression, &mut isolated_env)?;
            
            // Store result with symbol key
            results.insert(
                MapKey::Keyword(Keyword(binding.symbol.0.clone())), 
                value
            );
        }
        
        // Return results as a map (parallel bindings produce a map of results)
        Ok(Value::Map(results))
    }

    fn eval_def(&self, def_expr: &DefExpr, env: &mut Environment) -> RuntimeResult<Value> {
        let value = self.eval_expr(&def_expr.value, env)?;
        env.define(&def_expr.symbol, value.clone());
        Ok(value)
    }

    fn eval_defn(&self, defn_expr: &DefnExpr, env: &mut Environment) -> RuntimeResult<Value> {
        let function = Value::Function(Function::new_closure(
            defn_expr
                .params
                .iter()
                .map(|p| self.extract_param_symbol(&p.pattern))
                .collect(),
            Box::new(Expression::Do(DoExpr {
                expressions: defn_expr.body.clone(),
            })),
            Rc::new(env.clone()),
            defn_expr.delegation_hint.clone(),
        ));
        env.define(&defn_expr.name, function.clone());
        Ok(function)
    }

    fn match_catch_pattern(
        &self,
        pattern: &CatchPattern,
        value: &Value,
        env: &mut Environment,
        binding: Option<&Symbol>,
    ) -> RuntimeResult<bool> {
        match pattern {
            CatchPattern::Symbol(s) => {
                env.define(s, value.clone());
                Ok(true)
            }
            CatchPattern::Wildcard => {
                if let Some(b) = binding {
                    env.define(b, value.clone());
                }
                Ok(true)
            }
            CatchPattern::Keyword(k) => Ok(Value::Keyword(k.clone()) == *value),
            CatchPattern::Type(_t) => {
                // This is a placeholder implementation. A real implementation would need to
                // check the type of the value against the type expression t.
                Ok(true)
            }
        }
    }

    /// Clean up a resource handle by calling its appropriate cleanup function
    fn cleanup_resource(
        &self,
        handle: &mut crate::runtime::values::ResourceHandle,
    ) -> RuntimeResult<()> {
        // Check if already released
        if handle.state == crate::runtime::values::ResourceState::Released {
            return Ok(());
        }

        // Determine cleanup function based on resource type
        let cleanup_result = match handle.id.as_str() {
            "FileHandle" => {
                // Call tool:close-file or similar cleanup
                // For now, just log the cleanup
                println!("Cleaning up FileHandle: {}", handle.id);
                Ok(Value::Nil)
            }
            "DatabaseConnectionHandle" => {
                println!("Cleaning up DatabaseConnectionHandle: {}", handle.id);
                Ok(Value::Nil)
            }
            _ => {
                println!("Cleaning up generic resource: {}", handle.id);
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
    fn check_resource_state(
        &self,
        handle: &crate::runtime::values::ResourceHandle,
    ) -> RuntimeResult<()> {
        match handle.state {
            crate::runtime::values::ResourceState::Active => Ok(()),
            crate::runtime::values::ResourceState::Released => Err(RuntimeError::ResourceError {
                resource_type: handle.id.clone(),
                message: "Attempted to use released resource handle".to_string(),
            }),
        }
    }

    /// Extract the primary symbol from a parameter pattern for function creation
    /// This is used when creating functions to get the parameter names
    fn extract_param_symbol(&self, pattern: &crate::ast::Pattern) -> Symbol {
        match pattern {
            crate::ast::Pattern::Symbol(s) => s.clone(),
            crate::ast::Pattern::Wildcard => Symbol("_".to_string()),
            crate::ast::Pattern::VectorDestructuring { as_symbol, .. } => as_symbol
                .clone()
                .unwrap_or_else(|| Symbol("vec".to_string())),
            crate::ast::Pattern::MapDestructuring { as_symbol, .. } => as_symbol
                .clone()
                .unwrap_or_else(|| Symbol("map".to_string())),
        }
    }
    /// Evaluate a discover-agents expression
    fn eval_discover_agents(
        &self,
        _discover_expr: &crate::ast::DiscoverAgentsExpr,
        _env: &mut Environment,
    ) -> RuntimeResult<Value> {
        // TODO: Implement agent discovery
        Ok(Value::Vector(vec![]))
    }
    /// Parse a map of criteria into SimpleDiscoveryQuery
    fn parse_criteria_to_query(
        &self,
        criteria_map: &std::collections::HashMap<crate::ast::MapKey, Value>,
    ) -> RuntimeResult<SimpleDiscoveryQuery> {
        use crate::ast::{Keyword, MapKey};

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
                        }
                        "capability-id" | "capability_id" => {
                            query.capability_id = Some(self.parse_string_value(value)?);
                        }
                        "agent-id" | "agent_id" => {
                            query.agent_id = Some(self.parse_string_value(value)?);
                        }
                        "version" | "version-constraint" | "version_constraint" => {
                            query.version_constraint = Some(self.parse_string_value(value)?);
                        }
                        "tags" | "discovery-tags" | "discovery_tags" => {
                            query.discovery_tags = Some(self.parse_capabilities_list(value)?);
                        }
                        "limit" | "max-results" | "max_results" => match value {
                            Value::Integer(i) => {
                                query.limit = Some(*i as u32);
                            }
                            _ => {
                                return Err(RuntimeError::TypeError {
                                    expected: "Integer".to_string(),
                                    actual: format!("{:?}", value),
                                    operation: "parsing limit".to_string(),
                                })
                            }
                        },
                        _ => {
                            // Ignore unknown keys for now
                        }
                    }
                }
                _ => {
                    // Ignore non-keyword keys for now
                }
            }
        }

        Ok(query)
    }

    /// Parse discovery options from a map
    fn parse_options_to_query(
        &self,
        options_map: &std::collections::HashMap<crate::ast::MapKey, Value>,
    ) -> RuntimeResult<SimpleDiscoveryOptions> {
        use crate::ast::{Keyword, MapKey};

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
                        "timeout" | "timeout-ms" | "timeout_ms" => match value {
                            Value::Integer(ms) => {
                                options.timeout_ms = Some(*ms as u64);
                            }
                            _ => {
                                return Err(RuntimeError::TypeError {
                                    expected: "Integer".to_string(),
                                    actual: format!("{:?}", value),
                                    operation: "parsing timeout".to_string(),
                                })
                            }
                        },
                        "cache" | "cache-policy" | "cache_policy" => match value {
                            Value::String(policy) => {
                                use crate::agent::SimpleCachePolicy;
                                options.cache_policy = Some(match policy.as_str() {
                                    "use-cache" | "use_cache" => SimpleCachePolicy::UseCache,
                                    "no-cache" | "no_cache" => SimpleCachePolicy::NoCache,
                                    "refresh-cache" | "refresh_cache" => {
                                        SimpleCachePolicy::RefreshCache
                                    }
                                    _ => SimpleCachePolicy::UseCache,
                                });
                            }
                            _ => {
                                return Err(RuntimeError::TypeError {
                                    expected: "String".to_string(),
                                    actual: format!("{:?}", value),
                                    operation: "parsing cache policy".to_string(),
                                })
                            }
                        },
                        "include-offline" | "include_offline" => match value {
                            Value::Boolean(include) => {
                                options.include_offline = Some(*include);
                            }
                            _ => {
                                return Err(RuntimeError::TypeError {
                                    expected: "Boolean".to_string(),
                                    actual: format!("{:?}", value),
                                    operation: "parsing include-offline".to_string(),
                                })
                            }
                        },
                        "max-results" | "max_results" => match value {
                            Value::Integer(max) => {
                                options.max_results = Some(*max as u32);
                            }
                            _ => {
                                return Err(RuntimeError::TypeError {
                                    expected: "Integer".to_string(),
                                    actual: format!("{:?}", value),
                                    operation: "parsing max-results".to_string(),
                                })
                            }
                        },
                        _ => {
                            // Ignore unknown keys
                        }
                    }
                }
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
            Value::String(agent_card.agent_id),
        );

        // Add name if present
        if let Some(name) = agent_card.name {
            map.insert(
                crate::ast::MapKey::Keyword(crate::ast::Keyword("name".to_string())),
                Value::String(name),
            );
        }

        // Add version if present
        if let Some(version) = agent_card.version {
            map.insert(
                crate::ast::MapKey::Keyword(crate::ast::Keyword("version".to_string())),
                Value::String(version),
            );
        }

        // Add capabilities
        let capabilities: Vec<Value> = agent_card
            .capabilities
            .into_iter()
            .map(|cap| Value::String(cap))
            .collect();
        map.insert(
            crate::ast::MapKey::Keyword(crate::ast::Keyword("capabilities".to_string())),
            Value::Vector(capabilities),
        );

        // Add endpoint if present
        if let Some(endpoint) = agent_card.endpoint {
            map.insert(
                crate::ast::MapKey::Keyword(crate::ast::Keyword("endpoint".to_string())),
                Value::String(endpoint),
            );
        }

        // Add metadata as a JSON string for now
        map.insert(
            crate::ast::MapKey::Keyword(crate::ast::Keyword("metadata".to_string())),
            Value::String(agent_card.metadata.to_string()),
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
                        _ => {
                            return Err(RuntimeError::TypeError {
                                expected: "String".to_string(),
                                actual: format!("{:?}", item),
                                operation: "parsing capability".to_string(),
                            })
                        }
                    }
                }
                Ok(capabilities)
            }
            Value::String(s) => Ok(vec![s.clone()]),
            _ => Err(RuntimeError::TypeError {
                expected: "Vector or String".to_string(),
                actual: format!("{:?}", value),
                operation: "parsing capabilities".to_string(),
            }),
        }
    }
    /// Helper function to parse a string value
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
    fn match_match_pattern(
        &self,
        pattern: &crate::ast::MatchPattern,
        value: &Value,
        env: &mut Environment,
    ) -> RuntimeResult<bool> {
        match pattern {
            crate::ast::MatchPattern::Symbol(symbol) => {
                env.define(symbol, value.clone());
                Ok(true)
            }
            crate::ast::MatchPattern::Wildcard => {
                Ok(true) // Wildcard always matches
            }
            crate::ast::MatchPattern::Literal(lit_pattern) => {
                let lit_value = self.eval_literal(lit_pattern)?;
                Ok(lit_value == *value)
            }
            crate::ast::MatchPattern::Keyword(keyword_pattern) => {
                Ok(*value == Value::Keyword(keyword_pattern.clone()))
            }
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
            }
            crate::ast::MatchPattern::Map { entries, rest } => {
                if let Value::Map(value_map) = value {
                    let mut temp_env = env.clone();
                    let mut matched_keys = std::collections::HashSet::new();

                    // Match fixed elements
                    for entry in entries {
                        let map_key = entry.key.clone();

                        if let Some(v) = value_map.get(&map_key) {
                            if self.match_match_pattern(&entry.pattern, v, &mut temp_env)? {
                                matched_keys.insert(map_key);
                            } else {
                                return Ok(false); // Value pattern didn't match
                            }
                        } else {
                            return Ok(false); // Key not found in value map
                        }
                    }

                    // If we are here, all pattern elements matched.
                    // Now handle the rest, if any.
                    if let Some(rest_symbol) = rest {
                        let mut rest_map = value_map.clone();
                        for key in &matched_keys {
                            rest_map.remove(key);
                        }
                        temp_env.define(rest_symbol, Value::Map(rest_map));
                    } else {
                        // If no rest pattern, require an exact match (no extra keys in value)
                        if value_map.len() != entries.len() {
                            return Ok(false);
                        }
                    }

                    *env = temp_env;
                    Ok(true)
                } else {
                    Ok(false) // Pattern is a map, but value is not
                }
            }
            _ => Err(RuntimeError::NotImplemented(format!(
                "Complex match pattern matching not yet implemented for: {:?}",
                pattern
            ))),
        }
    }

    /// Bind a pattern to a value in an environment
    fn bind_pattern(
        &self,
        pattern: &crate::ast::Pattern,
        value: &Value,
        env: &mut Environment,
    ) -> RuntimeResult<()> {
        match pattern {
            crate::ast::Pattern::Symbol(symbol) => {
                env.define(symbol, value.clone());
                Ok(())
            }
            crate::ast::Pattern::Wildcard => {
                // Wildcard does nothing, successfully "matches" any value.
                Ok(())
            }
            crate::ast::Pattern::VectorDestructuring {
                elements,
                rest,
                as_symbol,
            } => {
                // First, bind the whole value to as_symbol if provided
                if let Some(as_sym) = as_symbol {
                    env.define(as_sym, value.clone());
                }

                // Pattern must match against a vector value
                if let Value::Vector(vector_values) = value {
                    // Check if we have enough elements to bind (considering rest parameter)
                    let required_elements = elements.len();
                    if rest.is_none() && vector_values.len() != required_elements {
                        return Err(RuntimeError::TypeError {
                            expected: format!("vector with exactly {} elements", required_elements),
                            actual: format!("vector with {} elements", vector_values.len()),
                            operation: "vector destructuring".to_string(),
                        });
                    }

                    if vector_values.len() < required_elements {
                        return Err(RuntimeError::TypeError {
                            expected: format!(
                                "vector with at least {} elements",
                                required_elements
                            ),
                            actual: format!("vector with {} elements", vector_values.len()),
                            operation: "vector destructuring".to_string(),
                        });
                    }

                    // Bind each pattern element to the corresponding vector element
                    for (i, element_pattern) in elements.iter().enumerate() {
                        self.bind_pattern(element_pattern, &vector_values[i], env)?;
                    }

                    // Handle rest parameter if present
                    if let Some(rest_symbol) = rest {
                        let rest_values: Vec<Value> = vector_values[required_elements..].to_vec();
                        env.define(rest_symbol, Value::Vector(rest_values));
                    }

                    Ok(())
                } else {
                    Err(RuntimeError::TypeError {
                        expected: "vector".to_string(),
                        actual: format!("{:?}", value),
                        operation: "vector destructuring".to_string(),
                    })
                }
            }
            crate::ast::Pattern::MapDestructuring {
                entries,
                rest,
                as_symbol,
            } => {
                // First, bind the whole value to as_symbol if provided
                if let Some(as_sym) = as_symbol {
                    env.define(as_sym, value.clone());
                }

                // Pattern must match against a map value
                if let Value::Map(map_values) = value {
                    let mut bound_keys = std::collections::HashSet::new();

                    // Process each destructuring entry
                    for entry in entries {
                                               match entry {
                            crate::ast::MapDestructuringEntry::KeyBinding { key, pattern } => {
                                bound_keys.insert(key.clone());
                                // Look up the key in the map
                                if let Some(map_value) = map_values.get(key) {
                                    // Recursively bind the pattern to the value
                                    self.bind_pattern(pattern, map_value, env)?;
                                } else {
                                    // Key not found in map - bind to Nil for optional patterns
                                    // or return error for required patterns
                                    self.bind_pattern(pattern, &Value::Nil, env)?;
                                }
                            }
                            crate::ast::MapDestructuringEntry::Keys(symbols) => {
                                // Handle :keys [key1 key2] syntax
                                for symbol in symbols {
                                    // Convert symbol to keyword for map lookup
                                    let key = crate::ast::MapKey::Keyword(crate::ast::Keyword(
                                        symbol.0.clone(),
                                    ));
                                    bound_keys.insert(key.clone());
                                    if let Some(map_value) = map_values.get(&key) {
                                        env.define(symbol, map_value.clone());
                                    } else {
                                        // Key not found - bind to Nil
                                        env.define(symbol, Value::Nil);
                                    }
                                }
                            }
                        }
                    }

                    // Handle rest parameter if present
                    if let Some(rest_symbol) = rest {
                        let mut rest_map = map_values.clone();
                        for key in bound_keys {
                            rest_map.remove(&key);
                        }
                        env.define(rest_symbol, Value::Map(rest_map));
                    }

                    Ok(())
                } else {
                    Err(RuntimeError::TypeError {
                        expected: "map".to_string(),
                        actual: format!("{:?}", value),
                        operation: "map destructuring".to_string(),
                    })
                }
            }
        }
    }



    fn execute_model_call(
        &self,
        model_id: &str,
        args: &[Value],
        _env: &mut Environment,
    ) -> RuntimeResult<Value> {
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

    fn args_to_prompt(&self, args: &[Value]) -> RuntimeResult<String> {
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
        &self,
        function: &Value,
        collection: &Value,
        env: &mut Environment,
    ) -> RuntimeResult<Value> {
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
                    // Call user-defined functions using the evaluator
                    let func_args = vec![item];
                    let mapped_value = self.call_function(
                        Value::Function(Function::Closure(closure.clone())),
                        &func_args,
                        env,
                    )?;
                    result.push(mapped_value);
                }
                Value::Function(Function::Native(native_func)) => {
                    // Call native functions
                    let func_args = vec![item];
                    let mapped_value = (native_func.func)(func_args)?;
                    result.push(mapped_value);
                }
                Value::Function(Function::Ir(_ir_func)) => {
                    // TODO: Implement IR function calling
                    return Err(RuntimeError::NotImplemented(
                        "map: IR functions not yet supported".to_string(),
                    ));
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

    /// Check if a capability is allowed in the current security context
    pub fn check_capability_permission(&self, capability_id: &str) -> Result<(), RuntimeError> {
        if !self.security_context.is_capability_allowed(capability_id) {
            return Err(RuntimeError::Generic(format!(
                "Capability '{}' not allowed in current security context", 
                capability_id
            )));
        }
        Ok(())
    }

    /// Check if a capability requires microVM execution
    pub fn requires_microvm(&self, capability_id: &str) -> bool {
        self.security_context.requires_microvm(capability_id)
    }

    /// Get the current security context
    pub fn security_context(&self) -> &RuntimeContext {
        &self.security_context
    }

    /// Gets a value from the current execution context
    pub fn get_context_value(&self, key: &str) -> Option<Value> {
        self.context_manager.borrow().get(key)
    }

    /// Sets a value in the current execution context
    pub fn set_context_value(&self, key: String, value: Value) -> RuntimeResult<()> {
        self.context_manager.borrow_mut().set(key, value)
    }

    /// Gets the current context depth
    pub fn context_depth(&self) -> usize {
        self.context_manager.borrow().depth()
    }

    /// Gets the current context ID
    pub fn current_context_id(&self) -> Option<String> {
        self.context_manager.borrow().current_context_id().map(|s| s.to_string())
    }

    /// Create a new evaluator with updated security context
    pub fn with_security_context(&self, security_context: RuntimeContext) -> Self {
        Self {
            module_registry: self.module_registry.clone(),
            env: self.env.clone(),
            recursion_depth: 0,
            max_recursion_depth: self.max_recursion_depth,

            delegation_engine: self.delegation_engine.clone(),
            model_registry: self.model_registry.clone(),
            security_context,
            host: self.host.clone(),
            special_forms: Self::default_special_forms(),
            type_validator: self.type_validator.clone(),
            type_config: self.type_config.clone(),
            context_manager: RefCell::new(ContextManager::new()),
        }
    }

    pub fn with_environment(
        module_registry: Rc<ModuleRegistry>,
        env: Environment,
        delegation_engine: Arc<dyn DelegationEngine>,
        security_context: RuntimeContext,
        host: Rc<dyn HostInterface>,
    ) -> Self {
        Self {
            module_registry,
            env,
            recursion_depth: 0,
            max_recursion_depth: 1000,

            delegation_engine,
            model_registry: Arc::new(ModelRegistry::with_defaults()),
            security_context,
            host,
            special_forms: Self::default_special_forms(),
            type_validator: Arc::new(TypeValidator::new()),
            type_config: TypeCheckingConfig::default(),
            context_manager: RefCell::new(ContextManager::new()),
        }
    }
}

impl Default for Evaluator {
    fn default() -> Self {
        let module_registry = Rc::new(ModuleRegistry::new());
        let static_map = std::collections::HashMap::new();
        let delegation_engine = Arc::new(StaticDelegationEngine::new(static_map));
        let security_context = RuntimeContext::pure();
        
        // Create a minimal host interface for default case
        // This should be replaced with a proper host in production
        use crate::runtime::host::RuntimeHost;
        use crate::runtime::capability_marketplace::CapabilityMarketplace;
        use crate::runtime::capability_registry::CapabilityRegistry;
        use crate::ccos::causal_chain::CausalChain;
        use std::sync::Arc;
        use tokio::sync::RwLock;
        let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
        let capability_marketplace = Arc::new(CapabilityMarketplace::new(registry.clone()));
        let causal_chain = Arc::new(Mutex::new(CausalChain::new().expect("Failed to create causal chain")));
        let runtime_host = RuntimeHost::new(
            causal_chain,
            capability_marketplace,
            security_context.clone(),
        );
        let host = Rc::new(runtime_host);

        Self::new(
            module_registry,
            delegation_engine,
            security_context,
            host,
        )
    }
}
