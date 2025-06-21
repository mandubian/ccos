// Runtime system for RTFS
// This module contains the evaluator, standard library, and runtime value system

pub mod evaluator;
pub mod stdlib;
pub mod values;
pub mod environment;
pub mod error;
pub mod ir_runtime;
pub mod module_runtime;

pub use evaluator::Evaluator;
pub use values::Value;
pub use environment::Environment;
pub use error::{RuntimeError, RuntimeResult};
use crate::ir_converter;
use crate::ast::{Expression, TopLevel, Literal};
use crate::parser::parse;
use crate::runtime::module_runtime::ModuleRegistry;

/// Runtime execution strategy
#[derive(Debug, Clone)]
pub enum RuntimeStrategy {
    /// Use AST-based evaluator (stable, compatible)
    Ast,
    /// Use IR-based runtime (high performance)
    Ir,
    /// Use IR with AST fallback for unsupported features
    IrWithFallback,
}

impl Default for RuntimeStrategy {
    fn default() -> Self {
        RuntimeStrategy::Ast // Keep AST as default for now
    }
}

/// Main runtime coordinator that can switch between AST and IR execution
pub struct Runtime<'a> {
    strategy: RuntimeStrategy,
    ast_evaluator: evaluator::Evaluator,
    ir_runtime: Option<ir_runtime::IrRuntime>,
    converter: ir_converter::IrConverter<'a>,
    // Persistent environment for REPL usage
    persistent_env: Environment,
    module_registry: &'a ModuleRegistry,
}

impl<'a> Runtime<'a> {
    pub fn new(module_registry: &'a ModuleRegistry) -> Self {
        let ast_evaluator = evaluator::Evaluator::new();
        let persistent_env = stdlib::StandardLibrary::create_global_environment();
        Self {
            strategy: RuntimeStrategy::default(),
            ast_evaluator,
            ir_runtime: Some(ir_runtime::IrRuntime::new()),
            converter: ir_converter::IrConverter::with_module_registry(module_registry),
            persistent_env,
            module_registry,
        }
    }
    
    pub fn with_strategy(strategy: RuntimeStrategy, module_registry: &'a ModuleRegistry) -> Self {
        let ast_evaluator = evaluator::Evaluator::new();
        let persistent_env = stdlib::StandardLibrary::create_global_environment();
        Self {
            strategy,
            ast_evaluator,
            ir_runtime: Some(ir_runtime::IrRuntime::new()),
            converter: ir_converter::IrConverter::with_module_registry(module_registry),
            persistent_env,
            module_registry,
        }
    }

    pub fn get_module_registry(&self) -> &'a ModuleRegistry {
        self.module_registry
    }

    pub fn set_task_context(&mut self, context: Value) {
        self.ast_evaluator.set_task_context(context.clone());
        if let Some(ir_runtime) = &mut self.ir_runtime {
            // ir_runtime.set_task_context(context); // TODO: Implement in IrRuntime
        }
    }

    pub fn with_strategy_and_agent_discovery(
        strategy: RuntimeStrategy,
        agent_discovery: Box<dyn crate::agent::AgentDiscovery>,
        module_registry: &'a ModuleRegistry,
    ) -> Self {
        let ast_evaluator = evaluator::Evaluator::with_agent_discovery(agent_discovery);
        let persistent_env = stdlib::StandardLibrary::create_global_environment();
        Self {
            strategy,
            ast_evaluator,
            ir_runtime: Some(ir_runtime::IrRuntime::new()),
            converter: ir_converter::IrConverter::with_module_registry(module_registry),
            persistent_env,
            module_registry,
        }
    }

    pub fn evaluate_expression(&mut self, expr: &crate::ast::Expression) -> RuntimeResult<Value> {
        match self.strategy {
            RuntimeStrategy::Ast => {
                self.ast_evaluator.evaluate_with_env(expr, &mut self.persistent_env.clone())
            }
            RuntimeStrategy::Ir => {
                let ir_node = self.converter.convert_expression(expr.clone())?;
                if let Some(ir_runtime) = &mut self.ir_runtime {
                    let mut env = environment::IrEnvironment::new();
                    ir_runtime.execute_node(&ir_node, &mut env, false, self.module_registry)
                } else {
                    Err(RuntimeError::InternalError("IR runtime not available".to_string()))
                }
            }
            RuntimeStrategy::IrWithFallback => {
                match self.converter.convert_expression(expr.clone()) {
                    Ok(ir_node) => {
                        if let Some(ir_runtime) = &mut self.ir_runtime {
                            let mut env = environment::IrEnvironment::new();
                            match ir_runtime.execute_node(&ir_node, &mut env, false, self.module_registry) {
                                Ok(value) => Ok(value),
                                Err(_) => self.ast_evaluator.evaluate_with_env(expr, &mut self.persistent_env.clone()), // Fallback
                            }
                        } else {
                            self.ast_evaluator.evaluate_with_env(expr, &mut self.persistent_env.clone()) // Fallback
                        }
                    }
                    Err(_) => self.ast_evaluator.evaluate_with_env(expr, &mut self.persistent_env.clone()), // Fallback
                }
            }
        }
    }
    
    /// Evaluate an IR node directly (for production compiler)
    pub fn evaluate_ir(&mut self, ir_node: &crate::ir::IrNode) -> RuntimeResult<Value> {
        if let Some(ir_runtime) = &mut self.ir_runtime {
            let mut env = environment::IrEnvironment::new();
            ir_runtime.execute_node(ir_node, &mut env, false, self.module_registry)
        } else {
            Err(RuntimeError::InternalError("IR runtime not available".to_string()))
        }
    }

    pub fn run(&mut self, input: &str, task_context: Option<Value>) -> Result<Value, RuntimeError> {
        use crate::runtime::values::Function;
        use std::cell::RefCell;
        use std::rc::Rc;

        if let Some(context) = task_context {
            self.set_task_context(context);
        }
        let mut file_env = Environment::with_parent(Rc::new(self.persistent_env.clone()));
        let top_levels = parse(input).map_err(|e| RuntimeError::InvalidProgram(e.to_string()))?;

        let mut task_plan: Option<Expression> = None;
        let mut definitions: Vec<Expression> = Vec::new();

        // 1. Separate task plan from definitions
        for top_level in top_levels {
            match top_level {
                TopLevel::Task(td) => {
                    if task_plan.is_some() {
                        return Err(RuntimeError::InvalidProgram(
                            "Multiple task definitions found.".to_string(),
                        ));
                    }
                    task_plan = td.plan;
                }
                TopLevel::Expression(expr) => {
                    let is_task = if let Expression::List(nodes) = &expr {
                        if let Some(Expression::Symbol(s)) = nodes.first() {
                            s.0 == "task"
                        } else {
                            false
                        }
                    } else {
                        false
                    };

                    if is_task {
                        if task_plan.is_some() {
                            return Err(RuntimeError::InvalidProgram(
                                "Multiple task definitions found.".to_string(),
                            ));
                        }
                        if let Expression::List(nodes) = expr {
                            // Brittle plan extraction from expression list
                            for node in nodes.iter().skip(1) {
                                if let Expression::List(prop) = node {
                                    if prop.len() >= 2 {
                                        if let Some(Expression::Literal(Literal::Keyword(k))) =
                                            prop.first()
                                        {
                                            if k.0 == "plan" {
                                                task_plan = Some(prop[1].clone());
                                                break;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    } else {
                        definitions.push(expr);
                    }
                }
                TopLevel::Module(module_def) => {
                    for m_def in module_def.definitions {
                        let expr = match m_def {
                            crate::ast::ModuleLevelDefinition::Def(d) => Expression::Def(Box::new(d)),
                            crate::ast::ModuleLevelDefinition::Defn(d) => Expression::Defn(Box::new(d)),
                            crate::ast::ModuleLevelDefinition::Import(_) => continue,
                        };
                        definitions.push(expr);
                    }
                }
            }
        }

        // 2. Process definitions with two-pass for forward references
        let mut function_defs = Vec::new();
        let mut other_defs = Vec::new();

        for def in definitions {
            let is_fn = match &def {
                Expression::Defn(_) => true,
                Expression::Def(d) => matches!(*d.value, Expression::Fn(_)),
                _ => false,
            };
            if is_fn {
                function_defs.push(def);
            } else {
                other_defs.push(def);
            }
        }

        // Pass 1: Create placeholders
        let mut placeholders = Vec::new();
        for fn_def in &function_defs {
            let name = match fn_def {
                Expression::Defn(d) => &d.name,
                Expression::Def(d) => &d.symbol,
                _ => unreachable!(),
            };
            let placeholder_cell = Rc::new(RefCell::new(Value::Nil));
            file_env.define(name, Value::FunctionPlaceholder(placeholder_cell.clone()));
            placeholders.push((fn_def.clone(), placeholder_cell));
        }

        // Evaluate non-function definitions
        for expr in &other_defs {
            self.ast_evaluator.evaluate_with_env(expr, &mut file_env)?;
        }

        // Pass 2: Evaluate and resolve functions
        for (fn_def, placeholder_cell) in placeholders {
            let function_value = match fn_def {
                Expression::Defn(defn_expr) => {
                    Value::Function(Function::UserDefined {
                        params: defn_expr.params.clone(),
                        variadic_param: defn_expr.variadic_param.clone(),
                        body: defn_expr.body.clone(),
                        closure: file_env.clone(),
                    })
                }
                Expression::Def(def_expr) => {
                    self
                        .ast_evaluator
                        .evaluate_with_env(&def_expr.value, &mut file_env)?
                }
                _ => unreachable!(),
            };
            *placeholder_cell.borrow_mut() = function_value;
        }

        // 3. Execute task plan
        if let Some(plan) = task_plan {
            self.ast_evaluator.evaluate_with_env(&plan, &mut file_env)
        } else if let Some(last_def) = other_defs.last() {
            // If no task, result of file is result of last expression
            self.ast_evaluator.evaluate_with_env(last_def, &mut file_env)
        } else {
            Ok(Value::Nil)
        }
    }
}
