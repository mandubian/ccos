// RTFS Evaluator - Executes parsed AST nodes

use crate::ast::{
    CatchPattern, DefExpr, DefnExpr, DefstructExpr, DoExpr, Expression, FnExpr, ForExpr, IfExpr,
    Keyword, LetExpr, Literal, LogStepExpr, MapKey, MatchExpr, ParallelExpr, Symbol, TopLevel,
    TryCatchExpr, WithResourceExpr,
};
use crate::runtime::environment::Environment;
use crate::runtime::error::{RuntimeError, RuntimeResult};
use crate::runtime::execution_outcome::{CallMetadata, ExecutionOutcome, HostCall};
use crate::runtime::host_interface::HostInterface;
use crate::runtime::module_runtime::ModuleRegistry;
use crate::runtime::security::IsolationLevel;
use crate::runtime::security::RuntimeContext;
use crate::runtime::stubs::{
    ConflictResolution, ExecutionResultStruct, SimpleAgentCard,
    SimpleDiscoveryOptions, SimpleDiscoveryQuery,
};
use crate::runtime::type_validator::{
    TypeCheckingConfig, TypeValidator, ValidationLevel, VerificationContext,
};
use crate::runtime::values::{Arity, BuiltinFunctionWithContext, Function, Value};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
type SpecialFormHandler =
    fn(&Evaluator, &[Expression], &mut Environment) -> Result<ExecutionOutcome, RuntimeError>;

#[derive(Clone, Debug)]
pub struct Evaluator {
    module_registry: Arc<ModuleRegistry>,
    pub env: Environment,
    recursion_depth: usize,
    max_recursion_depth: usize,
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
        // Removed: set! special form - no mutable variables allowed
        // Allow (get :key) as a special form that reads from the execution context
        // with cross-plan fallback. For other usages (get collection key [default])
        // fall back to the normal builtin function by delegating to the env lookup.
        special_forms.insert("get".to_string(), Self::eval_get_form);
        // Core iteration forms
        special_forms.insert("dotimes".to_string(), Self::eval_dotimes_form);
        special_forms.insert("for".to_string(), Self::eval_for_form);
        // Add other evaluator-level special forms here in the future

        // LLM execution bridge (M1)
        special_forms.insert("llm-execute".to_string(), Self::eval_llm_execute_form);

        // Resource management special form
        special_forms.insert(
            "with-resource".to_string(),
            Self::eval_with_resource_special_form,
        );

        // Match special form
        special_forms.insert("match".to_string(), Self::eval_match_form);

        special_forms
    }

    /// Create a new evaluator with secure environment and default security context
    pub fn new(
        module_registry: Arc<ModuleRegistry>,
        security_context: RuntimeContext,
        host: Arc<dyn HostInterface>,
    ) -> Self {
        let env = crate::runtime::stdlib::StandardLibrary::create_global_environment();
        Evaluator {
            module_registry,
            env,
            recursion_depth: 0,
            max_recursion_depth: 50,
            security_context,
            host,
            special_forms: Self::default_special_forms(),
            type_validator: Arc::new(TypeValidator::new()),
            type_config: TypeCheckingConfig::default(),
        }
    }

    /// Create a new evaluator with task context and security

    /// Create a new evaluator with default security context (pure)
    pub fn new_with_defaults(
        module_registry: Arc<ModuleRegistry>,
        host: Arc<dyn HostInterface>,
    ) -> Self {
        Self::new(module_registry, RuntimeContext::pure(), host)
    }

    /// Configure type checking behavior
    pub fn set_type_checking_config(&mut self, config: TypeCheckingConfig) {
        self.type_config = config;
    }

    /// Get current type checking configuration
    pub fn get_type_checking_config(&self) -> &TypeCheckingConfig {
        &self.type_config
    }

    /// Get the module registry
    pub fn module_registry(&self) -> &Arc<ModuleRegistry> {
        &self.module_registry
    }

    /// Create evaluator with optimized type checking for production
    pub fn new_optimized(
        module_registry: Arc<ModuleRegistry>,
        security_context: RuntimeContext,
        host: Arc<dyn HostInterface>,
    ) -> Self {
        let mut evaluator = Self::new(module_registry, security_context, host);
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
        module_registry: Arc<ModuleRegistry>,
        security_context: RuntimeContext,
        host: Arc<dyn HostInterface>,
    ) -> Self {
        let mut evaluator = Self::new(module_registry, security_context, host);
        evaluator.type_config = TypeCheckingConfig {
            skip_compile_time_verified: false,
            enforce_capability_boundaries: true,
            validate_external_data: true,
            validation_level: ValidationLevel::Strict,
        };
        evaluator
    }

    /// Get a value with cross-plan context fallback
    /// First tries the current step context, then falls back to cross-plan parameters
    pub fn get_with_cross_plan_fallback(&self, key: &str) -> Option<Value> {
        // First try host context (includes step-scoped context if host supports it)
        if let Some(value) = self.host.get_context_value(key) {
            return Some(value);
        }

        // Fall back to cross-plan parameters from RuntimeContext
        self.security_context.get_cross_plan_param(key).cloned()
    }

    /// Create verification context for local expression evaluation
    fn create_local_verification_context(
        &self,
        compile_time_verified: bool,
    ) -> VerificationContext {
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
        self.type_validator
            .validate_with_config(value, expected_type, &self.type_config, &context)
            .map_err(|e| RuntimeError::TypeValidationError(e.to_string()))
    }

    /// Validate arguments for known builtin functions
    fn validate_builtin_function_args(
        &self,
        function_name: &str,
        args: &[Value],
    ) -> RuntimeResult<()> {
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
                        return self
                            .type_validator
                            .validate_with_config(arg, &expected_type, &self.type_config, &context)
                            .map_err(|e| {
                                RuntimeError::TypeValidationError(format!(
                                    "Function '{}' argument {}: {}",
                                    function_name, i, e
                                ))
                            });
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
    fn validate_builtin_function_result(
        &self,
        function_name: &str,
        result: &Value,
        _args: &[Value],
    ) -> RuntimeResult<()> {
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
                    return self
                        .type_validator
                        .validate_with_config(result, &expected_type, &self.type_config, &context)
                        .map_err(|e| {
                            RuntimeError::TypeValidationError(format!(
                                "Function '{}' should return number: {}",
                                function_name, e
                            ))
                        });
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
            return matches!(
                function_name,
                "eval" | "load" | "exec" | "read-file" | "write-file"
            );
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

    /// Check if a function is non-pure (requires external execution)
    /// This is a simplified heuristic - in a real implementation, this would be
    /// based on function annotations, capability requirements, or other metadata
    fn is_non_pure_function(&self, fn_name: &str) -> bool {
        // Check for known non-pure functions that require external execution
        matches!(
            fn_name,
            "call"
                | "llm-execute"
                | "model-call"
                | "capability-call"
                | "http-request"
                | "database-query"
                | "file-read"
                | "file-write"
                | "network-request"
                | "external-api"
                | "system-command"
        )
    }

    pub fn eval_toplevel(
        &mut self,
        program: &[TopLevel],
    ) -> Result<ExecutionOutcome, RuntimeError> {
        let mut env = self.env.clone();
        let mut last_value = Value::Nil;
        for toplevel in program {
            match toplevel {
                TopLevel::Expression(expr) => match self.eval_expr(expr, &mut env)? {
                    ExecutionOutcome::Complete(v) => last_value = v,
                    ExecutionOutcome::RequiresHost(hc) => {
                        return Ok(ExecutionOutcome::RequiresHost(hc))
                    }
                    #[cfg(feature = "effect-boundary")]
                    ExecutionOutcome::RequiresHost(host_call) => {
                        return Ok(ExecutionOutcome::RequiresHost(host_call))
                    }
                    #[cfg(feature = "effect-boundary")]
                    ExecutionOutcome::RequiresHost(host_call) => {
                        return Ok(ExecutionOutcome::RequiresHost(host_call))
                    }
                },
                TopLevel::Intent(intent) => {
                    // Evaluate intent properties and return intent metadata
                    let mut intent_metadata = HashMap::new();
                    for property in &intent.properties {
                        let key = crate::ast::MapKey::String(property.key.0.clone());
                        match self.eval_expr(&property.value, &mut env)? {
                            ExecutionOutcome::Complete(v) => {
                                intent_metadata.insert(key, v);
                            }
                            ExecutionOutcome::RequiresHost(hc) => {
                                return Ok(ExecutionOutcome::RequiresHost(hc))
                            }
                            #[cfg(feature = "effect-boundary")]
                            ExecutionOutcome::RequiresHost(host_call) => {
                                return Ok(ExecutionOutcome::RequiresHost(host_call))
                            }
                        }
                    }
                    last_value = Value::Map(intent_metadata);
                }
                TopLevel::Plan(plan) => {
                    // Evaluate plan properties and return plan metadata
                    let mut plan_metadata = HashMap::new();
                    for property in &plan.properties {
                        let key = crate::ast::MapKey::String(property.key.0.clone());
                        match self.eval_expr(&property.value, &mut env)? {
                            ExecutionOutcome::Complete(v) => {
                                plan_metadata.insert(key, v);
                            }
                            ExecutionOutcome::RequiresHost(hc) => {
                                return Ok(ExecutionOutcome::RequiresHost(hc))
                            }
                            #[cfg(feature = "effect-boundary")]
                            ExecutionOutcome::RequiresHost(host_call) => {
                                return Ok(ExecutionOutcome::RequiresHost(host_call))
                            }
                        }
                    }
                    last_value = Value::Map(plan_metadata);
                }
                TopLevel::Action(action) => {
                    // Evaluate action properties and return action metadata
                    let mut action_metadata = HashMap::new();
                    for property in &action.properties {
                        let key = crate::ast::MapKey::String(property.key.0.clone());
                        match self.eval_expr(&property.value, &mut env)? {
                            ExecutionOutcome::Complete(v) => {
                                action_metadata.insert(key, v);
                            }
                            ExecutionOutcome::RequiresHost(hc) => {
                                return Ok(ExecutionOutcome::RequiresHost(hc))
                            }
                            #[cfg(feature = "effect-boundary")]
                            ExecutionOutcome::RequiresHost(host_call) => {
                                return Ok(ExecutionOutcome::RequiresHost(host_call))
                            }
                        }
                    }
                    last_value = Value::Map(action_metadata);
                }
                TopLevel::Capability(capability) => {
                    // Evaluate capability properties and return capability metadata
                    let mut capability_metadata = HashMap::new();
                    for property in &capability.properties {
                        let key = crate::ast::MapKey::String(property.key.0.clone());
                        match self.eval_expr(&property.value, &mut env)? {
                            ExecutionOutcome::Complete(v) => {
                                capability_metadata.insert(key, v);
                            }
                            ExecutionOutcome::RequiresHost(hc) => {
                                return Ok(ExecutionOutcome::RequiresHost(hc))
                            }
                            #[cfg(feature = "effect-boundary")]
                            ExecutionOutcome::RequiresHost(host_call) => {
                                return Ok(ExecutionOutcome::RequiresHost(host_call))
                            }
                        }
                    }
                    last_value = Value::Map(capability_metadata);
                }
                TopLevel::Resource(resource) => {
                    // Evaluate resource properties and return resource metadata
                    let mut resource_metadata = HashMap::new();
                    for property in &resource.properties {
                        let key = crate::ast::MapKey::String(property.key.0.clone());
                        match self.eval_expr(&property.value, &mut env)? {
                            ExecutionOutcome::Complete(v) => {
                                resource_metadata.insert(key, v);
                            }
                            ExecutionOutcome::RequiresHost(hc) => {
                                return Ok(ExecutionOutcome::RequiresHost(hc))
                            }
                            #[cfg(feature = "effect-boundary")]
                            ExecutionOutcome::RequiresHost(host_call) => {
                                return Ok(ExecutionOutcome::RequiresHost(host_call))
                            }
                        }
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
        Ok(ExecutionOutcome::Complete(last_value))
    }

    /// Evaluate an expression in a given environment
    pub fn eval_expr(
        &self,
        expr: &Expression,
        env: &mut Environment,
    ) -> Result<ExecutionOutcome, RuntimeError> {
        // Check recursion depth to prevent stack overflow
        if self.recursion_depth >= self.max_recursion_depth {
            return Err(RuntimeError::Generic(format!(
                "Maximum recursion depth ({}) exceeded. This usually indicates infinite recursion or deeply nested expressions.",
                self.max_recursion_depth
            )));
        }

        // Create a new evaluator with incremented recursion depth for recursive calls
        let mut deeper_evaluator = self.clone();
        deeper_evaluator.recursion_depth += 1;

        match expr {
            Expression::Literal(lit) => Ok(ExecutionOutcome::Complete(self.eval_literal(lit)?)),
            Expression::Symbol(sym) => {
                // First try environment lookup (local bindings, set!, let, etc.)
                if let Some(v) = env.lookup(sym) {
                    return Ok(ExecutionOutcome::Complete(v));
                }
                // Fallback to cross-plan parameters if not found in environment
                if let Some(v) = self.get_with_cross_plan_fallback(&sym.0) {
                    return Ok(ExecutionOutcome::Complete(v));
                }
                // If still not found, return undefined symbol error
                Err(RuntimeError::UndefinedSymbol(sym.clone()))
            }
            Expression::List(list) => {
                if list.is_empty() {
                    return Ok(ExecutionOutcome::Complete(Value::Vector(vec![])));
                }

                if let Expression::Symbol(s) = &list[0] {
                    if let Some(handler) = self.special_forms.get(&s.0) {
                        return handler(self, &list[1..], env);
                    }
                }

                // It's a regular function call
                let func_expr = &list[0];
                match deeper_evaluator.eval_expr(func_expr, env)? {
                    ExecutionOutcome::Complete(func_value) => {
                        // Evaluate args, aborting early if any arg requires host
                        let mut args_vec: Vec<Value> = Vec::new();
                        for e in &list[1..] {
                            match deeper_evaluator.eval_expr(e, env)? {
                                ExecutionOutcome::Complete(av) => args_vec.push(av),
                                ExecutionOutcome::RequiresHost(hc) => {
                                    return Ok(ExecutionOutcome::RequiresHost(hc))
                                }
                                #[cfg(feature = "effect-boundary")]
                                ExecutionOutcome::RequiresHost(host_call) => {
                                    return Ok(ExecutionOutcome::RequiresHost(host_call))
                                }
                            }
                        }
                        return self.call_function(func_value, &args_vec, env);
                    }
                    ExecutionOutcome::RequiresHost(hc) => {
                        return Ok(ExecutionOutcome::RequiresHost(hc))
                    }
                    #[cfg(feature = "effect-boundary")]
                    ExecutionOutcome::RequiresHost(host_call) => {
                        return Ok(ExecutionOutcome::RequiresHost(host_call))
                    }
                }
            }
            Expression::Vector(exprs) => {
                let mut values_vec: Vec<Value> = Vec::new();
                for e in exprs {
                    match self.eval_expr(e, env)? {
                        ExecutionOutcome::Complete(v) => values_vec.push(v),
                        ExecutionOutcome::RequiresHost(hc) => {
                            return Ok(ExecutionOutcome::RequiresHost(hc))
                        }
                        #[cfg(feature = "effect-boundary")]
                        ExecutionOutcome::RequiresHost(host_call) => {
                            return Ok(ExecutionOutcome::RequiresHost(host_call))
                        }
                    }
                }
                Ok(ExecutionOutcome::Complete(Value::Vector(values_vec)))
            }
            Expression::Map(map) => {
                let mut result = HashMap::new();
                for (key, value_expr) in map {
                    match self.eval_expr(value_expr, env)? {
                        ExecutionOutcome::Complete(v) => {
                            result.insert(key.clone(), v);
                        }
                        ExecutionOutcome::RequiresHost(hc) => {
                            return Ok(ExecutionOutcome::RequiresHost(hc))
                        }
                        #[cfg(feature = "effect-boundary")]
                        ExecutionOutcome::RequiresHost(host_call) => {
                            return Ok(ExecutionOutcome::RequiresHost(host_call))
                        }
                    }
                }
                Ok(ExecutionOutcome::Complete(Value::Map(result)))
            }
            Expression::FunctionCall { callee, arguments } => {
                // Check if this is a special form before evaluating the callee
                if let Expression::Symbol(s) = &**callee {
                    // Special case: if this is "step" and the first argument is a keyword,
                    // treat it as a regular function call instead of a special form
                    if s.0 == "step" && !arguments.is_empty() {
                        if let Expression::Literal(crate::ast::Literal::Keyword(_)) = &arguments[0]
                        {
                            // This is a step function call with keyword arguments, not a special form
                            match self.eval_expr(callee, env)? {
                                ExecutionOutcome::Complete(func_value) => {
                                    let mut args_vec: Vec<Value> = Vec::new();
                                    for e in arguments {
                                        match self.eval_expr(e, env)? {
                                            ExecutionOutcome::Complete(av) => args_vec.push(av),
                                            ExecutionOutcome::RequiresHost(hc) => {
                                                return Ok(ExecutionOutcome::RequiresHost(hc))
                                            }
                                            #[cfg(feature = "effect-boundary")]
                                            ExecutionOutcome::RequiresHost(host_call) => {
                                                return Ok(ExecutionOutcome::RequiresHost(
                                                    host_call,
                                                ))
                                            }
                                        }
                                    }
                                    return self.call_function(func_value, &args_vec, env);
                                }
                                ExecutionOutcome::RequiresHost(hc) => {
                                    return Ok(ExecutionOutcome::RequiresHost(hc))
                                }
                                #[cfg(feature = "effect-boundary")]
                                ExecutionOutcome::RequiresHost(host_call) => {
                                    return Ok(ExecutionOutcome::RequiresHost(host_call))
                                }
                            }
                        }
                    }

                    if let Some(handler) = self.special_forms.get(&s.0) {
                        return handler(self, arguments, env);
                    }
                }

                match self.eval_expr(callee, env)? {
                    ExecutionOutcome::Complete(func_value) => {
                        if let Value::Function(Function::Builtin(f)) = &func_value {
                            if f.name == "quote" {
                                if arguments.len() != 1 {
                                    return Err(RuntimeError::ArityMismatch {
                                        function: "quote".to_string(),
                                        expected: "1".to_string(),
                                        actual: arguments.len(),
                                    });
                                }
                                return Ok(ExecutionOutcome::Complete(Value::from(
                                    arguments[0].clone(),
                                )));
                            }
                        }
                        // Evaluate arguments and call normally
                        let mut args_vec: Vec<Value> = Vec::new();
                        for e in arguments {
                            match self.eval_expr(e, env)? {
                                ExecutionOutcome::Complete(av) => args_vec.push(av),
                                ExecutionOutcome::RequiresHost(hc) => {
                                    return Ok(ExecutionOutcome::RequiresHost(hc))
                                }
                                #[cfg(feature = "effect-boundary")]
                                ExecutionOutcome::RequiresHost(host_call) => {
                                    return Ok(ExecutionOutcome::RequiresHost(host_call))
                                }
                            }
                        }
                        return self.call_function(func_value, &args_vec, env);
                    }
                    ExecutionOutcome::RequiresHost(hc) => {
                        return Ok(ExecutionOutcome::RequiresHost(hc))
                    }
                    #[cfg(feature = "effect-boundary")]
                    ExecutionOutcome::RequiresHost(host_call) => {
                        return Ok(ExecutionOutcome::RequiresHost(host_call))
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
            Expression::Defstruct(defstruct_expr) => self.eval_defstruct(defstruct_expr, env),
            Expression::For(for_expr) => self.eval_for(for_expr, env),
            Expression::Deref(expr) => {
                // @atom-name desugars to (deref atom-name)
                let deref_call = Expression::FunctionCall {
                    callee: Box::new(Expression::Symbol(Symbol("deref".to_string()))),
                    arguments: vec![*expr.clone()],
                };
                self.eval_expr(&deref_call, env)
            }
            Expression::DiscoverAgents(discover_expr) => {
                self.eval_discover_agents(discover_expr, env)
            }
            Expression::ResourceRef(s) => {
                // Resolve resource references from the host's execution context
                if let Some(val) = self.host.get_context_value(s) {
                    return Ok(ExecutionOutcome::Complete(val));
                }
                // Fallback: echo as symbolic reference string (keeps prior behavior for resource:ref)
                Ok(ExecutionOutcome::Complete(Value::String(format!("@{}", s))))
            }
            Expression::Metadata(metadata_map) => {
                // Metadata is typically attached to definitions, not evaluated as standalone expressions
                // For now, we'll evaluate it to a map value. Each evaluated entry returns an
                // ExecutionOutcome which must be unwrapped to extract the underlying Value or
                // propagated if it requires a host call.
                let mut result_map = std::collections::HashMap::new();
                for (key, value_expr) in metadata_map {
                    match self.eval_expr(value_expr, env)? {
                        ExecutionOutcome::Complete(v) => {
                            result_map.insert(key.clone(), v);
                        }
                        ExecutionOutcome::RequiresHost(hc) => {
                            return Ok(ExecutionOutcome::RequiresHost(hc))
                        }
                        #[cfg(feature = "effect-boundary")]
                        ExecutionOutcome::RequiresHost(host_call) => {
                            return Ok(ExecutionOutcome::RequiresHost(host_call))
                        }
                    }
                }
                Ok(ExecutionOutcome::Complete(Value::Map(result_map)))
            }
        }
    }

    fn eval_step_form(
        &self,
        args: &[Expression],
        env: &mut Environment,
    ) -> Result<ExecutionOutcome, RuntimeError> {
        // 1. Validate arguments: "name" [options as keyword/value pairs] ...body
        if args.is_empty() {
            return Err(RuntimeError::InvalidArguments {
                expected: "at least 1 (a string name)".to_string(),
                actual: "0".to_string(),
            });
        }

        let step_name = match &args[0] {
            Expression::Literal(crate::ast::Literal::String(s)) => s.clone(),
            _ => {
                return Err(RuntimeError::InvalidArguments {
                    expected: "a string for the step name".to_string(),
                    actual: serde_json::to_string(&args[0])
                        .unwrap_or_else(|_| format!("{:?}", args[0])),
                })
            }
        };

        // 2. Parse optional options: :expose-context (bool), :context-keys (vector of strings)
        use crate::ast::Literal as Lit;
        // Type alias for readability: map of parameter name -> expression
        type ParamsExprMap = std::collections::HashMap<String, crate::ast::Expression>;
        let mut i = 1;
        let mut expose_override: Option<bool> = None;
        let mut context_keys_override: Option<Vec<String>> = None;
        // Optional params map (keyword :params followed by a map expression)
        let mut params_expr_map: Option<std::collections::HashMap<String, crate::ast::Expression>> =
            None;
        while i + 1 < args.len() {
            match &args[i] {
                Expression::Literal(Lit::Keyword(k)) => {
                    let key = k.0.as_str();
                    let val_expr = &args[i + 1];
                    match (key, val_expr) {
                        ("expose-context", Expression::Literal(Lit::Boolean(b))) => {
                            expose_override = Some(*b);
                            i += 2;
                            continue;
                        }
                        ("context-keys", Expression::Vector(v)) => {
                            let mut keys: Vec<String> = Vec::new();
                            for e in v {
                                if let Expression::Literal(Lit::String(s)) = e {
                                    keys.push(s.clone());
                                } else {
                                    return Err(RuntimeError::InvalidArguments {
                                        expected: "vector of strings for :context-keys".to_string(),
                                        actual: serde_json::to_string(&e)
                                            .unwrap_or_else(|_| format!("{:?}", e)),
                                    });
                                }
                            }
                            context_keys_override = Some(keys);
                            i += 2;
                            continue;
                        }
                        ("params", Expression::Map(m)) => {
                            // collect expressions from the map into a ParamsExprMap
                            let mut pm: ParamsExprMap = ParamsExprMap::new();
                            for (mk, mv) in m.iter() {
                                if let crate::ast::MapKey::String(s) = mk {
                                    pm.insert(s.clone(), mv.clone());
                                } else {
                                    return Err(RuntimeError::InvalidArguments {
                                        expected: "string keys in :params map".to_string(),
                                        actual: serde_json::to_string(&mk)
                                            .unwrap_or_else(|_| format!("{:?}", mk)),
                                    });
                                }
                            }
                            params_expr_map = Some(pm);
                            i += 2;
                            continue;
                        }
                        _ => {
                            break;
                        }
                    }
                }
                _ => break,
            }
        }

        // 3. Enter step context
        // Enforce isolation policy via RuntimeContext before entering
        if !self
            .security_context
            .is_isolation_allowed(&IsolationLevel::Inherit)
        {
            return Err(RuntimeError::SecurityViolation {
                operation: "step".to_string(),
                capability: "isolation:inherit".to_string(),
                context: format!(
                    "Isolation level not permitted: Inherit under {:?}",
                    self.security_context.security_level
                ),
            });
        }
        // ContextManager removed - step lifecycle handled by host via notify_step_started/completed

        // 4. Apply step exposure override if provided
        if let Some(expose) = expose_override {
            self.host
                .set_step_exposure_override(expose, context_keys_override.clone());
        }

        // Prepare a separate child environment when params are supplied so
        // step-local bindings (like %params) don't overwrite the parent's bindings
        // permanently. We'll evaluate the body in `body_env` which either
        // references the provided `env` or a newly created child environment.
        let mut _child_env_opt: Option<Environment> = None;
        let body_env: &mut Environment;
        if let Some(param_map) = params_expr_map {
            // adapt param_map to expected type for binder: HashMap<String, Expression>
            use crate::runtime::param_binding::bind_parameters;
            // build evaluator closure that evaluates against the parent env while
            // binding params (param expressions can refer to parent bindings)
            let mut eval_cb = |expr: &crate::ast::Expression| -> RuntimeResult<Value> {
                // eval_expr now returns ExecutionOutcome; unwrap Complete(v) to Value
                match self.eval_expr(expr, env)? {
                    ExecutionOutcome::Complete(v) => Ok(v),
                    ExecutionOutcome::RequiresHost(_hc) => Err(RuntimeError::Generic(
                        "Host call required in param binding".into(),
                    )),
                }
            };
            match bind_parameters(&param_map, &mut eval_cb) {
                Ok(bound) => {
                    // Create a child environment with the current env as parent
                    let parent_rc = Arc::new(env.clone());
                    let mut child = Environment::with_parent(parent_rc);
                    // Insert bound params into child environment under reserved symbol %params
                    let mut map_vals = std::collections::HashMap::new();
                    for (k, v) in bound.into_iter() {
                        map_vals.insert(crate::ast::MapKey::String(k), v);
                    }
                    let sym = crate::ast::Symbol("%params".to_string());
                    child.define(&sym, Value::Map(map_vals));
                    _child_env_opt = Some(child);
                }
                Err(e) => {
                    return Err(RuntimeError::from(e));
                }
            }
            // body_env will refer to the child we created
            body_env = _child_env_opt.as_mut().unwrap();
        } else {
            // No params supplied; evaluate body in the existing environment
            body_env = env;
        }

        // 5. Notify host that step has started
        let step_action_id = self.host.notify_step_started(&step_name)?;

        // 6. Evaluate the body of the step in the appropriate environment
        let body_exprs = &args[i..];
        let mut last_result = Value::Nil;

        for expr in body_exprs {
            match self.eval_expr(expr, body_env)? {
                ExecutionOutcome::Complete(v) => last_result = v,
                ExecutionOutcome::RequiresHost(hc) => {
                    // On host-requirement, notify host of interruption and propagate
                    self.host.notify_step_failed(
                        &step_action_id,
                        "Host call required during step execution",
                    )?;
                    // ContextManager removed - step lifecycle handled by host
                    self.host.clear_step_exposure_override();
                    return Ok(ExecutionOutcome::RequiresHost(hc));
                }
            }
        }

        // 7. Notify host of successful completion
        let exec_result = ExecutionResultStruct {
            success: true,
            value: last_result.clone(),
            metadata: Default::default(),
        };
        self.host
            .notify_step_completed(&step_action_id, &exec_result)?;

        // 8. Clear step exposure override (step lifecycle handled by host)
        self.host.clear_step_exposure_override();

        Ok(ExecutionOutcome::Complete(last_result))
    }

    fn eval_step_if_form(
        &self,
        args: &[Expression],
        env: &mut Environment,
    ) -> Result<ExecutionOutcome, RuntimeError> {
        // Validate arguments: [options] condition then-branch [else-branch]
        if args.len() < 2 {
            return Err(RuntimeError::InvalidArguments {
                expected: "at least 2 (condition then-branch [else-branch])".to_string(),
                actual: args.len().to_string(),
            });
        }

        // Parse optional options
        use crate::ast::Literal as Lit;
        let mut i: usize = 0;
        let mut expose_override: Option<bool> = None;
        let mut context_keys_override: Option<Vec<String>> = None;
        while i + 1 < args.len() {
            match &args[i] {
                Expression::Literal(Lit::Keyword(k)) => {
                    let key = k.0.as_str();
                    let val_expr = &args[i + 1];
                    match (key, val_expr) {
                        ("expose-context", Expression::Literal(Lit::Boolean(b))) => {
                            expose_override = Some(*b);
                            i += 2;
                            continue;
                        }
                        ("context-keys", Expression::Vector(v)) => {
                            let mut keys: Vec<String> = Vec::new();
                            for e in v {
                                if let Expression::Literal(Lit::String(s)) = e {
                                    keys.push(s.clone());
                                } else {
                                    return Err(RuntimeError::InvalidArguments {
                                        expected: "vector of strings for :context-keys".to_string(),
                                        actual: serde_json::to_string(&e)
                                            .unwrap_or_else(|_| format!("{:?}", e)),
                                    });
                                }
                            }
                            context_keys_override = Some(keys);
                            i += 2;
                            continue;
                        }
                        _ => {
                            break;
                        }
                    }
                }
                _ => break,
            }
        }

        // Extract condition and branches after options
        if args.len().saturating_sub(i) < 2 {
            return Err(RuntimeError::InvalidArguments {
                expected: "condition then-branch [else-branch] after options".to_string(),
                actual: format!("{}", args.len().saturating_sub(i)),
            });
        }
        let condition_expr = &args[i + 0];
        let then_branch = &args[i + 1];
        let else_branch = args.get(i + 2);

        // 1. Enter step context and notify host that step-if has started
        let step_name = "step-if";
        {
            // Enforce isolation policy
            if !self
                .security_context
                .is_isolation_allowed(&IsolationLevel::Inherit)
            {
                return Err(RuntimeError::SecurityViolation {
                    operation: "step-if".to_string(),
                    capability: "isolation:inherit".to_string(),
                    context: format!(
                        "Isolation level not permitted: Inherit under {:?}",
                        self.security_context.security_level
                    ),
                });
            }
            // ContextManager removed - step lifecycle handled by host
        }
        // Apply step exposure override if provided
        if let Some(expose) = expose_override {
            self.host
                .set_step_exposure_override(expose, context_keys_override.clone());
        }
        let step_action_id = self.host.notify_step_started(step_name)?;

        // 2. Evaluate the condition
        let condition_value = match self.eval_expr(condition_expr, env)? {
            ExecutionOutcome::Complete(v) => v,
            ExecutionOutcome::RequiresHost(hc) => {
                self.host.notify_step_failed(
                    &step_action_id,
                    "Host call required during step-if condition",
                )?;
                // ContextManager removed - step lifecycle handled by host
                self.host.clear_step_exposure_override();
                return Ok(ExecutionOutcome::RequiresHost(hc));
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
        let branch_to_execute = if condition_bool {
            then_branch
        } else {
            else_branch.unwrap_or(&Expression::Literal(crate::ast::Literal::Nil))
        };

        let result = match self.eval_expr(branch_to_execute, env)? {
            ExecutionOutcome::Complete(v) => v,
            ExecutionOutcome::RequiresHost(hc) => {
                self.host.notify_step_failed(
                    &step_action_id,
                    "Host call required during step-if branch",
                )?;
                // ContextManager removed - step lifecycle handled by host
                return Ok(ExecutionOutcome::RequiresHost(hc));
            }
        };

        // 4. Notify host of successful completion
        let exec_result = ExecutionResultStruct {
            success: true,
            value: result.clone(),
            metadata: Default::default(),
        };
        self.host
            .notify_step_completed(&step_action_id, &exec_result)?;

        // 5. Clear step exposure override (step lifecycle handled by host)
        self.host.clear_step_exposure_override();

        Ok(ExecutionOutcome::Complete(result))
    }

    fn eval_step_loop_form(
        &self,
        args: &[Expression],
        env: &mut Environment,
    ) -> Result<ExecutionOutcome, RuntimeError> {
        // Validate arguments: [options] condition body
        if args.len() < 2 {
            return Err(RuntimeError::InvalidArguments {
                expected: "(condition body) optionally preceded by options".to_string(),
                actual: args.len().to_string(),
            });
        }
        // Parse optional options
        use crate::ast::Literal as Lit;
        let mut i: usize = 0;
        let mut expose_override: Option<bool> = None;
        let mut context_keys_override: Option<Vec<String>> = None;
        while i + 1 < args.len() {
            match &args[i] {
                Expression::Literal(Lit::Keyword(k)) => {
                    let key = k.0.as_str();
                    let val_expr = &args[i + 1];
                    match (key, val_expr) {
                        ("expose-context", Expression::Literal(Lit::Boolean(b))) => {
                            expose_override = Some(*b);
                            i += 2;
                            continue;
                        }
                        ("context-keys", Expression::Vector(v)) => {
                            let mut keys: Vec<String> = Vec::new();
                            for e in v {
                                if let Expression::Literal(Lit::String(s)) = e {
                                    keys.push(s.clone());
                                } else {
                                    return Err(RuntimeError::InvalidArguments {
                                        expected: "vector of strings for :context-keys".to_string(),
                                        actual: serde_json::to_string(&e)
                                            .unwrap_or_else(|_| format!("{:?}", e)),
                                    });
                                }
                            }
                            context_keys_override = Some(keys);
                            i += 2;
                            continue;
                        }
                        _ => {
                            break;
                        }
                    }
                }
                _ => break,
            }
        }
        if args.len().saturating_sub(i) != 2 {
            return Err(RuntimeError::InvalidArguments {
                expected: "condition body after options".to_string(),
                actual: format!("{}", args.len().saturating_sub(i)),
            });
        }
        let condition_expr = &args[i + 0];
        let body_expr = &args[i + 1];

        // 1. Enter step context and notify host that step-loop has started
        let step_name = "step-loop";
        {
            // Enforce isolation policy
            if !self
                .security_context
                .is_isolation_allowed(&IsolationLevel::Inherit)
            {
                return Err(RuntimeError::SecurityViolation {
                    operation: "step-loop".to_string(),
                    capability: "isolation:inherit".to_string(),
                    context: format!(
                        "Isolation level not permitted: Inherit under {:?}",
                        self.security_context.security_level
                    ),
                });
            }
            // ContextManager removed - step lifecycle handled by host
        }
        if let Some(expose) = expose_override {
            self.host
                .set_step_exposure_override(expose, context_keys_override.clone());
        }
        let step_action_id = self.host.notify_step_started(step_name)?;

        let mut last_result = Value::Nil;
        let mut iteration_count = 0;
        const MAX_ITERATIONS: usize = 10000; // Safety limit to prevent infinite loops

        loop {
            // Check iteration limit
            if iteration_count >= MAX_ITERATIONS {
                let error_msg = format!("Loop exceeded maximum iterations ({})", MAX_ITERATIONS);
                self.host.notify_step_failed(&step_action_id, &error_msg)?;
                // Exit step context on failure
                // ContextManager removed - step lifecycle handled by host
                return Err(RuntimeError::Generic(error_msg));
            }

            // Evaluate the condition
            let condition_value = match self.eval_expr(condition_expr, env)? {
                ExecutionOutcome::Complete(v) => v,
                ExecutionOutcome::RequiresHost(hc) => {
                    self.host.notify_step_failed(
                        &step_action_id,
                        "Host call required during loop condition",
                    )?;
                    // ContextManager removed - step lifecycle handled by host
                    self.host.clear_step_exposure_override();
                    return Ok(ExecutionOutcome::RequiresHost(hc));
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
            match self.eval_expr(body_expr, env)? {
                ExecutionOutcome::Complete(v) => last_result = v,
                ExecutionOutcome::RequiresHost(hc) => {
                    self.host.notify_step_failed(
                        &step_action_id,
                        "Host call required during loop body",
                    )?;
                    // ContextManager removed - step lifecycle handled by host
                    self.host.clear_step_exposure_override();
                    return Ok(ExecutionOutcome::RequiresHost(hc));
                }
            }
            iteration_count += 1;
        }

        // 2. Notify host of successful completion
        let exec_result = ExecutionResultStruct {
            success: true,
            value: last_result.clone(),
            metadata: Default::default(),
        };
        self.host
            .notify_step_completed(&step_action_id, &exec_result)?;

        // 3. Clear step exposure override (step lifecycle handled by host)
        self.host.clear_step_exposure_override();

        Ok(ExecutionOutcome::Complete(last_result))
    }

    fn eval_step_parallel_form(
        &self,
        args: &[Expression],
        env: &mut Environment,
    ) -> Result<ExecutionOutcome, RuntimeError> {
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

        // 2. Parse optional keyword arguments (e.g., :merge-policy :overwrite, :expose-context, :context-keys)
        use crate::ast::Literal as Lit;
        let mut i: usize = 0;
        let mut _merge_policy = ConflictResolution::KeepExisting;
        let mut expose_override: Option<bool> = None;
        let mut context_keys_override: Option<Vec<String>> = None;
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
                        Expression::Literal(Lit::Boolean(b)) => Value::Boolean(*b),
                        Expression::Vector(v) if key == "context-keys" => {
                            // Special-case vector literal for context-keys
                            let mut keys: Vec<String> = Vec::new();
                            for e in v {
                                if let Expression::Literal(Lit::String(s)) = e {
                                    keys.push(s.clone());
                                } else {
                                    return Err(RuntimeError::InvalidArguments {
                                        expected: "vector of strings for :context-keys".to_string(),
                                        actual: serde_json::to_string(&e)
                                            .unwrap_or_else(|_| format!("{:?}", e)),
                                    });
                                }
                            }
                            // Store and advance, continue loop
                            context_keys_override = Some(keys);
                            i += 2;
                            continue;
                        }
                        other => {
                            // Stop parsing options if non-literal encountered to avoid
                            // consuming actual branch expressions
                            if i == 0 {
                                // Unknown option style; treat as invalid args
                                return Err(RuntimeError::InvalidArguments {
                                    expected: "keyword-value pairs (e.g., :merge-policy :overwrite) followed by branch expressions".to_string(),
                                    actual: serde_json::to_string(&other).unwrap_or_else(|_| format!("{:?}", other)),
                                });
                            }
                            break;
                        }
                    };

                    if key == "merge-policy" || key == "merge_policy" {
                        // Map value to ConflictResolution
                        _merge_policy = match val {
                            Value::Keyword(crate::ast::Keyword(s)) | Value::String(s) => {
                                match s.as_str() {
                                    "keep-existing" | "keep_existing" | "parent-wins"
                                    | "parent_wins" => ConflictResolution::KeepExisting,
                                    "overwrite" | "child-wins" | "child_wins" => {
                                        ConflictResolution::Overwrite
                                    }
                                    "merge" => ConflictResolution::Merge,
                                    other => {
                                        return Err(RuntimeError::InvalidArguments {
                                            expected: ":keep-existing | :overwrite | :merge"
                                                .to_string(),
                                            actual: other.to_string(),
                                        });
                                    }
                                }
                            }
                            _ => ConflictResolution::KeepExisting,
                        };
                        i += 2;
                        continue;
                    } else if key == "expose-context" || key == "expose_context" {
                        expose_override = match val {
                            Value::Boolean(b) => Some(b),
                            _ => None,
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

        // 3. Apply step exposure override if provided
        if let Some(expose) = expose_override {
            self.host
                .set_step_exposure_override(expose, context_keys_override.clone());
        }

        // 4. Create and use isolated contexts per branch on demand

        // Sequential execution with isolation, plus deterministic merging after each branch
        let mut results: Vec<Value> = Vec::with_capacity(args.len().saturating_sub(i));
        let last_error: Option<RuntimeError> = None;
        for (rel_index, expr) in args[i..].iter().enumerate() {
            let index = i + rel_index;
            // Begin isolated child context for this branch (also switches into it)
            let _child_id = {
                // Enforce isolation policy for isolated branch contexts
                if !self
                    .security_context
                    .is_isolation_allowed(&IsolationLevel::Isolated)
                {
                    return Err(RuntimeError::SecurityViolation {
                        operation: "step-parallel".to_string(),
                        capability: "isolation:isolated".to_string(),
                        context: format!(
                            "Isolation level not permitted: Isolated under {:?}",
                            self.security_context.security_level
                        ),
                    });
                }
                // ContextManager removed - parallel execution isolation handled by host
                format!("parallel-{}", index)
            };
            match self.eval_expr(expr, env)? {
                ExecutionOutcome::Complete(v) => {
                    results.push(v);
                    // ContextManager removed - parallel execution isolation handled by host
                }
                ExecutionOutcome::RequiresHost(hc) => {
                    self.host.notify_step_failed(
                        &step_action_id,
                        "Host call required during parallel branch",
                    )?;
                    return Ok(ExecutionOutcome::RequiresHost(hc));
                }
            }
        }
        if let Some(err) = last_error {
            self.host
                .notify_step_failed(&step_action_id, &err.to_string())?;
            return Err(err);
        }

        // Clear exposure override at the end
        self.host.clear_step_exposure_override();

        // 5. Notify host of successful completion
        let final_result = Value::Vector(results);
        let exec_result = ExecutionResultStruct {
            success: true,
            value: final_result.clone(),
            metadata: Default::default(),
        };
        self.host
            .notify_step_completed(&step_action_id, &exec_result)?;

        Ok(ExecutionOutcome::Complete(final_result))
    }

    fn eval_get_form(
        &self,
        args: &[Expression],
        env: &mut Environment,
    ) -> Result<ExecutionOutcome, RuntimeError> {
        // Support shorthand (get :key) to read from current step context or
        // cross-plan params via get_with_cross_plan_fallback. For other
        // arities defer to the existing builtin 'get' implementation by
        // evaluating arguments and calling the builtin function.
        if args.len() == 1 {
            match &args[0] {
                Expression::Literal(crate::ast::Literal::Keyword(k)) => {
                    // First try local evaluator environment (set! stores symbols into env)
                    let sym = crate::ast::Symbol(k.0.clone());
                    if let Some(v) = env.lookup(&sym) {
                        return Ok(ExecutionOutcome::Complete(v));
                    }
                    // Then try step context / cross-plan fallback
                    if let Some(v) = self.get_with_cross_plan_fallback(&k.0) {
                        return Ok(ExecutionOutcome::Complete(v));
                    }
                    return Ok(ExecutionOutcome::Complete(Value::Nil));
                }
                Expression::Symbol(s) => {
                    // First try local evaluator environment
                    let sym = crate::ast::Symbol(s.0.clone());
                    if let Some(v) = env.lookup(&sym) {
                        return Ok(ExecutionOutcome::Complete(v));
                    }
                    // Then try step context / cross-plan fallback
                    if let Some(v) = self.get_with_cross_plan_fallback(&s.0) {
                        return Ok(ExecutionOutcome::Complete(v));
                    }
                    return Ok(ExecutionOutcome::Complete(Value::Nil));
                }
                _ => {
                    return Err(RuntimeError::InvalidArguments {
                        expected: "a keyword or symbol when using (get :key) shorthand".to_string(),
                        actual: serde_json::to_string(&args[0])
                            .unwrap_or_else(|_| format!("{:?}", args[0])),
                    })
                }
            }
        }

        // For other arities, evaluate args and call builtin 'get'
        let mut evaluated_args: Vec<Value> = Vec::new();
        for e in args {
            match self.eval_expr(e, env)? {
                ExecutionOutcome::Complete(v) => evaluated_args.push(v),
                ExecutionOutcome::RequiresHost(hc) => {
                    return Ok(ExecutionOutcome::RequiresHost(hc))
                }
                #[cfg(feature = "effect-boundary")]
                ExecutionOutcome::RequiresHost(host_call) => {
                    return Ok(ExecutionOutcome::RequiresHost(host_call))
                }
            }
        }

        // Lookup builtin 'get' in the environment and call it
        if let Some(val) = env.lookup(&crate::ast::Symbol("get".to_string())) {
            match val {
                Value::Function(Function::Builtin(bf)) => {
                    return Ok(ExecutionOutcome::Complete((bf.func)(evaluated_args)?))
                }
                Value::Function(Function::BuiltinWithContext(bfctx)) => {
                    return (bfctx.func)(evaluated_args, self, env)
                        .map(|v| ExecutionOutcome::Complete(v))
                }
                other => {
                    // Fallback to generic call path
                    return self.call_function(other, &evaluated_args, env);
                }
            }
        }

        Err(RuntimeError::UndefinedSymbol(crate::ast::Symbol(
            "get".to_string(),
        )))
    }

    /// LLM execution bridge special form
    /// Usage:
    ///   (llm-execute "model-id" "prompt")
    ///   (llm-execute :model "model-id" :prompt "prompt text" [:system "system prompt"])
    fn eval_llm_execute_form(
        &self,
        args: &[Expression],
        env: &mut Environment,
    ) -> Result<ExecutionOutcome, RuntimeError> {
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
            match self.eval_expr(&args[0], env)? {
                ExecutionOutcome::Complete(Value::String(mstr)) => {
                    match self.eval_expr(&args[1], env)? {
                        ExecutionOutcome::Complete(Value::String(pstr)) => {
                            model_id = Some(mstr);
                            prompt = Some(pstr);
                        }
                        ExecutionOutcome::Complete(other) => {
                            return Err(RuntimeError::InvalidArguments {
                                expected: "prompt string".to_string(),
                                actual: serde_json::to_string(&other)
                                    .unwrap_or_else(|_| format!("{:?}", other)),
                            })
                        }
                        ExecutionOutcome::RequiresHost(hc) => {
                            return Ok(ExecutionOutcome::RequiresHost(hc))
                        }
                        #[cfg(feature = "effect-boundary")]
                        ExecutionOutcome::RequiresHost(host_call) => {
                            return Ok(ExecutionOutcome::RequiresHost(host_call))
                        }
                    }
                }
                ExecutionOutcome::Complete(other) => {
                    return Err(RuntimeError::InvalidArguments {
                        expected: "model id string".to_string(),
                        actual: serde_json::to_string(&other)
                            .unwrap_or_else(|_| format!("{:?}", other)),
                    })
                }
                ExecutionOutcome::RequiresHost(hc) => {
                    return Ok(ExecutionOutcome::RequiresHost(hc))
                }
                #[cfg(feature = "effect-boundary")]
                ExecutionOutcome::RequiresHost(host_call) => {
                    return Ok(ExecutionOutcome::RequiresHost(host_call))
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
                            expected: "keyword-value pairs (e.g., :model \"id\" :prompt \"text\")"
                                .to_string(),
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
                // eval_expr returns ExecutionOutcome; unwrap or propagate RequiresHost
                let outcome = self.eval_expr(&args[i], env)?;
                let val = match outcome {
                    ExecutionOutcome::Complete(v) => v,
                    ExecutionOutcome::RequiresHost(hc) => {
                        return Ok(ExecutionOutcome::RequiresHost(hc))
                    }
                    #[cfg(feature = "effect-boundary")]
                    ExecutionOutcome::RequiresHost(host_call) => {
                        return Ok(ExecutionOutcome::RequiresHost(host_call))
                    }
                };
                match (key.as_str(), val) {
                    ("model", Value::String(s)) => model_id = Some(s),
                    ("prompt", Value::String(s)) => prompt = Some(s),
                    ("system", Value::String(s)) => system_prompt = Some(s),
                    (k, v) => {
                        return Err(RuntimeError::InvalidArguments {
                            expected: format!("string value for {}", k),
                            actual: serde_json::to_string(&v)
                                .unwrap_or_else(|_| format!("{:?}", v)),
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
        let _final_prompt = if let Some(sys) = system_prompt {
            format!("System:\n{}\n\nUser:\n{}", sys, prompt)
        } else {
            prompt.clone()
        };

        // Handle special echo-model for testing
        let output = if model_id == "echo-model" {
            // For echo-model, just return the prompt text (echo behavior)
            prompt.clone()
        } else {
            // For other models, return a placeholder
            format!("[Model inference placeholder for {}]", model_id)
        };

        let value = Value::String(output);
        let exec_result = ExecutionResultStruct {
            success: true,
            value: value.clone(),
            metadata: Default::default(),
        };

        // Notify host of completion
        self.host
            .notify_step_completed(&step_action_id, &exec_result)?;
        Ok(ExecutionOutcome::Complete(value))
    }

    /// Evaluate an expression in the global environment
    pub fn evaluate(&self, expr: &Expression) -> Result<ExecutionOutcome, RuntimeError> {
        let mut env = self.env.clone();
        self.eval_expr(expr, &mut env)
    }

    /// Evaluate an expression with a provided environment
    pub fn evaluate_with_env(
        &self,
        expr: &Expression,
        env: &mut Environment,
    ) -> Result<ExecutionOutcome, RuntimeError> {
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
            Literal::Symbol(s) => Value::Symbol(s.clone()),
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
                Literal::Integer(_) => {
                    crate::ast::TypeExpr::Primitive(crate::ast::PrimitiveType::Int)
                }
                Literal::Float(_) => {
                    crate::ast::TypeExpr::Primitive(crate::ast::PrimitiveType::Float)
                }
                Literal::String(_) => {
                    crate::ast::TypeExpr::Primitive(crate::ast::PrimitiveType::String)
                }
                Literal::Boolean(_) => {
                    crate::ast::TypeExpr::Primitive(crate::ast::PrimitiveType::Bool)
                }
                Literal::Keyword(_) => {
                    crate::ast::TypeExpr::Primitive(crate::ast::PrimitiveType::Keyword)
                }
                Literal::Symbol(_) => {
                    crate::ast::TypeExpr::Primitive(crate::ast::PrimitiveType::Symbol)
                }
                Literal::Nil => crate::ast::TypeExpr::Primitive(crate::ast::PrimitiveType::Nil),
                Literal::Timestamp(_) | Literal::Uuid(_) | Literal::ResourceHandle(_) => {
                    crate::ast::TypeExpr::Primitive(crate::ast::PrimitiveType::String)
                }
            };

            // Validate using the optimization system
            let context = self.create_local_verification_context(true); // compile_time_verified = true
            self.type_validator
                .validate_with_config(&value, &inferred_type, &self.type_config, &context)
                .map_err(|e| RuntimeError::TypeValidationError(e.to_string()))?;

            Ok(value)
        }
    }

    pub fn call_function(
        &self,
        func_value: Value,
        args: &[Value],
        env: &mut Environment,
    ) -> Result<ExecutionOutcome, RuntimeError> {
        match func_value {
            Value::FunctionPlaceholder(cell) => {
                let guard = cell
                    .read()
                    .map_err(|e| RuntimeError::InternalError(format!("RwLock poisoned: {}", e)))?;
                let f = guard.clone();
                if let Value::Function(f) = f {
                    let mut deeper_evaluator = self.clone();
                    deeper_evaluator.recursion_depth += 1;
                    deeper_evaluator.call_function(Value::Function(f), args, env)
                } else {
                    Err(RuntimeError::InternalError(
                        "Function placeholder not resolved".to_string(),
                    ))
                }
            }
            Value::Function(Function::Builtin(func)) => {
                // debug: removed temporary diagnostic print for reduce arity
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

                // No special interception here for :ccos.user.ask — allow capability implementation
                // (e.g., local stdin-reading capability) to execute normally.

                // Call the function
                let result = (func.func)(args.to_vec())?;

                // Validate function result for known signatures
                self.validate_builtin_function_result(&func.name, &result, args)?;

                Ok(ExecutionOutcome::Complete(result))
            }
            Value::Function(Function::BuiltinWithContext(func)) => {
                // debug: removed temporary diagnostic print for reduce arity
                // Check arity
                if !self.check_arity(&func.arity, args.len()) {
                    return Err(RuntimeError::ArityMismatch {
                        function: func.name,
                        expected: self.arity_to_string(&func.arity),
                        actual: args.len(),
                    });
                }
                // Allow builtin-with-context calls (including capability calls) to run the
                // registered capability implementation (e.g., ccos.user.ask reading stdin).
                // Call the builtin-with-context and wrap return value into ExecutionOutcome
                let res = (func.func)(args.to_vec(), self, env)?;
                Ok(ExecutionOutcome::Complete(res))
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
                            // Yield control to CCOS for model execution
                            let host_call = HostCall {
                                capability_id: format!("model-call:{}", id),
                                args: args.to_vec(),
                                security_context: self.security_context.clone(),
                                causal_context: None,
                                metadata: Some(CallMetadata::new()),
                            };
                            return Ok(ExecutionOutcome::RequiresHost(host_call));
                        }
                    }
                } else {
                    // No hint: check if this is a non-pure function that requires delegation
                    let fn_symbol = env
                        .find_function_name(&func_value)
                        .unwrap_or("unknown-function");
                    if self.is_non_pure_function(fn_symbol) {
                        // Yield control to CCOS for non-pure operations
                        let host_call = HostCall {
                            capability_id: fn_symbol.to_string(),
                            args: args.to_vec(),
                            security_context: self.security_context.clone(),
                            causal_context: None,
                            metadata: Some(CallMetadata::new()),
                        };
                        return Ok(ExecutionOutcome::RequiresHost(host_call));
                    }
                    // Pure functions continue with normal execution (fall through)
                }
                // Create new environment for function execution, parented by the captured closure
                let mut func_env = Environment::with_parent(closure.env.clone());

                // Bind arguments to parameter patterns (supports destructuring and variadic)
                if let Some(variadic_symbol) = &closure.variadic_param {
                    // This closure has a variadic parameter
                    let required_param_count = closure.param_patterns.len();

                    // Check minimum argument count for required parameters
                    if args.len() < required_param_count {
                        return Err(RuntimeError::ArityMismatch {
                            function: "user-defined function".to_string(),
                            expected: format!("at least {}", required_param_count),
                            actual: args.len(),
                        });
                    }

                    // Validate required parameters against annotations if present
                    if closure.param_type_annotations.len() == closure.param_patterns.len() {
                        for i in 0..required_param_count {
                            if let Some(t) = closure.param_type_annotations[i].clone() {
                                self.type_validator
                                    .validate_with_config(
                                        &args[i],
                                        &t,
                                        &self.type_config,
                                        &VerificationContext::default(),
                                    )
                                    .map_err(|e| {
                                        RuntimeError::TypeValidationError(e.to_string())
                                    })?;
                            }
                        }
                    }

                    // Bind required parameters normally
                    for (i, pat) in closure.param_patterns.iter().enumerate() {
                        self.bind_pattern(pat, &args[i], &mut func_env)?;
                    }

                    // Bind variadic parameter - collect remaining args into a list
                    let rest_args = if args.len() > required_param_count {
                        args[required_param_count..].to_vec()
                    } else {
                        Vec::new()
                    };
                    // Validate variadic items against their annotation if present
                    if let Some(var_type) = &closure.variadic_param_type {
                        for (j, a) in rest_args.iter().enumerate() {
                            self.type_validator
                                .validate_with_config(
                                    a,
                                    var_type,
                                    &self.type_config,
                                    &VerificationContext::default(),
                                )
                                .map_err(|e| {
                                    RuntimeError::TypeValidationError(format!(
                                        "variadic argument {}: {}",
                                        j, e
                                    ))
                                })?;
                        }
                    }
                    func_env.define(variadic_symbol, Value::List(rest_args));
                } else if !closure.param_patterns.is_empty() {
                    // Normal parameter binding for non-variadic functions
                    if closure.param_patterns.len() != args.len() {
                        return Err(RuntimeError::ArityMismatch {
                            function: "user-defined function".to_string(),
                            expected: closure.param_patterns.len().to_string(),
                            actual: args.len(),
                        });
                    }
                    // Validate parameters against annotations if present
                    if closure.param_type_annotations.len() == closure.param_patterns.len() {
                        for (i, arg) in args.iter().enumerate() {
                            if let Some(t) = closure.param_type_annotations[i].clone() {
                                self.type_validator
                                    .validate_with_config(
                                        arg,
                                        &t,
                                        &self.type_config,
                                        &VerificationContext::default(),
                                    )
                                    .map_err(|e| {
                                        RuntimeError::TypeValidationError(e.to_string())
                                    })?;
                            }
                        }
                    }
                    for (pat, arg) in closure.param_patterns.iter().zip(args.iter()) {
                        self.bind_pattern(pat, arg, &mut func_env)?;
                    }
                }

                // Execute function body with incremented recursion depth
                let mut deeper_evaluator = self.clone();
                deeper_evaluator.recursion_depth += 1;
                match deeper_evaluator.eval_expr(&closure.body, &mut func_env)? {
                    ExecutionOutcome::Complete(v) => {
                        // Enforce return type if declared
                        if let Some(ret_t) = &closure.return_type {
                            self.type_validator
                                .validate_with_config(
                                    &v,
                                    ret_t,
                                    &self.type_config,
                                    &VerificationContext::default(),
                                )
                                .map_err(|e| RuntimeError::TypeValidationError(e.to_string()))?;
                        }
                        Ok(ExecutionOutcome::Complete(v))
                    }
                    ExecutionOutcome::RequiresHost(hc) => Ok(ExecutionOutcome::RequiresHost(hc)),
                    #[cfg(feature = "effect-boundary")]
                    ExecutionOutcome::RequiresHost(host_call) => {
                        Ok(ExecutionOutcome::RequiresHost(host_call))
                    }
                }
            }
            Value::Keyword(keyword) => {
                // Keywords act as functions: (:key map) is equivalent to (get map :key)
                if args.len() == 1 {
                    match &args[0] {
                        Value::Map(map) => {
                            let map_key = crate::ast::MapKey::Keyword(keyword);
                            Ok(ExecutionOutcome::Complete(
                                map.get(&map_key).cloned().unwrap_or(Value::Nil),
                            ))
                        }
                        Value::Nil => Ok(ExecutionOutcome::Complete(Value::Nil)),
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
                            Ok(ExecutionOutcome::Complete(
                                map.get(&map_key).cloned().unwrap_or(args[1].clone()),
                            ))
                        }
                        Value::Nil => Ok(ExecutionOutcome::Complete(args[1].clone())),
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

    fn eval_if(
        &self,
        if_expr: &IfExpr,
        env: &mut Environment,
    ) -> Result<ExecutionOutcome, RuntimeError> {
        let condition_out = self.eval_expr(&if_expr.condition, env)?;
        let condition = match condition_out {
            ExecutionOutcome::Complete(v) => v,
            ExecutionOutcome::RequiresHost(hc) => return Ok(ExecutionOutcome::RequiresHost(hc)),
            #[cfg(feature = "effect-boundary")]
            ExecutionOutcome::RequiresHost(host_call) => {
                return Ok(ExecutionOutcome::RequiresHost(host_call))
            }
        };

        if condition.is_truthy() {
            self.eval_expr(&if_expr.then_branch, env)
        } else if let Some(else_branch) = &if_expr.else_branch {
            self.eval_expr(else_branch, env)
        } else {
            Ok(ExecutionOutcome::Complete(Value::Nil))
        }
    }
    fn eval_let(
        &self,
        let_expr: &LetExpr,
        env: &mut Environment,
    ) -> Result<ExecutionOutcome, RuntimeError> {
        // Check if we should use recursive evaluation
        if self.should_use_recursive_evaluation(let_expr) {
            match self.eval_let_with_recursion(let_expr, env)? {
                ExecutionOutcome::Complete(v) => Ok(ExecutionOutcome::Complete(v)),
                ExecutionOutcome::RequiresHost(hc) => Ok(ExecutionOutcome::RequiresHost(hc)),
                #[cfg(feature = "effect-boundary")]
                ExecutionOutcome::RequiresHost(host_call) => {
                    Ok(ExecutionOutcome::RequiresHost(host_call))
                }
            }
        } else {
            match self.eval_let_simple(let_expr, env)? {
                ExecutionOutcome::Complete(v) => Ok(ExecutionOutcome::Complete(v)),
                ExecutionOutcome::RequiresHost(hc) => Ok(ExecutionOutcome::RequiresHost(hc)),
                #[cfg(feature = "effect-boundary")]
                ExecutionOutcome::RequiresHost(host_call) => {
                    Ok(ExecutionOutcome::RequiresHost(host_call))
                }
            }
        }
    }

    fn should_use_recursive_evaluation(&self, let_expr: &LetExpr) -> bool {
        // First, check if all bindings are functions (original logic)
        let all_bindings_are_functions = let_expr
            .bindings
            .iter()
            .all(|binding| matches!(&*binding.value, Expression::Fn(_) | Expression::Defn(_)));

        if all_bindings_are_functions {
            return self.detect_recursion_in_let(&let_expr.bindings);
        }

        // Second, check for mixed cases where some bindings reference themselves
        // even when there are nested non-function bindings
        for binding in &let_expr.bindings {
            if let crate::ast::Pattern::Symbol(symbol) = &binding.pattern {
                let binding_names = std::collections::HashSet::from([symbol.0.as_str()]);
                if self.expr_references_symbols(&binding.value, &binding_names) {
                    return true;
                }
            }
        }

        false
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
            | Expression::List(_)
            | Expression::ResourceRef(_)
            | Expression::Defstruct(_)
            | Expression::For(_) => false,
            Expression::Vector(exprs) => {
                // Check if any expression in the vector references the symbols
                exprs
                    .iter()
                    .any(|expr| self.expr_references_symbols(expr, symbols))
            }
            Expression::Map(map) => {
                // Check if any value expression in the map references the symbols
                map.values()
                    .any(|expr| self.expr_references_symbols(expr, symbols))
            }
            Expression::Deref(expr) => self.expr_references_symbols(expr, symbols),
            Expression::Metadata(metadata_map) => {
                // Check if any metadata values reference the symbols
                metadata_map
                    .values()
                    .any(|expr| self.expr_references_symbols(expr, symbols))
            }
        }
    }

    fn eval_let_simple(
        &self,
        let_expr: &LetExpr,
        env: &mut Environment,
    ) -> Result<ExecutionOutcome, RuntimeError> {
        let mut let_env = Environment::with_parent(Arc::new(env.clone()));

        for binding in &let_expr.bindings {
            // Evaluate each binding value in the accumulated let environment
            // This allows sequential bindings to reference previous bindings
            match self.eval_expr(&binding.value, &mut let_env)? {
                ExecutionOutcome::Complete(v) => {
                    self.bind_pattern(&binding.pattern, &v, &mut let_env)?
                }
                ExecutionOutcome::RequiresHost(hc) => {
                    return Ok(ExecutionOutcome::RequiresHost(hc))
                }
                #[cfg(feature = "effect-boundary")]
                ExecutionOutcome::RequiresHost(host_call) => {
                    return Ok(ExecutionOutcome::RequiresHost(host_call))
                }
            }
        }

        self.eval_do_body(&let_expr.body, &mut let_env)
    }

    fn eval_let_with_recursion(
        &self,
        let_expr: &LetExpr,
        env: &mut Environment,
    ) -> Result<ExecutionOutcome, RuntimeError> {
        let mut letrec_env = Environment::with_parent(Arc::new(env.clone()));
        let mut placeholders = Vec::new();

        // First pass: create placeholders for all function bindings
        for binding in &let_expr.bindings {
            if let crate::ast::Pattern::Symbol(symbol) = &binding.pattern {
                let placeholder_cell = Arc::new(RwLock::new(Value::Nil));
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
            match self.eval_expr(&value_expr, &mut letrec_env)? {
                ExecutionOutcome::Complete(value) => {
                    if matches!(value, Value::Function(_)) {
                        let mut guard = placeholder_cell.write().map_err(|e| {
                            RuntimeError::InternalError(format!("RwLock poisoned: {}", e))
                        })?;
                        *guard = value;
                    } else {
                        return Err(RuntimeError::TypeError {
                            expected: "function".to_string(),
                            actual: value.type_name().to_string(),
                            operation: format!("binding {} in recursive let", symbol.0),
                        });
                    }
                }
                ExecutionOutcome::RequiresHost(hc) => {
                    return Ok(ExecutionOutcome::RequiresHost(hc))
                }
                #[cfg(feature = "effect-boundary")]
                ExecutionOutcome::RequiresHost(host_call) => {
                    return Ok(ExecutionOutcome::RequiresHost(host_call))
                }
            }
        }

        self.eval_do_body(&let_expr.body, &mut letrec_env)
    }

    fn eval_do(
        &self,
        do_expr: &DoExpr,
        env: &mut Environment,
    ) -> Result<ExecutionOutcome, RuntimeError> {
        self.eval_do_body(&do_expr.expressions, env)
    }

    fn eval_do_body(
        &self,
        exprs: &[Expression],
        env: &mut Environment,
    ) -> Result<ExecutionOutcome, RuntimeError> {
        if exprs.is_empty() {
            return Ok(ExecutionOutcome::Complete(Value::Nil));
        }

        let mut last_outcome = ExecutionOutcome::Complete(Value::Nil);
        for expr in exprs {
            match self.eval_expr(expr, env)? {
                ExecutionOutcome::Complete(v) => last_outcome = ExecutionOutcome::Complete(v),
                ExecutionOutcome::RequiresHost(hc) => {
                    return Ok(ExecutionOutcome::RequiresHost(hc))
                }
                #[cfg(feature = "effect-boundary")]
                ExecutionOutcome::RequiresHost(host_call) => {
                    return Ok(ExecutionOutcome::RequiresHost(host_call))
                }
            }
        }
        Ok(last_outcome)
    }

    fn eval_match(
        &self,
        match_expr: &MatchExpr,
        env: &mut Environment,
    ) -> Result<ExecutionOutcome, RuntimeError> {
        let value_to_match_out = self.eval_expr(&match_expr.expression, env)?;
        let value_to_match = match value_to_match_out {
            ExecutionOutcome::Complete(v) => v,
            ExecutionOutcome::RequiresHost(hc) => return Ok(ExecutionOutcome::RequiresHost(hc)),
            #[cfg(feature = "effect-boundary")]
            ExecutionOutcome::RequiresHost(host_call) => {
                return Ok(ExecutionOutcome::RequiresHost(host_call))
            }
        };

        for clause in &match_expr.clauses {
            let mut clause_env = Environment::with_parent(Arc::new(env.clone()));
            if self.match_match_pattern(&clause.pattern, &value_to_match, &mut clause_env)? {
                if let Some(guard) = &clause.guard {
                    let guard_result = self.eval_expr(guard, &mut clause_env)?;
                    // guard_result is ExecutionOutcome
                    match guard_result {
                        ExecutionOutcome::Complete(v) => {
                            if !v.is_truthy() {
                                continue;
                            }
                        }
                        ExecutionOutcome::RequiresHost(hc) => {
                            return Ok(ExecutionOutcome::RequiresHost(hc))
                        }
                        #[cfg(feature = "effect-boundary")]
                        ExecutionOutcome::RequiresHost(host_call) => {
                            return Ok(ExecutionOutcome::RequiresHost(host_call))
                        }
                    }
                }
                return self.eval_expr(&clause.body, &mut clause_env);
            }
        }

        Err(RuntimeError::MatchError("No matching clause".to_string()))
    }

    fn eval_log_step(
        &self,
        log_expr: &LogStepExpr,
        env: &mut Environment,
    ) -> Result<ExecutionOutcome, RuntimeError> {
        let level = log_expr
            .level
            .as_ref()
            .map(|k| k.0.as_str())
            .unwrap_or("info");
        let mut messages = Vec::new();
        for expr in &log_expr.values {
            match self.eval_expr(expr, env)? {
                ExecutionOutcome::Complete(v) => messages.push(v.to_string()),
                ExecutionOutcome::RequiresHost(hc) => {
                    return Ok(ExecutionOutcome::RequiresHost(hc))
                }
                #[cfg(feature = "effect-boundary")]
                ExecutionOutcome::RequiresHost(host_call) => {
                    return Ok(ExecutionOutcome::RequiresHost(host_call))
                }
            }
        }
        println!("[{}] {}", level, messages.join(" "));
        Ok(ExecutionOutcome::Complete(Value::Nil))
    }

    // Removed eval_set_form - no mutable variables allowed in RTFS 2.0

    fn eval_try_catch(
        &self,
        try_expr: &TryCatchExpr,
        env: &mut Environment,
    ) -> Result<ExecutionOutcome, RuntimeError> {
        let try_result = self.eval_do_body(&try_expr.try_body, env);
        match try_result {
            Ok(value) => {
                // Success path: run finally if present, propagate finally error if it fails
                if let Some(finally_body) = &try_expr.finally_body {
                    // Execute finally in the original environment
                    let finally_result = self.eval_do_body(finally_body, env);
                    if let Err(fe) = finally_result {
                        return Err(fe);
                    }
                }
                Ok(value)
            }
            Err(e) => {
                // Error path: try to match catch clauses
                let mut handled: Option<Value> = None;
                for catch_clause in &try_expr.catch_clauses {
                    let mut catch_env = Environment::with_parent(Arc::new(env.clone()));
                    if self.match_catch_pattern(
                        &catch_clause.pattern,
                        &e.to_value(),
                        &mut catch_env,
                        Some(&catch_clause.binding),
                    )? {
                        // If catch body errors, preserve that error (after running finally)
                        match self.eval_do_body(&catch_clause.body, &mut catch_env) {
                            Ok(ExecutionOutcome::Complete(v)) => {
                                handled = Some(v);
                            }
                            Ok(ExecutionOutcome::RequiresHost(hc)) => {
                                return Ok(ExecutionOutcome::RequiresHost(hc))
                            }
                            Err(catch_err) => {
                                // Run finally then return catch error
                                if let Some(finally_body) = &try_expr.finally_body {
                                    if let Err(fe) = self.eval_do_body(finally_body, env) {
                                        return Err(fe);
                                    }
                                }
                                return Err(catch_err);
                            }
                        }
                        break;
                    }
                }

                // Run finally if present
                if let Some(finally_body) = &try_expr.finally_body {
                    if let Err(fe) = self.eval_do_body(finally_body, env) {
                        return Err(fe);
                    }
                }

                if let Some(v) = handled {
                    Ok(ExecutionOutcome::Complete(v))
                } else {
                    Err(e)
                }
            }
        }
    }

    fn eval_fn(
        &self,
        fn_expr: &FnExpr,
        env: &mut Environment,
    ) -> Result<ExecutionOutcome, RuntimeError> {
        // Extract variadic parameter for anonymous functions if present
        let variadic_param = fn_expr
            .variadic_param
            .as_ref()
            .map(|p| self.extract_param_symbol(&p.pattern));

        Ok(ExecutionOutcome::Complete(Value::Function(
            Function::new_closure(
                fn_expr
                    .params
                    .iter()
                    .map(|p| self.extract_param_symbol(&p.pattern))
                    .collect(),
                fn_expr.params.iter().map(|p| p.pattern.clone()).collect(),
                fn_expr
                    .params
                    .iter()
                    .map(|p| p.type_annotation.clone())
                    .collect(),
                variadic_param,
                fn_expr
                    .variadic_param
                    .as_ref()
                    .and_then(|p| p.type_annotation.clone()),
                Box::new(Expression::Do(DoExpr {
                    expressions: fn_expr.body.clone(),
                })),
                Arc::new(env.clone()),
                fn_expr.delegation_hint.clone(),
                fn_expr.return_type.clone(),
            ),
        )))
    }

    fn eval_with_resource(
        &self,
        with_expr: &WithResourceExpr,
        env: &mut Environment,
    ) -> Result<ExecutionOutcome, RuntimeError> {
        let resource = self.eval_expr(&with_expr.resource_init, env)?;
        let mut resource_env = Environment::with_parent(Arc::new(env.clone()));
        match resource {
            ExecutionOutcome::Complete(v) => resource_env.define(&with_expr.resource_symbol, v),
            ExecutionOutcome::RequiresHost(hc) => return Ok(ExecutionOutcome::RequiresHost(hc)),
            #[cfg(feature = "effect-boundary")]
            ExecutionOutcome::RequiresHost(host_call) => {
                return Ok(ExecutionOutcome::RequiresHost(host_call))
            }
        }
        self.eval_do_body(&with_expr.body, &mut resource_env)
    }
    fn eval_parallel(
        &self,
        parallel_expr: &ParallelExpr,
        env: &mut Environment,
    ) -> Result<ExecutionOutcome, RuntimeError> {
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
            match value {
                ExecutionOutcome::Complete(v) => {
                    results.insert(MapKey::Keyword(Keyword(binding.symbol.0.clone())), v);
                }
                ExecutionOutcome::RequiresHost(hc) => {
                    return Ok(ExecutionOutcome::RequiresHost(hc))
                }
                #[cfg(feature = "effect-boundary")]
                ExecutionOutcome::RequiresHost(host_call) => {
                    return Ok(ExecutionOutcome::RequiresHost(host_call))
                }
            }
        }

        // Return results as a map (parallel bindings produce a map of results)
        Ok(ExecutionOutcome::Complete(Value::Map(results)))
    }

    fn eval_def(
        &self,
        def_expr: &DefExpr,
        env: &mut Environment,
    ) -> Result<ExecutionOutcome, RuntimeError> {
        match self.eval_expr(&def_expr.value, env)? {
            ExecutionOutcome::Complete(v) => {
                env.define(&def_expr.symbol, v.clone());
                Ok(ExecutionOutcome::Complete(v))
            }
            ExecutionOutcome::RequiresHost(hc) => Ok(ExecutionOutcome::RequiresHost(hc)),
            #[cfg(feature = "effect-boundary")]
            ExecutionOutcome::RequiresHost(host_call) => {
                Ok(ExecutionOutcome::RequiresHost(host_call))
            }
        }
    }

    fn eval_defn(
        &self,
        defn_expr: &DefnExpr,
        env: &mut Environment,
    ) -> Result<ExecutionOutcome, RuntimeError> {
        // Extract variadic parameter if present
        let variadic_param = defn_expr
            .variadic_param
            .as_ref()
            .map(|p| self.extract_param_symbol(&p.pattern));

        let function = Value::Function(Function::new_closure(
            defn_expr
                .params
                .iter()
                .map(|p| self.extract_param_symbol(&p.pattern))
                .collect(),
            defn_expr.params.iter().map(|p| p.pattern.clone()).collect(),
            defn_expr
                .params
                .iter()
                .map(|p| p.type_annotation.clone())
                .collect(),
            variadic_param,
            defn_expr
                .variadic_param
                .as_ref()
                .and_then(|p| p.type_annotation.clone()),
            Box::new(Expression::Do(DoExpr {
                expressions: defn_expr.body.clone(),
            })),
            Arc::new(env.clone()),
            defn_expr.delegation_hint.clone(),
            defn_expr.return_type.clone(),
        ));
        env.define(&defn_expr.name, function.clone());
        Ok(ExecutionOutcome::Complete(function))
    }

    pub fn eval_defstruct(
        &self,
        defstruct_expr: &DefstructExpr,
        env: &mut Environment,
    ) -> Result<ExecutionOutcome, RuntimeError> {
        let struct_name = defstruct_expr.name.0.clone();

        // Create a constructor function that validates inputs
        let struct_name_clone = struct_name.clone();
        let fields = defstruct_expr.fields.clone();

        let constructor = Function::BuiltinWithContext(BuiltinFunctionWithContext {
            name: format!("{}.new", struct_name),
            arity: Arity::Fixed(1), // Takes a map as input
            func: Arc::new(
                move |args: Vec<Value>,
                      evaluator: &Evaluator,
                      _env: &mut Environment|
                      -> RuntimeResult<Value> {
                    if args.len() != 1 {
                        return Err(RuntimeError::ArityMismatch {
                            function: struct_name_clone.clone(),
                            expected: "1".to_string(),
                            actual: args.len(),
                        });
                    }

                    let input_map = &args[0];

                    // Validate that input is a map
                    let Value::Map(map) = input_map else {
                        return Err(RuntimeError::TypeError {
                            expected: "map".to_string(),
                            actual: input_map.type_name().to_string(),
                            operation: format!("{} constructor", struct_name_clone),
                        });
                    };

                    // Check that all required fields are present and have correct types
                    for field in &fields {
                        let key = MapKey::Keyword(field.key.clone());

                        if let Some(value) = map.get(&key) {
                            // Validate the field type using the type validator
                            if let Err(validation_error) = evaluator
                                .type_validator
                                .validate_value(value, &field.field_type)
                            {
                                return Err(RuntimeError::TypeValidationError(format!(
                                    "Field {} failed type validation: {:?}",
                                    field.key.0, validation_error
                                )));
                            }
                        } else {
                            // Required field is missing
                            return Err(RuntimeError::TypeValidationError(format!(
                                "Required field {} is missing",
                                field.key.0
                            )));
                        }
                    }

                    // If all validations pass, return the input map (it's already a valid struct)
                    Ok(input_map.clone())
                },
            ),
        });

        // Store the constructor function in the environment
        let constructor_value = Value::Function(constructor);
        env.define(&defstruct_expr.name, constructor_value.clone());

        Ok(ExecutionOutcome::Complete(constructor_value))
    }

    /// Special form: (dotimes [i n] body)
    fn eval_dotimes_form(
        &self,
        args: &[Expression],
        env: &mut Environment,
    ) -> Result<ExecutionOutcome, RuntimeError> {
        if args.len() != 2 {
            return Err(RuntimeError::ArityMismatch {
                function: "dotimes".into(),
                expected: "2".into(),
                actual: args.len(),
            });
        }
        // Extract binding vector directly from AST (don't evaluate it)
        let (sym, count) = match &args[0] {
            Expression::Vector(v) if v.len() == 2 => {
                let sym = match &v[0] {
                    Expression::Symbol(s) => s.clone(),
                    _ => {
                        return Err(RuntimeError::TypeError {
                            expected: "symbol".into(),
                            actual: "non-symbol".into(),
                            operation: "dotimes".into(),
                        })
                    }
                };
                // Evaluate the count expression
                let count_val = self.eval_expr(&v[1], env)?;
                let count_val = match count_val {
                    ExecutionOutcome::Complete(v) => v,
                    ExecutionOutcome::RequiresHost(hc) => {
                        return Ok(ExecutionOutcome::RequiresHost(hc))
                    }
                    #[cfg(feature = "effect-boundary")]
                    ExecutionOutcome::RequiresHost(host_call) => {
                        return Ok(ExecutionOutcome::RequiresHost(host_call))
                    }
                };
                let n = match count_val {
                    Value::Integer(i) => i,
                    other => {
                        return Err(RuntimeError::TypeError {
                            expected: "integer".into(),
                            actual: other.type_name().into(),
                            operation: "dotimes".into(),
                        })
                    }
                };
                (sym, n)
            }
            _ => {
                return Err(RuntimeError::TypeError {
                    expected: "[symbol integer]".into(),
                    actual: "non-vector".into(),
                    operation: "dotimes".into(),
                })
            }
        };
        if count <= 0 {
            return Ok(ExecutionOutcome::Complete(Value::Nil));
        }
        let mut last = Value::Nil;
        for i in 0..count {
            // Create a child environment that can access parent variables
            let mut loop_env = Environment::with_parent(Arc::new(env.clone()));
            // Define the loop variable in the child environment
            loop_env.define(&sym, Value::Integer(i));
            // Evaluate the loop body in the child environment
            let body_res = self.eval_expr(&args[1], &mut loop_env)?;
            match body_res {
                ExecutionOutcome::Complete(v) => last = v,
                ExecutionOutcome::RequiresHost(hc) => {
                    return Ok(ExecutionOutcome::RequiresHost(hc))
                }
                #[cfg(feature = "effect-boundary")]
                ExecutionOutcome::RequiresHost(host_call) => {
                    return Ok(ExecutionOutcome::RequiresHost(host_call))
                }
            }
        }
        Ok(ExecutionOutcome::Complete(last))
    }

    /// Special form: (for [x coll] body) or (for [x coll y coll2 ...] body)
    /// Multi-binding form nests loops left-to-right and returns a vector of results
    fn eval_for_form(
        &self,
        args: &[Expression],
        env: &mut Environment,
    ) -> Result<ExecutionOutcome, RuntimeError> {
        if args.len() != 2 {
            return Err(RuntimeError::ArityMismatch {
                function: "for".into(),
                expected: "2".into(),
                actual: args.len(),
            });
        }
        let binding_val = self.eval_expr(&args[0], env)?;
        let binding_val = match binding_val {
            ExecutionOutcome::Complete(v) => v,
            ExecutionOutcome::RequiresHost(hc) => return Ok(ExecutionOutcome::RequiresHost(hc)),
            #[cfg(feature = "effect-boundary")]
            ExecutionOutcome::RequiresHost(host_call) => {
                return Ok(ExecutionOutcome::RequiresHost(host_call))
            }
        };
        let bindings_vec = match binding_val {
            Value::Vector(v) => v,
            other => {
                return Err(RuntimeError::TypeError {
                    expected: "vector of [sym coll] pairs".into(),
                    actual: other.type_name().into(),
                    operation: "for".into(),
                })
            }
        };
        if bindings_vec.len() % 2 != 0 || bindings_vec.is_empty() {
            return Err(RuntimeError::Generic(
                "for requires an even number of binding elements [sym coll ...]".into(),
            ));
        }

        // Convert into Vec<(Symbol, Vec<Value>)>
        let mut pairs: Vec<(Symbol, Vec<Value>)> = Vec::new();
        let mut i = 0;
        while i < bindings_vec.len() {
            let sym = match &bindings_vec[i] {
                Value::Symbol(s) => s.clone(),
                other => {
                    return Err(RuntimeError::TypeError {
                        expected: "symbol".into(),
                        actual: other.type_name().into(),
                        operation: "for binding symbol".into(),
                    })
                }
            };
            let coll_val = bindings_vec[i + 1].clone();
            let items = match coll_val {
                Value::Vector(v) => v,
                other => {
                    return Err(RuntimeError::TypeError {
                        expected: "vector".into(),
                        actual: other.type_name().into(),
                        operation: "for binding collection".into(),
                    })
                }
            };
            pairs.push((sym, items));
            i += 2;
        }

        // Recursive nested iteration
        let mut out: Vec<Value> = Vec::new();
        self.for_nest(&pairs, 0, env, &args[1], &mut out)?;
        Ok(ExecutionOutcome::Complete(Value::Vector(out)))
    }

    fn eval_for(
        &self,
        for_expr: &ForExpr,
        env: &mut Environment,
    ) -> Result<ExecutionOutcome, RuntimeError> {
        // Evaluate the bindings vector to get the actual values
        let mut bindings_vec = Vec::new();
        for binding in &for_expr.bindings {
            match self.eval_expr(binding, env)? {
                ExecutionOutcome::Complete(v) => bindings_vec.push(v),
                ExecutionOutcome::RequiresHost(hc) => {
                    return Ok(ExecutionOutcome::RequiresHost(hc))
                }
                #[cfg(feature = "effect-boundary")]
                ExecutionOutcome::RequiresHost(host_call) => {
                    return Ok(ExecutionOutcome::RequiresHost(host_call))
                }
                #[cfg(feature = "effect-boundary")]
                ExecutionOutcome::RequiresHost(host_call) => {
                    return Ok(ExecutionOutcome::RequiresHost(host_call))
                }
            }
        }

        // Convert to (Symbol, Vec<Value>) pairs
        if bindings_vec.len() % 2 != 0 {
            return Err(RuntimeError::Generic(
                "for requires an even number of binding elements [sym coll ...]".into(),
            ));
        }

        let mut pairs: Vec<(Symbol, Vec<Value>)> = Vec::new();
        let mut i = 0;
        while i < bindings_vec.len() {
            let sym = match &bindings_vec[i] {
                Value::Symbol(s) => s.clone(),
                other => {
                    return Err(RuntimeError::TypeError {
                        expected: "symbol".into(),
                        actual: other.type_name().into(),
                        operation: "for binding symbol".into(),
                    })
                }
            };
            let coll_val = bindings_vec[i + 1].clone();
            let items = match coll_val {
                Value::Vector(v) => v,
                other => {
                    return Err(RuntimeError::TypeError {
                        expected: "vector".into(),
                        actual: other.type_name().into(),
                        operation: "for binding collection".into(),
                    })
                }
            };
            pairs.push((sym, items));
            i += 2;
        }

        // Recursive nested iteration
        let mut out: Vec<Value> = Vec::new();
        match self.for_nest(&pairs, 0, env, &for_expr.body, &mut out)? {
            ExecutionOutcome::Complete(_) => Ok(ExecutionOutcome::Complete(Value::Vector(out))),
            ExecutionOutcome::RequiresHost(hc) => Ok(ExecutionOutcome::RequiresHost(hc)),
            #[cfg(feature = "effect-boundary")]
            ExecutionOutcome::RequiresHost(host_call) => {
                Ok(ExecutionOutcome::RequiresHost(host_call))
            }
            #[cfg(feature = "effect-boundary")]
            ExecutionOutcome::RequiresHost(host_call) => {
                Ok(ExecutionOutcome::RequiresHost(host_call))
            }
        }
    }
    fn for_nest(
        &self,
        pairs: &[(Symbol, Vec<Value>)],
        depth: usize,
        env: &Environment,
        body: &Expression,
        out: &mut Vec<Value>,
    ) -> Result<ExecutionOutcome, RuntimeError> {
        if depth == pairs.len() {
            // Evaluate body in current env clone
            let mut eval_env = env.clone();
            match self.eval_expr(body, &mut eval_env)? {
                ExecutionOutcome::Complete(v) => {
                    out.push(v);
                    return Ok(ExecutionOutcome::Complete(Value::Nil));
                }
                ExecutionOutcome::RequiresHost(hc) => {
                    return Ok(ExecutionOutcome::RequiresHost(hc))
                }
                #[cfg(feature = "effect-boundary")]
                ExecutionOutcome::RequiresHost(host_call) => {
                    return Ok(ExecutionOutcome::RequiresHost(host_call))
                }
                #[cfg(feature = "effect-boundary")]
                ExecutionOutcome::RequiresHost(host_call) => {
                    return Ok(ExecutionOutcome::RequiresHost(host_call))
                }
            }
        }
        let (sym, items) = &pairs[depth];
        for it in items.clone() {
            let mut loop_env = Environment::with_parent(Arc::new(env.clone()));
            loop_env.define(sym, it);
            match self.for_nest(pairs, depth + 1, &loop_env, body, out)? {
                ExecutionOutcome::Complete(_) => (),
                ExecutionOutcome::RequiresHost(hc) => {
                    return Ok(ExecutionOutcome::RequiresHost(hc))
                }
                #[cfg(feature = "effect-boundary")]
                ExecutionOutcome::RequiresHost(host_call) => {
                    return Ok(ExecutionOutcome::RequiresHost(host_call))
                }
                #[cfg(feature = "effect-boundary")]
                ExecutionOutcome::RequiresHost(host_call) => {
                    return Ok(ExecutionOutcome::RequiresHost(host_call))
                }
            }
        }
        Ok(ExecutionOutcome::Complete(Value::Nil))
    }

    /// Evaluate with-resource special form: (with-resource [name type init] body)
    fn eval_match_form(
        &self,
        args: &[Expression],
        env: &mut Environment,
    ) -> Result<ExecutionOutcome, RuntimeError> {
        if args.len() < 3 {
            return Err(RuntimeError::ArityMismatch {
                function: "match".into(),
                expected: "at least 3".into(),
                actual: args.len(),
            });
        }

        // First argument is the value to match against
        let value_to_match = match self.eval_expr(&args[0], env)? {
            ExecutionOutcome::Complete(v) => v,
            ExecutionOutcome::RequiresHost(hc) => return Ok(ExecutionOutcome::RequiresHost(hc)),
            #[cfg(feature = "effect-boundary")]
            ExecutionOutcome::RequiresHost(host_call) => {
                return Ok(ExecutionOutcome::RequiresHost(host_call))
            }
            #[cfg(feature = "effect-boundary")]
            ExecutionOutcome::RequiresHost(host_call) => {
                return Ok(ExecutionOutcome::RequiresHost(host_call))
            }
        };

        // Remaining arguments are pattern-body pairs
        let mut i = 1;
        while i < args.len() {
            if i + 1 >= args.len() {
                return Err(RuntimeError::Generic(
                    "match: incomplete pattern-body pair".into(),
                ));
            }

            let pattern_expr = &args[i];
            let body_expr = &args[i + 1];

            // For now, we'll implement a simple pattern matching
            // This is a simplified version - full pattern matching would be much more complex
            match pattern_expr {
                Expression::Symbol(sym) if sym.0 == "_" => {
                    // Wildcard pattern - always matches
                    return self.eval_expr(body_expr, env);
                }
                Expression::Literal(lit) => {
                    // Literal pattern matching
                    let pattern_value = self.eval_literal(lit)?;
                    if value_to_match == pattern_value {
                        return self.eval_expr(body_expr, env);
                    }
                }
                Expression::Symbol(sym) => {
                    // Variable binding pattern
                    let mut clause_env = Environment::with_parent(Arc::new(env.clone()));
                    clause_env.define(sym, value_to_match.clone());
                    return self.eval_expr(body_expr, &mut clause_env);
                }
                _ => {
                    // For now, treat complex patterns as non-matching
                    // This would need to be expanded for full pattern matching support
                }
            }

            i += 2;
        }

        Err(RuntimeError::MatchError("No matching clause".to_string()))
    }

    fn eval_with_resource_special_form(
        &self,
        args: &[Expression],
        env: &mut Environment,
    ) -> Result<ExecutionOutcome, RuntimeError> {
        // Expect (with-resource [binding-vector] body)
        if args.len() != 2 {
            return Err(RuntimeError::ArityMismatch {
                function: "with-resource".to_string(),
                expected: "2".to_string(),
                actual: args.len(),
            });
        }

        // Parse binding vector [name type init]
        let binding_vec = match &args[0] {
            Expression::Vector(elements) => elements,
            _ => {
                return Err(RuntimeError::TypeError {
                    expected: "vector for binding".to_string(),
                    actual: format!("{:?}", args[0]),
                    operation: "with-resource".to_string(),
                });
            }
        };

        if binding_vec.len() != 3 {
            return Err(RuntimeError::ArityMismatch {
                function: "with-resource binding".to_string(),
                expected: "3 elements [name type init]".to_string(),
                actual: binding_vec.len(),
            });
        }

        // Extract variable name
        let var_name = match &binding_vec[0] {
            Expression::Symbol(s) => s.clone(),
            _ => {
                return Err(RuntimeError::TypeError {
                    expected: "symbol for variable name".to_string(),
                    actual: format!("{:?}", binding_vec[0]),
                    operation: "with-resource binding name".to_string(),
                });
            }
        };

        // Evaluate the initialization expression
        let init_val = match self.eval_expr(&binding_vec[2], env)? {
            ExecutionOutcome::Complete(v) => v,
            ExecutionOutcome::RequiresHost(hc) => return Ok(ExecutionOutcome::RequiresHost(hc)),
            #[cfg(feature = "effect-boundary")]
            ExecutionOutcome::RequiresHost(host_call) => {
                return Ok(ExecutionOutcome::RequiresHost(host_call))
            }
            #[cfg(feature = "effect-boundary")]
            ExecutionOutcome::RequiresHost(host_call) => {
                return Ok(ExecutionOutcome::RequiresHost(host_call))
            }
        };

        // Create a new environment scope with the variable bound
        let mut resource_env = Environment::with_parent(Arc::new(env.clone()));
        resource_env.define(&var_name, init_val);

        // Evaluate the body in the new scope
        match self.eval_expr(&args[1], &mut resource_env)? {
            ExecutionOutcome::Complete(v) => Ok(ExecutionOutcome::Complete(v)),
            ExecutionOutcome::RequiresHost(hc) => Ok(ExecutionOutcome::RequiresHost(hc)),
            #[cfg(feature = "effect-boundary")]
            ExecutionOutcome::RequiresHost(host_call) => {
                Ok(ExecutionOutcome::RequiresHost(host_call))
            }
            #[cfg(feature = "effect-boundary")]
            ExecutionOutcome::RequiresHost(host_call) => {
                Ok(ExecutionOutcome::RequiresHost(host_call))
            }
        }
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
                // Define both the pattern symbol and the optional binding (if provided)
                env.define(s, value.clone());
                if let Some(b) = binding {
                    // Avoid double-define if same symbol name; Environment::define typically overwrites which is fine
                    env.define(b, value.clone());
                }
                Ok(true)
            }
            CatchPattern::Wildcard => {
                if let Some(b) = binding {
                    env.define(b, value.clone());
                }
                Ok(true)
            }
            CatchPattern::Keyword(k) => {
                let matches = Value::Keyword(k.clone()) == *value;
                if matches {
                    if let Some(b) = binding {
                        env.define(b, value.clone());
                    }
                }
                Ok(matches)
            }
            CatchPattern::Type(_t) => {
                // This is a placeholder implementation. A real implementation would need to
                // check the type of the value against the type expression t. For now, it always matches.
                if let Some(b) = binding {
                    env.define(b, value.clone());
                }
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
    ) -> Result<ExecutionOutcome, RuntimeError> {
        // TODO: Implement agent discovery
        Ok(ExecutionOutcome::Complete(Value::Vector(vec![])))
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
                                query.limit = Some(*i as usize);
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
                                // Store cache policy as string (SimpleDiscoveryOptions expects Option<String>)
                                options.cache_policy = Some(policy.clone());
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
                                options.max_results = Some(*max as usize);
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

        // Add agent ID (primary id)
        map.insert(
            crate::ast::MapKey::Keyword(crate::ast::Keyword("agent-id".to_string())),
            Value::String(agent_card.id.clone()),
        );

        // Add agent-id field if present (different from primary id)
        if let Some(agent_id) = &agent_card.agent_id {
            map.insert(
                crate::ast::MapKey::Keyword(crate::ast::Keyword("agent-id-field".to_string())),
                Value::String(agent_id.clone()),
            );
        }

        // Add name
        map.insert(
            crate::ast::MapKey::Keyword(crate::ast::Keyword("name".to_string())),
            Value::String(agent_card.name.clone()),
        );

        // Add version if present
        if let Some(version) = &agent_card.version {
            map.insert(
                crate::ast::MapKey::Keyword(crate::ast::Keyword("version".to_string())),
                Value::String(version.clone()),
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
        if let Some(endpoint) = &agent_card.endpoint {
            map.insert(
                crate::ast::MapKey::Keyword(crate::ast::Keyword("endpoint".to_string())),
                Value::String(endpoint.clone()),
            );
        }

        // Add metadata if present (convert HashMap to Value::Map)
        if let Some(metadata) = &agent_card.metadata {
            let metadata_map: std::collections::HashMap<_, _> = metadata
                .iter()
                .map(|(k, v)| {
                    (
                        crate::ast::MapKey::String(k.clone()),
                        Value::String(v.clone()),
                    )
                })
                .collect();
            map.insert(
                crate::ast::MapKey::Keyword(crate::ast::Keyword("metadata".to_string())),
                Value::Map(metadata_map),
            );
        }

        Value::Map(map)
    }

    /// Helper function to parse capabilities list from a value
    fn parse_capabilities_list(&self, value: &Value) -> RuntimeResult<Vec<String>> {
        match value {
            Value::Vector(vec) => {
                let mut out = Vec::new();
                for v in vec {
                    match v {
                        Value::String(s) => out.push(s.clone()),
                        other => {
                            return Err(RuntimeError::TypeError {
                                expected: "vector of strings".into(),
                                actual: other.type_name().into(),
                                operation: "parsing capabilities list".into(),
                            })
                        }
                    }
                }
                Ok(out)
            }
            Value::String(s) => Ok(vec![s.clone()]),
            Value::Keyword(k) => Ok(vec![k.0.clone()]),
            other => Err(RuntimeError::TypeError {
                expected: "vector or string".into(),
                actual: other.type_name().into(),
                operation: "parsing capabilities list".into(),
            }),
        }
    }
    /// Helper to extract a string from a Value (String or Keyword)
    fn parse_string_value(&self, value: &Value) -> RuntimeResult<String> {
        match value {
            Value::String(s) => Ok(s.clone()),
            Value::Keyword(k) => Ok(k.0.clone()),
            other => Err(RuntimeError::TypeError {
                expected: "string or keyword".to_string(),
                actual: other.type_name().to_string(),
                operation: "parse_string_value".to_string(),
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
                                    // Internally, keywords are stored WITHOUT the leading ':'.
                                    // The pattern symbols are plain identifiers (e.g., key1). If a ':' is
                                    // present for any reason, strip it to normalize.
                                    let normalized = if symbol.0.starts_with(":") {
                                        symbol.0.trim_start_matches(':').to_string()
                                    } else {
                                        symbol.0.clone()
                                    };
                                    let key = crate::ast::MapKey::Keyword(crate::ast::Keyword(
                                        normalized,
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

    fn handle_map_with_user_functions(
        &self,
        function: &Value,
        collection: &Value,
        env: &mut Environment,
    ) -> Result<ExecutionOutcome, RuntimeError> {
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
                    match self.call_function(
                        Value::Function(Function::Closure(closure.clone())),
                        &func_args,
                        env,
                    )? {
                        ExecutionOutcome::Complete(v) => result.push(v),
                        ExecutionOutcome::RequiresHost(hc) => {
                            return Ok(ExecutionOutcome::RequiresHost(hc))
                        }
                        #[cfg(feature = "effect-boundary")]
                        ExecutionOutcome::RequiresHost(host_call) => {
                            return Ok(ExecutionOutcome::RequiresHost(host_call))
                        }
                        #[cfg(feature = "effect-boundary")]
                        ExecutionOutcome::RequiresHost(host_call) => {
                            return Ok(ExecutionOutcome::RequiresHost(host_call))
                        }
                    }
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
        Ok(ExecutionOutcome::Complete(Value::Vector(result)))
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

    /// Gets a value from the current execution context with cross-plan fallback
    pub fn get_context_value(&self, key: &str) -> Option<Value> {
        self.get_with_cross_plan_fallback(key)
    }

    /// Sets a step-scoped context value (delegates to host)
    pub fn set_context_value(&self, key: String, value: Value) -> RuntimeResult<()> {
        self.host.set_step_context_value(key, value)
    }

    /// Gets the current context depth (stub - ContextManager removed)
    /// Context depth is managed by the host if it supports hierarchical context.
    pub fn context_depth(&self) -> usize {
        0 // ContextManager removed - depth now managed by host
    }

    /// Gets the current context ID (stub - ContextManager removed)
    /// Context ID is managed by the host if it supports hierarchical context.
    pub fn current_context_id(&self) -> Option<String> {
        None // ContextManager removed - ID now managed by host
    }

    /// Create a new evaluator with updated security context
    pub fn with_security_context(&self, security_context: RuntimeContext) -> Self {
        Self {
            module_registry: Arc::clone(&self.module_registry),
            env: self.env.clone(),
            recursion_depth: 0,
            max_recursion_depth: self.max_recursion_depth,
            security_context,
            host: self.host.clone(),
            special_forms: Self::default_special_forms(),
            type_validator: self.type_validator.clone(),
            type_config: self.type_config.clone(),
        }
    }

    pub fn with_environment(
        module_registry: Arc<ModuleRegistry>,
        env: Environment,
        security_context: RuntimeContext,
        host: Arc<dyn HostInterface>,
    ) -> Self {
        Self {
            module_registry,
            env,
            recursion_depth: 0,
            max_recursion_depth: 50,
            security_context,
            host,
            special_forms: Self::default_special_forms(),
            type_validator: Arc::new(TypeValidator::new()),
            type_config: TypeCheckingConfig::default(),
        }
    }
}

impl Default for Evaluator {
    fn default() -> Self {
        let module_registry = Arc::new(ModuleRegistry::new());
        let security_context = RuntimeContext::pure();

        // Create a minimal host interface for default case
        // This should be replaced with a proper host in production
        // CCOS dependencies removed - using pure_host instead
        // Use pure host for standalone RTFS (no CCOS dependencies)
        use crate::runtime::pure_host::create_pure_host;
        let host = create_pure_host();

        Self::new(module_registry, security_context, host)
    }
}
