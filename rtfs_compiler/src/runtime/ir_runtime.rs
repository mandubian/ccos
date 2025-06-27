// IR Runtime - Efficient execution engine for typed RTFS IR
// This runtime leverages type information and pre-resolved bindings for performance

use super::environment::IrEnvironment;
use super::error::RuntimeError;
use super::module_runtime::ModuleRegistry;
use super::values::{Function, Value};
use crate::ast::Expression;
// use crate::ir::IrNode;
use crate::ir::core::IrNode;
use crate::ir::converter::IrConverter;
use crate::runtime::RuntimeStrategy;

/// A `RuntimeStrategy` that uses the `IrRuntime`.
/// It owns both the runtime and the module registry, breaking the dependency cycle.
#[derive(Clone, Debug)]
pub struct IrStrategy {
    runtime: IrRuntime,
    module_registry: ModuleRegistry,
}

impl IrStrategy {
    pub fn new(module_registry: ModuleRegistry) -> Self {
        Self {
            runtime: IrRuntime::new(),
            module_registry,
        }
    }
}

impl RuntimeStrategy for IrStrategy {
    fn run(&mut self, program: &Expression) -> Result<Value, RuntimeError> {
        let mut converter = IrConverter::new();
        let ir_node = converter
            .convert_expression(program.clone())
            .map_err(|e| RuntimeError::Generic(format!("IR conversion error: {:?}", e)))?;
        self.runtime
            .execute_program(&ir_node, &mut self.module_registry)
    }

    fn clone_box(&self) -> Box<dyn RuntimeStrategy> {
        Box::new(self.clone())
    }
}

/// The Intermediate Representation (IR) runtime.
/// Executes a program represented in IR form.
#[derive(Default, Clone, Debug)]
pub struct IrRuntime {
    // This runtime is now stateless.
    // The ModuleRegistry is managed by the IrStrategy.
}

impl IrRuntime {
    /// Creates a new IR runtime.
    pub fn new() -> Self {
        IrRuntime::default()
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
                let value_to_assign =
                    self.execute_node(init_expr, env, false, module_registry)?;
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
            IrNode::QualifiedSymbolRef {
                module, symbol, ..
            } => {
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
            IrNode::Module { .. } => Ok(Value::Nil),
            IrNode::Do { expressions, .. } => {
                let mut result = Value::Nil;
                for expr in expressions {
                    result = self.execute_node(expr, env, false, module_registry)?;
                }
                Ok(result)
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
        _env: &mut IrEnvironment,
        _is_tail_call: bool,
        module_registry: &mut ModuleRegistry,
    ) -> Result<Value, RuntimeError> {
        match function {
            Value::Function(f) => match f {
                Function::Native(native_fn) => (native_fn.func)(args.to_vec()),
                Function::Ir(ir_fn) => {
                    let param_names: Vec<String> = ir_fn
                        .params
                        .iter()
                        .map(|p| match p {
                            IrNode::VariableBinding { name, .. } => Ok(name.clone()),
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
            },
            _ => Err(RuntimeError::Generic(format!(
                "Not a function: {}",
                function.to_string()
            ))),
        }
    }
}
