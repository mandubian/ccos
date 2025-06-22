// Complete AST to IR Converter Implementation
// Provides full conversion from parsed AST to optimized IR

use std::collections::HashMap;
use std::rc::Rc;
use crate::ast::*;
use crate::ir::*;
use crate::runtime::module_runtime::ModuleRegistry;

/// Error types for IR conversion
#[derive(Debug, Clone, PartialEq)]
pub enum IrConversionError {
    UndefinedSymbol {
        symbol: String,
        location: Option<SourceLocation>,
    },
    TypeMismatch {
        expected: IrType,
        found: IrType,
        location: Option<SourceLocation>,
    },
    InvalidPattern {
        message: String,
        location: Option<SourceLocation>,
    },    InvalidTypeAnnotation {
        message: String,
        location: Option<SourceLocation>,
    },
    InvalidSpecialForm {
        form: String,
        message: String,
    },
    InternalError {
        message: String,
    },
}

pub type IrConversionResult<T> = Result<T, IrConversionError>;

/// Information about a binding in the current scope
#[derive(Debug, Clone)]
pub struct BindingInfo {
    pub name: String,
    pub binding_id: NodeId,
    pub ir_type: IrType,
    pub kind: BindingKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BindingKind {
    Variable,
    Function,
    Parameter,
    Resource,
}

/// Scope for symbol resolution with proper mutable access
#[derive(Debug, Clone)]
pub struct Scope {
    bindings: HashMap<String, BindingInfo>,
    parent: Option<Rc<Scope>>,
}

impl Scope {
    pub fn new() -> Self {
        Scope {
            bindings: HashMap::new(),
            parent: None,
        }
    }
    
    pub fn with_parent(parent: Rc<Scope>) -> Self {
        Scope {
            bindings: HashMap::new(),
            parent: Some(parent),
        }
    }
    
    pub fn define(&mut self, name: String, info: BindingInfo) {
        self.bindings.insert(name, info);
    }
    
    pub fn lookup(&self, name: &str) -> Option<&BindingInfo> {
        self.bindings.get(name).or_else(|| {
            self.parent.as_ref().and_then(|p| p.lookup(name))
        })
    }
}

/// Type inference and checking context
#[derive(Debug, Clone)]
pub struct TypeContext {
    type_aliases: HashMap<String, IrType>,
    constraints: Vec<TypeConstraint>,
}

#[derive(Debug, Clone)]
pub struct TypeConstraint {
    pub node_id: NodeId,
    pub expected: IrType,
    pub actual: IrType,
}

/// Main AST to IR converter with complete implementation
pub struct IrConverter<'a> {
    next_node_id: NodeId,
    scope_stack: Vec<HashMap<String, BindingInfo>>,
    type_context: TypeContext,
    capture_analysis: HashMap<NodeId, Vec<IrCapture>>,
    /// Optional module registry for resolving qualified symbols during conversion
    module_registry: Option<&'a ModuleRegistry>,
}

impl<'a> IrConverter<'a> {
    pub fn new() -> Self {
        let mut converter = IrConverter {
            next_node_id: 1,
            scope_stack: vec![HashMap::new()],
            type_context: TypeContext {
                type_aliases: HashMap::new(),
                constraints: Vec::new(),
            },
            capture_analysis: HashMap::new(),
            module_registry: None,
        };
        
        // Add built-in functions to global scope
        converter.add_builtin_functions();
        converter
    }
    pub fn with_module_registry(registry: &'a ModuleRegistry) -> Self {
        let mut converter = IrConverter {
            next_node_id: 1,
            scope_stack: vec![HashMap::new()],
            type_context: TypeContext {
                type_aliases: HashMap::new(),
                constraints: Vec::new(),
            },
            capture_analysis: HashMap::new(),
            module_registry: Some(registry),
        };
        
        // Add built-in functions to global scope
        converter.add_builtin_functions();
        converter
    }
    
    pub fn next_id(&mut self) -> NodeId {
        let id = self.next_node_id;
        self.next_node_id += 1;
        id
    }
      /// Add built-in functions to global scope
    fn add_builtin_functions(&mut self) {
        let builtins = [
            // Arithmetic operators
            ("+", IrType::Function {
                param_types: vec![IrType::Int, IrType::Int],
                variadic_param_type: Some(Box::new(IrType::Int)),
                return_type: Box::new(IrType::Int),
            }),
            ("-", IrType::Function {
                param_types: vec![IrType::Int, IrType::Int],
                variadic_param_type: None,
                return_type: Box::new(IrType::Int),
            }),
            ("*", IrType::Function {
                param_types: vec![IrType::Int, IrType::Int],
                variadic_param_type: Some(Box::new(IrType::Int)),
                return_type: Box::new(IrType::Int),
            }),
            ("/", IrType::Function {
                param_types: vec![IrType::Int, IrType::Int],
                variadic_param_type: None,
                return_type: Box::new(IrType::Int),
            }),
            
            // Comparison operators
            ("=", IrType::Function {
                param_types: vec![IrType::Any, IrType::Any],
                variadic_param_type: None,
                return_type: Box::new(IrType::Bool),
            }),
            ("!=", IrType::Function {
                param_types: vec![IrType::Any, IrType::Any],
                variadic_param_type: None,
                return_type: Box::new(IrType::Bool),
            }),
            (">", IrType::Function {
                param_types: vec![IrType::Any, IrType::Any],
                variadic_param_type: None,
                return_type: Box::new(IrType::Bool),
            }),
            ("<", IrType::Function {
                param_types: vec![IrType::Any, IrType::Any],
                variadic_param_type: None,
                return_type: Box::new(IrType::Bool),
            }),
            (">=", IrType::Function {
                param_types: vec![IrType::Any, IrType::Any],
                variadic_param_type: None,
                return_type: Box::new(IrType::Bool),
            }),
            ("<=", IrType::Function {
                param_types: vec![IrType::Any, IrType::Any],
                variadic_param_type: None,
                return_type: Box::new(IrType::Bool),
            }),
            
            // Logical operators
            ("and", IrType::Function {
                param_types: vec![IrType::Any],
                variadic_param_type: Some(Box::new(IrType::Any)),
                return_type: Box::new(IrType::Any),
            }),
            ("or", IrType::Function {
                param_types: vec![IrType::Any],
                variadic_param_type: Some(Box::new(IrType::Any)),
                return_type: Box::new(IrType::Any),
            }),
            ("not", IrType::Function {
                param_types: vec![IrType::Any],
                variadic_param_type: None,
                return_type: Box::new(IrType::Bool),
            }),
            
            // String functions
            ("str", IrType::Function {
                param_types: vec![IrType::Any],
                variadic_param_type: Some(Box::new(IrType::Any)),
                return_type: Box::new(IrType::String),
            }),
            ("string?", IrType::Function {
                param_types: vec![IrType::Any],
                variadic_param_type: None,
                return_type: Box::new(IrType::Bool),
            }),
            ("string-length", IrType::Function {
                param_types: vec![IrType::String],
                variadic_param_type: None,
                return_type: Box::new(IrType::Int),
            }),
            ("substring", IrType::Function {
                param_types: vec![IrType::String, IrType::Int],
                variadic_param_type: None,
                return_type: Box::new(IrType::String),
            }),
            
            // Collection functions  
            ("map", IrType::Function {
                param_types: vec![IrType::Any],
                variadic_param_type: Some(Box::new(IrType::Any)),
                return_type: Box::new(IrType::Any),
            }),
            ("vector", IrType::Function {
                param_types: vec![],
                variadic_param_type: Some(Box::new(IrType::Any)),
                return_type: Box::new(IrType::Vector(Box::new(IrType::Any))),
            }),
            ("vector?", IrType::Function {
                param_types: vec![IrType::Any],
                variadic_param_type: None,
                return_type: Box::new(IrType::Bool),
            }),
            ("map?", IrType::Function {
                param_types: vec![IrType::Any],
                variadic_param_type: None,
                return_type: Box::new(IrType::Bool),
            }),
            ("count", IrType::Function {
                param_types: vec![IrType::Any],
                variadic_param_type: None,
                return_type: Box::new(IrType::Int),
            }),
            ("get", IrType::Function {
                param_types: vec![IrType::Any, IrType::Any],
                variadic_param_type: None,
                return_type: Box::new(IrType::Any),
            }),
            ("assoc", IrType::Function {
                param_types: vec![IrType::Any, IrType::Any, IrType::Any],
                variadic_param_type: Some(Box::new(IrType::Any)),
                return_type: Box::new(IrType::Any),
            }),
            ("dissoc", IrType::Function {
                param_types: vec![IrType::Any, IrType::Any],
                variadic_param_type: Some(Box::new(IrType::Any)),
                return_type: Box::new(IrType::Any),
            }),
            ("conj", IrType::Function {
                param_types: vec![IrType::Any, IrType::Any],
                variadic_param_type: Some(Box::new(IrType::Any)),
                return_type: Box::new(IrType::Any),
            }),
            
            // Type predicate functions
            ("nil?", IrType::Function {
                param_types: vec![IrType::Any],
                variadic_param_type: None,
                return_type: Box::new(IrType::Bool),
            }),
            ("bool?", IrType::Function {
                param_types: vec![IrType::Any],
                variadic_param_type: None,
                return_type: Box::new(IrType::Bool),
            }),
            ("int?", IrType::Function {
                param_types: vec![IrType::Any],
                variadic_param_type: None,
                return_type: Box::new(IrType::Bool),
            }),
            ("float?", IrType::Function {
                param_types: vec![IrType::Any],
                variadic_param_type: None,
                return_type: Box::new(IrType::Bool),
            }),
            ("number?", IrType::Function {
                param_types: vec![IrType::Any],
                variadic_param_type: None,
                return_type: Box::new(IrType::Bool),
            }),
            ("fn?", IrType::Function {
                param_types: vec![IrType::Any],
                variadic_param_type: None,
                return_type: Box::new(IrType::Bool),
            }),
            ("symbol?", IrType::Function {
                param_types: vec![IrType::Any],
                variadic_param_type: None,
                return_type: Box::new(IrType::Bool),            }),
            ("keyword?", IrType::Function {
                param_types: vec![IrType::Any],
                variadic_param_type: None,
                return_type: Box::new(IrType::Bool),
            }),
            ("empty?", IrType::Function {
                param_types: vec![IrType::Any],
                variadic_param_type: None,
                return_type: Box::new(IrType::Bool),
            }),
            ("even?", IrType::Function {
                param_types: vec![IrType::Int],
                variadic_param_type: None,
                return_type: Box::new(IrType::Bool),
            }),
            ("odd?", IrType::Function {
                param_types: vec![IrType::Int],
                variadic_param_type: None,
                return_type: Box::new(IrType::Bool),
            }),
            ("zero?", IrType::Function {
                param_types: vec![IrType::Int],
                variadic_param_type: None,
                return_type: Box::new(IrType::Bool),
            }),
            ("pos?", IrType::Function {
                param_types: vec![IrType::Int],
                variadic_param_type: None,
                return_type: Box::new(IrType::Bool),
            }),
            ("neg?", IrType::Function {
                param_types: vec![IrType::Int],
                variadic_param_type: None,
                return_type: Box::new(IrType::Bool),
            }),
            ("inc", IrType::Function {
                param_types: vec![IrType::Int],
                variadic_param_type: None,
                return_type: Box::new(IrType::Int),
            }),
            ("dec", IrType::Function {
                param_types: vec![IrType::Int],
                variadic_param_type: None,
                return_type: Box::new(IrType::Int),
            }),            ("partition", IrType::Function {
                param_types: vec![IrType::Int, IrType::Any],
                variadic_param_type: None,
                return_type: Box::new(IrType::Vector(Box::new(IrType::Any))),
            }),
            ("first", IrType::Function {
                param_types: vec![IrType::Any],
                variadic_param_type: None,
                return_type: Box::new(IrType::Any),
            }),
            ("rest", IrType::Function {
                param_types: vec![IrType::Any],
                variadic_param_type: None,
                return_type: Box::new(IrType::Any),
            }),
            ("cons", IrType::Function {
                param_types: vec![IrType::Any, IrType::Any],
                variadic_param_type: None,
                return_type: Box::new(IrType::Any),
            }),
            ("concat", IrType::Function {
                param_types: vec![IrType::Any],
                variadic_param_type: Some(Box::new(IrType::Any)),
                return_type: Box::new(IrType::Any),
            }),
            ("nth", IrType::Function {
                param_types: vec![IrType::Any, IrType::Int],
                variadic_param_type: None,
                return_type: Box::new(IrType::Any),
            }),
            ("contains?", IrType::Function {
                param_types: vec![IrType::Any, IrType::Any],
                variadic_param_type: None,
                return_type: Box::new(IrType::Bool),
            }),
            
            // Higher-order functions
            ("map-fn", IrType::Function {
                param_types: vec![IrType::Any, IrType::Any],
                variadic_param_type: Some(Box::new(IrType::Any)),
                return_type: Box::new(IrType::Any),
            }),
            ("filter", IrType::Function {
                param_types: vec![IrType::Any, IrType::Any],
                variadic_param_type: None,
                return_type: Box::new(IrType::Any),
            }),            
            ("reduce", IrType::Function {
                param_types: vec![IrType::Any, IrType::Any],
                variadic_param_type: None,
                return_type: Box::new(IrType::Any),
            }),
            
            // Agent system functions
            ("discover-agents", IrType::Function {
                param_types: vec![IrType::Map {
                    entries: vec![],
                    wildcard: Some(Box::new(IrType::Any)),
                }],
                variadic_param_type: None,
                return_type: Box::new(IrType::Vector(Box::new(IrType::Any))),
            }),            
            ("task", IrType::Function {
                param_types: vec![IrType::Any],
                variadic_param_type: Some(Box::new(IrType::Any)),
                return_type: Box::new(IrType::Any),
            }),
              // Mathematical functions
            ("fact", IrType::Function {
                param_types: vec![IrType::Int],
                variadic_param_type: None,
                return_type: Box::new(IrType::Int),
            }),
            ("max", IrType::Function {
                param_types: vec![IrType::Any],
                variadic_param_type: Some(Box::new(IrType::Any)),
                return_type: Box::new(IrType::Any),
            }),
            ("min", IrType::Function {
                param_types: vec![IrType::Any],
                variadic_param_type: Some(Box::new(IrType::Any)),
                return_type: Box::new(IrType::Any),
            }),
            ("length", IrType::Function {
                param_types: vec![IrType::Any],
                variadic_param_type: None,
                return_type: Box::new(IrType::Int),
            }),
            
            // Agent system functions - Advanced
            ("discover-and-assess-agents", IrType::Function {
                param_types: vec![IrType::Any],
                variadic_param_type: Some(Box::new(IrType::Any)),
                return_type: Box::new(IrType::Vector(Box::new(IrType::Any))),
            }),
            ("establish-system-baseline", IrType::Function {
                param_types: vec![IrType::Any],
                variadic_param_type: Some(Box::new(IrType::Any)),
                return_type: Box::new(IrType::Map {
                    entries: vec![],
                    wildcard: Some(Box::new(IrType::Any)),
                }),
            }),
              // Tool functions for agent coordination
            ("tool:current-timestamp-ms", IrType::Function {
                param_types: vec![],
                variadic_param_type: None,
                return_type: Box::new(IrType::Int),
            }),            ("tool:log", IrType::Function {
                param_types: vec![IrType::Any],
                variadic_param_type: None,
                return_type: Box::new(IrType::Nil),
            }),
            
            // Special forms
            ("lambda", IrType::Function {
                param_types: vec![IrType::Any, IrType::Any],
                variadic_param_type: Some(Box::new(IrType::Any)),
                return_type: Box::new(IrType::Function {
                    param_types: vec![],
                    variadic_param_type: Some(Box::new(IrType::Any)),
                    return_type: Box::new(IrType::Any),
                }),
            }),
            ("quote", IrType::Function {
                param_types: vec![IrType::Any],
                variadic_param_type: None,
                return_type: Box::new(IrType::Any),
            }),
            ("letrec", IrType::Function {
                param_types: vec![IrType::Vector(Box::new(IrType::Any)), IrType::Any],
                variadic_param_type: Some(Box::new(IrType::Any)),
                return_type: Box::new(IrType::Any),
            }),
            ("if", IrType::Function {
                param_types: vec![IrType::Any, IrType::Any],
                variadic_param_type: Some(Box::new(IrType::Any)),
                return_type: Box::new(IrType::Any),
            }),
            ("let", IrType::Function {
                param_types: vec![IrType::Vector(Box::new(IrType::Any)), IrType::Any],
                variadic_param_type: Some(Box::new(IrType::Any)),
                return_type: Box::new(IrType::Any),
            }),
            ("do", IrType::Function {
                param_types: vec![IrType::Any],
                variadic_param_type: Some(Box::new(IrType::Any)),
                return_type: Box::new(IrType::Any),
            }),
            ("def", IrType::Function {
                param_types: vec![IrType::Any, IrType::Any],
                variadic_param_type: None,
                return_type: Box::new(IrType::Any),
            }),
            ("defn", IrType::Function {
                param_types: vec![IrType::Any, IrType::Vector(Box::new(IrType::Any)), IrType::Any],
                variadic_param_type: Some(Box::new(IrType::Any)),
                return_type: Box::new(IrType::Any),
            }),
        ];
        
        for (name, func_type) in builtins {
            let binding_info = BindingInfo {
                name: name.to_string(),
                binding_id: self.next_id(),
                ir_type: func_type,
                kind: BindingKind::Function,
            };
            self.scope_stack[0].insert(name.to_string(), binding_info);
        }
    }
    
    /// Enter a new scope
    fn enter_scope(&mut self) {
        self.scope_stack.push(HashMap::new());
    }
    
    /// Exit the current scope
    fn exit_scope(&mut self) {
        self.scope_stack.pop();
    }
    
    /// Define a binding in the current scope
    pub fn define_binding(&mut self, name: String, info: BindingInfo) {
        if let Some(current_scope) = self.scope_stack.last_mut() {
            current_scope.insert(name, info);
        }
    }
    
    /// Update an existing binding in the current scope
    pub fn update_binding(&mut self, name: String, info: BindingInfo) {
        if let Some(current_scope) = self.scope_stack.last_mut() {
            current_scope.insert(name, info);
        }
    }
    
    /// Look up a symbol in the scope stack
    pub fn lookup_symbol(&self, name: &str) -> Option<&BindingInfo> {
        for scope in self.scope_stack.iter().rev() {
            if let Some(binding) = scope.get(name) {
                return Some(binding);
            }
        }
        None
    }
    
    /// Convert a simple expression (main entry point)
    pub fn convert_expression(&mut self, expr: Expression) -> IrConversionResult<IrNode> {
        match expr {
            Expression::Literal(lit) => self.convert_literal(lit),
            Expression::Symbol(sym) => self.convert_symbol_ref(sym),
            Expression::FunctionCall { callee, arguments } => {
                self.convert_function_call(*callee, arguments)
            }
            Expression::If(if_expr) => self.convert_if(if_expr),            Expression::Let(let_expr) => self.convert_let(let_expr),
            Expression::Letrec(let_expr) => self.convert_letrec(let_expr),
            Expression::Do(do_expr) => self.convert_do(do_expr),
            Expression::Fn(fn_expr) => self.convert_fn(fn_expr),
            Expression::Match(match_expr) => self.convert_match(match_expr),
            Expression::Vector(exprs) => self.convert_vector(exprs),
            Expression::Map(map) => self.convert_map(map),
            Expression::List(exprs) => self.convert_list_as_application(exprs),
            Expression::TryCatch(try_expr) => self.convert_try_catch(try_expr),
            Expression::Parallel(parallel_expr) => self.convert_parallel(parallel_expr),
            Expression::WithResource(with_expr) => self.convert_with_resource(with_expr),
            Expression::LogStep(log_expr) => self.convert_log_step(*log_expr),
            Expression::DiscoverAgents(discover_expr) => self.convert_discover_agents(discover_expr),
            Expression::Def(def_expr) => self.convert_def(*def_expr),
            Expression::Defn(defn_expr) => self.convert_defn(*defn_expr),
        }
    }
    
    /// High-level conversion method (entry point)
    pub fn convert(&mut self, expr: &Expression) -> IrConversionResult<IrNode> {
        self.convert_expression(expr.clone())
    }
    
    /// Convert a literal value
    fn convert_literal(&mut self, lit: Literal) -> IrConversionResult<IrNode> {
        let id = self.next_id();
        let ir_type = match &lit {
            Literal::Integer(_) => IrType::Int,
            Literal::Float(_) => IrType::Float,
            Literal::String(_) => IrType::String,
            Literal::Boolean(_) => IrType::Bool,
            Literal::Keyword(_) => IrType::Keyword,
            Literal::Nil => IrType::Nil,
        };
        
        Ok(IrNode::Literal {
            id,
            value: lit,
            ir_type,
            source_location: None,
        })
    }
    
    /// Convert symbol reference (variable lookup)
    fn convert_symbol_ref(&mut self, sym: Symbol) -> IrConversionResult<IrNode> {
        let id = self.next_id();
        let name = sym.0.clone();
        
        // Handle qualified symbols (module/symbol syntax)
        // Only treat as qualified if '/' is not at the beginning or end and there's actual content on both sides
        if let Some(index) = name.find('/') {
            if index > 0 && index < name.len() - 1 {
                let module_name = &name[..index];
                let symbol_name = &name[index+1..];

                if let Some(registry) = self.module_registry {
                    if let Some(module) = registry.get_module(module_name) {
                        // Check exported functions/values
                        if let Some(export) = module.exports.borrow().get(symbol_name) {
                            return Ok(IrNode::QualifiedSymbolRef {
                                id,
                                module: module_name.to_string(),
                                symbol: symbol_name.to_string(),
                                ir_type: export.ir_type.clone(),
                                source_location: None, // TODO: Add source location
                            });
                        }
                    }
                }
                
                // If not found in module registry, it's an error
                return Err(IrConversionError::UndefinedSymbol {
                    symbol: name,
                    location: None, // TODO: Add source location
                });
            }
        }
        
        // Look up the symbol in current scope
        match self.lookup_symbol(&name) {
            Some(binding_info) => {
                Ok(IrNode::VariableRef {
                    id,
                    name,
                    binding_id: binding_info.binding_id,
                    ir_type: binding_info.ir_type.clone(),
                    source_location: None,
                })
            }
            None => {
                Err(IrConversionError::UndefinedSymbol {
                    symbol: name,
                    location: None,
                })
            }
        }
    }

    /// Convert function call
    fn convert_function_call(&mut self, callee: Expression, arguments: Vec<Expression>) -> IrConversionResult<IrNode> {
        // Check for special forms first
        if let Expression::Symbol(Symbol(ref name)) = callee {
            match name.as_str() {
                "lambda" => {
                    return self.convert_lambda_special_form(arguments);
                }
                _ => {}
            }
        }
        
        let id = self.next_id();
        let function = Box::new(self.convert_expression(callee)?);
        let mut ir_arguments = Vec::new();
        
        for arg in arguments {
            ir_arguments.push(self.convert_expression(arg)?);
        }
        
        // Infer return type from function type
        let return_type = match function.ir_type() {
            Some(IrType::Function { return_type, .. }) => (**return_type).clone(),
            _ => IrType::Any,
        };

        Ok(IrNode::Apply {
            id,
            function,
            arguments: ir_arguments,
            ir_type: return_type,
            source_location: None,
        })
    }

    /// Convert lambda special form: (lambda [params] body...)
    fn convert_lambda_special_form(&mut self, arguments: Vec<Expression>) -> IrConversionResult<IrNode> {
        if arguments.len() < 2 {
            return Err(IrConversionError::InvalidSpecialForm {
                form: "lambda".to_string(),
                message: "lambda requires at least 2 arguments: parameter list and body".to_string(),
            });
        }
        
        let id = self.next_id();
        
        // Enter new scope for lambda body
        self.enter_scope();
        
        // Parse parameter list (first argument)
        let param_list = &arguments[0];
        let mut params = Vec::new();
        
        if let Expression::Vector(elements) = param_list {
            for element in elements {
                if let Expression::Symbol(Symbol(param_name)) = element {
                    let param_id = self.next_id();
                    let param_type = IrType::Any; // Lambda parameters are untyped
                    
                    // Add parameter to scope
                    let binding_info = BindingInfo {
                        name: param_name.clone(),
                        binding_id: param_id,
                        ir_type: param_type.clone(),
                        kind: BindingKind::Parameter,
                    };
                    self.define_binding(param_name.clone(), binding_info);
                    
                    // Create parameter node
                    params.push(IrNode::Param {
                        id: param_id,
                        binding: Box::new(IrNode::VariableBinding {
                            id: param_id,
                            name: param_name.clone(),
                            ir_type: param_type.clone(),
                            source_location: None,
                        }),
                        type_annotation: Some(param_type.clone()),
                        ir_type: param_type,
                        source_location: None,
                    });
                } else {
                    return Err(IrConversionError::InvalidSpecialForm {
                        form: "lambda".to_string(),
                        message: "lambda parameters must be symbols".to_string(),
                    });
                }
            }
        } else {
            return Err(IrConversionError::InvalidSpecialForm {
                form: "lambda".to_string(),
                message: "lambda first argument must be a vector of parameters".to_string(),
            });
        }
        
        // Convert body expressions (remaining arguments)
        let mut body_exprs = Vec::new();
        for body_expr in &arguments[1..] {
            body_exprs.push(self.convert_expression(body_expr.clone())?);
        }
        
        // Exit lambda scope
        self.exit_scope();
        
        // Determine return type from last body expression
        let return_type = body_exprs.last()
            .and_then(|expr| expr.ir_type())
            .cloned()
            .unwrap_or(IrType::Any);
        
        // Build function type
        let param_types: Vec<IrType> = params.iter()
            .filter_map(|p| p.ir_type())
            .cloned()
            .collect();
        
        let function_type = IrType::Function {
            param_types,
            variadic_param_type: None,
            return_type: Box::new(return_type),
        };

        Ok(IrNode::Lambda {
            id,
            params,
            variadic_param: None,
            body: body_exprs,
            captures: Vec::new(), // TODO: Implement capture analysis
            ir_type: function_type,
            source_location: None,
        })
    }
    
    /// Convert if expression
    fn convert_if(&mut self, if_expr: IfExpr) -> IrConversionResult<IrNode> {
        let id = self.next_id();
        let condition = Box::new(self.convert_expression(*if_expr.condition)?);
        let then_branch = Box::new(self.convert_expression(*if_expr.then_branch)?);
        let else_branch = if let Some(else_expr) = if_expr.else_branch {
            Some(Box::new(self.convert_expression(*else_expr)?))
        } else {
            None
        };
        
        // Infer result type as union of branches
        let result_type = match (then_branch.ir_type(), else_branch.as_ref().and_then(|e| e.ir_type())) {
            (Some(then_type), Some(else_type)) if then_type == else_type => then_type.clone(),
            (Some(then_type), Some(else_type)) => IrType::Union(vec![then_type.clone(), else_type.clone()]),
            (Some(then_type), None) => IrType::Union(vec![then_type.clone(), IrType::Nil]),
            _ => IrType::Any,
        };
        
        Ok(IrNode::If {
            id,
            condition,
            then_branch,
            else_branch,
            ir_type: result_type,
            source_location: None,
        })
    }
    
    /// Convert let expression with proper scope management
    fn convert_let(&mut self, let_expr: LetExpr) -> IrConversionResult<IrNode> {
        let id = self.next_id();
        let mut bindings = Vec::new();
        
        // Enter new scope for let bindings
        self.enter_scope();
        
        // Two-pass approach for recursive function bindings (similar to letrec)
        let mut function_bindings = Vec::new();
        let mut other_bindings = Vec::new();
        
        // Pass 1: Identify function bindings and create placeholders
        for binding in let_expr.bindings {
            if let Pattern::Symbol(symbol) = &binding.pattern {
                if matches!(*binding.value, Expression::Fn(_)) {
                    let binding_id = self.next_id();
                    
                    // Create placeholder binding info for the function
                    let binding_info = BindingInfo {
                        name: symbol.0.clone(),
                        binding_id,
                        ir_type: IrType::Function {
                            param_types: vec![IrType::Any], // Will be refined later
                            variadic_param_type: None,
                            return_type: Box::new(IrType::Any),
                        },
                        kind: BindingKind::Function,
                    };
                    
                    // Add placeholder to scope immediately
                    self.define_binding(symbol.0.clone(), binding_info);
                    function_bindings.push((binding, binding_id));
                } else {
                    other_bindings.push(binding);
                }
            } else {
                other_bindings.push(binding);
            }
        }
        
        // Pass 2: Process function bindings with placeholders in scope
        for (binding, binding_id) in function_bindings {
            let init_expr = self.convert_expression(*binding.value)?;
            let binding_type = init_expr.ir_type().cloned().unwrap_or(IrType::Any);
            let pattern_node = self.convert_pattern(binding.pattern, binding_id, binding_type.clone())?;
            
            bindings.push(IrLetBinding {
                pattern: pattern_node,
                type_annotation: binding.type_annotation.map(|t| self.convert_type_annotation(t)).transpose()?,
                init_expr,
            });
        }
        
        // Process non-function bindings sequentially
        for binding in other_bindings {
            let binding_id = self.next_id();
            let pattern_clone = binding.pattern.clone();
            let init_expr = self.convert_expression(*binding.value)?;
            let binding_type = init_expr.ir_type().cloned().unwrap_or(IrType::Any);
            let pattern_node = self.convert_pattern(binding.pattern, binding_id, binding_type.clone())?;
            
            // Add non-function binding to scope after converting init expression
            if let Pattern::Symbol(sym) = &pattern_clone {
                let binding_info = BindingInfo {
                    name: sym.0.clone(),
                    binding_id,
                    ir_type: binding_type.clone(),
                    kind: BindingKind::Variable,
                };
                self.define_binding(sym.0.clone(), binding_info);
            }
            
            bindings.push(IrLetBinding {
                pattern: pattern_node,
                type_annotation: binding.type_annotation.map(|t| self.convert_type_annotation(t)).transpose()?,
                init_expr,
            });
        }
        
        // Convert body expressions in the new scope
        let mut body_exprs = Vec::new();
        for body_expr in let_expr.body {
            body_exprs.push(self.convert_expression(body_expr)?);
        }
        
        // Exit scope
        self.exit_scope();
        
        // Infer result type from last body expression
        let result_type = body_exprs.last()
            .and_then(|expr| expr.ir_type())
            .cloned()
            .unwrap_or(IrType::Nil);
        
        Ok(IrNode::Let {
            id,
            bindings,
            body: body_exprs,
            ir_type: result_type,
            source_location: None,
        })
    }

    /// Convert letrec expression with proper recursive binding support
    fn convert_letrec(&mut self, let_expr: LetExpr) -> IrConversionResult<IrNode> {
        let id = self.next_id();
        let mut bindings = Vec::new();
        
        // Enter new scope for letrec bindings
        self.enter_scope();
        
        // For letrec, all bindings are available during the evaluation of all init expressions
        // First pass: Create placeholder bindings for all symbols
        let mut binding_infos = Vec::new();
        for binding in &let_expr.bindings {
            if let Pattern::Symbol(symbol) = &binding.pattern {
                let binding_id = self.next_id();                let binding_info = BindingInfo {
                    name: symbol.0.clone(),
                    binding_id,
                    ir_type: IrType::Any, // Will be refined after processing init expressions
                    kind: if matches!(*binding.value, Expression::Fn(_)) {
                        BindingKind::Function
                    } else {
                        BindingKind::Variable
                    },
                };
                
                // Add placeholder to scope immediately so it's available during init conversion
                self.define_binding(symbol.0.clone(), binding_info.clone());
                binding_infos.push((binding, binding_info));
            } else {                return Err(IrConversionError::InvalidPattern {
                    message: "letrec currently only supports simple symbol bindings".to_string(),
                    location: None,
                });
            }
        }
        
        // Second pass: Convert all init expressions with all placeholders in scope
        for (binding, binding_info) in binding_infos {
            let init_expr = self.convert_expression(*binding.value.clone())?;
            let binding_type = init_expr.ir_type().cloned().unwrap_or(IrType::Any);
            let pattern_node = self.convert_pattern(binding.pattern.clone(), binding_info.binding_id, binding_type.clone())?;
            
            // Update the binding info with the refined type
            if let Pattern::Symbol(sym) = &binding.pattern {
                let updated_binding_info = BindingInfo {
                    name: sym.0.clone(),
                    binding_id: binding_info.binding_id,
                    ir_type: binding_type.clone(),
                    kind: binding_info.kind,
                };
                self.update_binding(sym.0.clone(), updated_binding_info);
            }
            
            bindings.push(IrLetBinding {
                pattern: pattern_node,
                type_annotation: binding.type_annotation.clone().map(|t| self.convert_type_annotation(t)).transpose()?,
                init_expr,
            });
        }
        
        // Convert body expressions in the scope with all bindings available
        let mut body_exprs = Vec::new();
        for body_expr in let_expr.body {
            body_exprs.push(self.convert_expression(body_expr)?);
        }
        
        // Exit scope
        self.exit_scope();
        
        // Infer result type from last body expression
        let result_type = body_exprs.last()
            .and_then(|expr| expr.ir_type())
            .cloned()
            .unwrap_or(IrType::Nil);
        
        Ok(IrNode::Letrec {
            id,
            bindings,
            body: body_exprs,
            ir_type: result_type,
            source_location: None,
        })
    }

    fn convert_do(&mut self, do_expr: DoExpr) -> IrConversionResult<IrNode> {
        let id = self.next_id();
        let mut expressions = Vec::new();
        for expr in do_expr.expressions {
            expressions.push(self.convert_expression(expr)?);
        }
        let ir_type = expressions.last().and_then(|n| n.ir_type()).cloned().unwrap_or(IrType::Nil);
        Ok(IrNode::Do { id, expressions, ir_type, source_location: None })
    }

    fn convert_fn(&mut self, fn_expr: FnExpr) -> IrConversionResult<IrNode> {
        let id = self.next_id();
        self.enter_scope();
        let mut params = Vec::new();
        for p_def in fn_expr.params {
            if let Pattern::Symbol(s) = p_def.pattern {
                let param_id = self.next_id();
                let param_type = self.convert_type_annotation_option(p_def.type_annotation)?;

                let binding_info = BindingInfo {
                    name: s.0.clone(),
                    binding_id: param_id,
                    ir_type: param_type.clone(),
                    kind: BindingKind::Parameter,
                };
                self.define_binding(s.0.clone(), binding_info);

                params.push(IrNode::Param {
                    id: param_id,
                    binding: Box::new(IrNode::VariableBinding {
                        id: param_id,
                        name: s.0,
                        ir_type: param_type.clone(),
                        source_location: None,
                    }),
                    type_annotation: Some(param_type.clone()),
                    ir_type: param_type,
                    source_location: None,
                });
            }
            // TODO: Handle other patterns in params
        }        // Handle variadic parameter
        let variadic_param = if let Some(variadic_param_def) = fn_expr.variadic_param {
            if let Pattern::Symbol(s) = variadic_param_def.pattern {
                let param_id = self.next_id();
                let param_type = self.convert_type_annotation_option(variadic_param_def.type_annotation)?;

                let binding_info = BindingInfo {
                    name: s.0.clone(),
                    binding_id: param_id,
                    ir_type: param_type.clone(),
                    kind: BindingKind::Parameter,
                };
                self.define_binding(s.0.clone(), binding_info);

                Some(Box::new(IrNode::Param {
                    id: param_id,
                    binding: Box::new(IrNode::VariableBinding {
                        id: param_id,
                        name: s.0,
                        ir_type: param_type.clone(),
                        source_location: None,
                    }),
                    type_annotation: Some(param_type.clone()),
                    ir_type: param_type.clone(),
                    source_location: None,
                }))
            } else {
                None // TODO: Handle other patterns in variadic params
            }
        } else {
            None
        };

        let mut body = Vec::new();
        for expr in fn_expr.body {
            body.push(self.convert_expression(expr)?);
        }
        self.exit_scope();

        let return_type = if let Some(ret_type_expr) = fn_expr.return_type {
            self.convert_type_annotation(ret_type_expr)?
        } else {
            body.last().and_then(|n| n.ir_type()).cloned().unwrap_or(IrType::Any)
        };

        let param_types = params.iter().map(|p| p.ir_type().cloned().unwrap_or(IrType::Any)).collect();
        let variadic_param_type = variadic_param.as_ref()
            .and_then(|p| p.ir_type().cloned())
            .map(|t| Box::new(t));

        Ok(IrNode::Lambda {
            id,
            params,
            variadic_param,
            body,
            captures: vec![], // TODO
            ir_type: IrType::Function {
                param_types,
                variadic_param_type,
                return_type: Box::new(return_type),
            },
            source_location: None,
        })
    }

    fn convert_match(&mut self, match_expr: MatchExpr) -> IrConversionResult<IrNode> {
        let id = self.next_id();
        let expression = Box::new(self.convert_expression(*match_expr.expression)?);
        let mut clauses = Vec::new();
        for clause in match_expr.clauses {
            let pattern = self.convert_match_pattern(clause.pattern)?;
            let guard = match clause.guard {
                Some(g) => Some(self.convert_expression(*g)?),
                None => None,
            };
            let body = self.convert_expression(*clause.body)?;
            clauses.push(IrMatchClause { pattern, guard, body });
        }
        let ir_type = clauses.first().map(|c| c.body.ir_type().cloned().unwrap_or(IrType::Any)).unwrap_or(IrType::Any); // Simplified
        Ok(IrNode::Match { id, expression, clauses, ir_type, source_location: None })
    }

    fn convert_vector(&mut self, exprs: Vec<Expression>) -> IrConversionResult<IrNode> {
        let id = self.next_id();
        let mut elements = Vec::new();
        for expr in exprs {
            elements.push(self.convert_expression(expr)?);
        }
        Ok(IrNode::Vector { id, elements, ir_type: IrType::Vector(Box::new(IrType::Any)), source_location: None })
    }

    fn convert_map(&mut self, map: HashMap<MapKey, Expression>) -> IrConversionResult<IrNode> {
        let id = self.next_id();
        let mut entries = Vec::new();
        let mut map_type_entries = Vec::new();

        for (key, value) in map {
            let ir_key = self.convert_map_key(key.clone())?;
            let ir_value = self.convert_expression(value)?;
            
            if let (Some(_key_type), Some(value_type)) = (ir_key.ir_type(), ir_value.ir_type()) {
                 if let IrNode::Literal { value: Literal::Keyword(kw), .. } = &ir_key {
                    map_type_entries.push(IrMapTypeEntry {
                        key: kw.clone(),
                        value_type: value_type.clone(),
                        optional: false,
                    });
                 }
            }

            entries.push(IrMapEntry { key: ir_key, value: ir_value });
        }

        let ir_type = IrType::Map {
            entries: map_type_entries,
            wildcard: None,
        };

        Ok(IrNode::Map { id, entries, ir_type, source_location: None })
    }

    fn convert_map_key(&mut self, key: MapKey) -> IrConversionResult<IrNode> {
        let id = self.next_id();
        match key {
            MapKey::Keyword(k) => Ok(IrNode::Literal { id, value: Literal::Keyword(k), ir_type: IrType::Keyword, source_location: None }),
            MapKey::String(s) => Ok(IrNode::Literal { id, value: Literal::String(s), ir_type: IrType::String, source_location: None }),
            MapKey::Integer(i) => Ok(IrNode::Literal { id, value: Literal::Integer(i), ir_type: IrType::Int, source_location: None }),
        }
    }

    fn convert_list_as_application(&mut self, exprs: Vec<Expression>) -> IrConversionResult<IrNode> {
        if exprs.is_empty() {
            return Err(IrConversionError::InvalidSpecialForm { form: "()".to_string(), message: "Empty list cannot be called".to_string() });
        }
        let callee = exprs[0].clone();
        let arguments = exprs[1..].to_vec();
        self.convert_function_call(callee, arguments)
    }

    fn convert_try_catch(&mut self, try_expr: TryCatchExpr) -> IrConversionResult<IrNode> {
        let id = self.next_id();
        
        let try_body_expressions = try_expr.try_body.into_iter()
            .map(|e| self.convert_expression(e))
            .collect::<Result<_,_>>()?;

        let mut catch_clauses = Vec::new();
        for clause in try_expr.catch_clauses {
            let pattern = match clause.pattern {
                CatchPattern::Keyword(k) => IrPattern::Literal(Literal::Keyword(k)),
                CatchPattern::Type(t) => IrPattern::Type(self.convert_type_annotation(t)?),
                CatchPattern::Symbol(s) => IrPattern::Variable(s.0),
            };
            let body = clause.body.into_iter()
                .map(|e| self.convert_expression(e))
                .collect::<Result<_,_>>()?;
            catch_clauses.push(IrCatchClause {
                error_pattern: pattern,
                binding: Some(clause.binding.0),
                body,
            });
        }
        
        let finally_body = if let Some(fb) = try_expr.finally_body {
            Some(fb.into_iter().map(|e| self.convert_expression(e)).collect::<Result<_,_>>()?)
        } else {
            None
        };

        Ok(IrNode::TryCatch {
            id,
            try_body: try_body_expressions,
            catch_clauses,
            finally_body,
            ir_type: IrType::Any,
            source_location: None,
        })
    }

    fn convert_parallel(&mut self, parallel_expr: ParallelExpr) -> IrConversionResult<IrNode> {
        let id = self.next_id();
        let mut bindings = Vec::new();
        for binding in parallel_expr.bindings {
            let init_expr = self.convert_expression(*binding.expression)?;
            let binding_node = IrNode::VariableBinding {
                id: self.next_id(),
                name: binding.symbol.0,
                ir_type: init_expr.ir_type().cloned().unwrap_or(IrType::Any),
                source_location: None,
            };
            bindings.push(IrParallelBinding {
                binding: binding_node,
                init_expr,
            });
        }
        Ok(IrNode::Parallel { id, bindings, ir_type: IrType::Vector(Box::new(IrType::Any)), source_location: None })
    }

    fn convert_with_resource(&mut self, with_expr: WithResourceExpr) -> IrConversionResult<IrNode> {
        let id = self.next_id();
        let resource_id = self.next_id();
        let resource_name = with_expr.resource_symbol.0;
        let init_expr = Box::new(self.convert_expression(*with_expr.resource_init)?);
        
        self.enter_scope();
        let binding_info = BindingInfo {
            name: resource_name.clone(),
            binding_id: resource_id,
            ir_type: IrType::Any, // Type of resource is not known at this stage
            kind: BindingKind::Resource,
        };
        self.define_binding(resource_name.clone(), binding_info);
        
        let body_expressions = with_expr.body.into_iter()
            .map(|e| self.convert_expression(e))
            .collect::<Result<_,_>>()?;
        
        self.exit_scope();

        let binding_node = IrNode::VariableBinding {
            id: resource_id,
            name: resource_name,
            ir_type: self.convert_type_annotation(with_expr.resource_type)?,
            source_location: None,
        };

        Ok(IrNode::WithResource {
            id,
            binding: Box::new(binding_node),
            init_expr,
            body: body_expressions,
            ir_type: IrType::Any,
            source_location: None,
        })
    }

    fn convert_log_step(&mut self, log_expr: LogStepExpr) -> IrConversionResult<IrNode> {
        let id = self.next_id();
        let values = log_expr.values.into_iter().map(|e| self.convert_expression(e)).collect::<Result<_,_>>()?;
        Ok(IrNode::LogStep {
            id,
            values,
            level: log_expr.level.unwrap_or(Keyword("info".to_string())),
            location: log_expr.location,
            ir_type: IrType::Nil,
            source_location: None,
        })
    }

    fn convert_discover_agents(&mut self, discover_expr: DiscoverAgentsExpr) -> IrConversionResult<IrNode> {
        let id = self.next_id();
        let criteria = Box::new(self.convert_expression(*discover_expr.criteria)?);
        // TODO: Handle options
        Ok(IrNode::DiscoverAgents { id, criteria, ir_type: IrType::Vector(Box::new(IrType::Any)), source_location: None })
    }

    fn convert_def(&mut self, def_expr: DefExpr) -> IrConversionResult<IrNode> {
        let id = self.next_id();
        let name = def_expr.symbol.0;
        let init_expr = Box::new(self.convert_expression(*def_expr.value)?);
        let type_annotation = if let Some(ta) = def_expr.type_annotation {
            Some(self.convert_type_annotation(ta)?)
        } else {
            None
        };
        let ir_type = init_expr.ir_type().cloned().unwrap_or(IrType::Any);

        let binding_info = BindingInfo {
            name: name.clone(),
            binding_id: id, // Use def's ID as binding ID
            ir_type: ir_type.clone(),
            kind: BindingKind::Variable,
        };
        self.define_binding(name.clone(), binding_info);

        Ok(IrNode::VariableDef {
            id,
            name,
            type_annotation,
            init_expr,
            ir_type,
            source_location: None,
        })
    }

    fn convert_defn(&mut self, defn_expr: DefnExpr) -> IrConversionResult<IrNode> {
        let id = self.next_id();
        let name = defn_expr.name.0;

        let fn_expr = FnExpr {
            params: defn_expr.params,
            variadic_param: defn_expr.variadic_param,
            return_type: defn_expr.return_type,
            body: defn_expr.body,
        };
        let lambda = self.convert_fn(fn_expr)?;
        let ir_type = lambda.ir_type().cloned().unwrap_or(IrType::Any);

        let binding_info = BindingInfo {
            name: name.clone(),
            binding_id: id,
            ir_type: ir_type.clone(),
            kind: BindingKind::Function,
        };
        self.define_binding(name.clone(), binding_info);

        Ok(IrNode::FunctionDef {
            id,
            name,
            lambda: Box::new(lambda),
            ir_type,
            source_location: None,
        })
    }

    /// Convert pattern to IR node
    fn convert_pattern(&mut self, pattern: Pattern, binding_id: NodeId, ir_type: IrType) -> IrConversionResult<IrNode> {
        match pattern {
            Pattern::Symbol(sym) => {
                Ok(IrNode::VariableBinding {
                    id: binding_id,
                    name: sym.0,
                    ir_type,
                    source_location: None,
                })
            }
            Pattern::Wildcard => {
                Ok(IrNode::VariableBinding {
                    id: binding_id,
                    name: "_".to_string(),
                    ir_type,
                    source_location: None,
                })
            }
            Pattern::VectorDestructuring { .. } => {
                // Create a destructuring pattern (simplified)
                Ok(IrNode::VariableBinding {
                    id: binding_id,
                    name: format!("__vector_destructure_{}", binding_id),
                    ir_type,
                    source_location: None,
                })
            }
            Pattern::MapDestructuring { .. } => {
                // Similar to vector - simplified destructuring
                Ok(IrNode::VariableBinding {
                    id: binding_id,
                    name: format!("__map_destructure_{}", binding_id),
                    ir_type,
                    source_location: None,
                })
            }
        }
    }
    
    /// Convert type annotation to IR type
    fn convert_type_annotation(&mut self, type_expr: TypeExpr) -> IrConversionResult<IrType> {
        match type_expr {
            TypeExpr::Primitive(PrimitiveType::Int) => Ok(IrType::Int),
            TypeExpr::Primitive(PrimitiveType::Float) => Ok(IrType::Float),
            TypeExpr::Primitive(PrimitiveType::String) => Ok(IrType::String),
            TypeExpr::Primitive(PrimitiveType::Bool) => Ok(IrType::Bool),
            TypeExpr::Primitive(PrimitiveType::Keyword) => Ok(IrType::Keyword),
            TypeExpr::Primitive(PrimitiveType::Symbol) => Ok(IrType::Symbol),
            TypeExpr::Any => Ok(IrType::Any),
            TypeExpr::Never => Ok(IrType::Never),
            TypeExpr::Vector(element_type) => {
                let ir_element_type = self.convert_type_annotation(*element_type)?;
                Ok(IrType::Vector(Box::new(ir_element_type)))
            }
            TypeExpr::Union(types) => {
                let mut ir_types = Vec::new();
                for t in types {
                    ir_types.push(self.convert_type_annotation(t)?);
                }
                Ok(IrType::Union(ir_types))
            }
            TypeExpr::Literal(lit) => Ok(IrType::LiteralValue(lit)),
            TypeExpr::Alias(sym) => Ok(IrType::TypeRef(sym.0)),
            _ => Ok(IrType::Any), // TODO: Implement remaining type conversions
        }
    }

    fn convert_type_annotation_option(&mut self, type_expr: Option<TypeExpr>) -> IrConversionResult<IrType> {
        match type_expr {
            Some(t) => self.convert_type_annotation(t),
            None => Ok(IrType::Any),
        }
    }

    fn convert_match_pattern(&mut self, pattern: MatchPattern) -> IrConversionResult<IrPattern> {
        match pattern {
            MatchPattern::Literal(l) => Ok(IrPattern::Literal(l)),
            MatchPattern::Symbol(s) => Ok(IrPattern::Variable(s.0)),
            MatchPattern::Keyword(k) => Ok(IrPattern::Literal(Literal::Keyword(k))),
            MatchPattern::Wildcard => Ok(IrPattern::Wildcard),
            MatchPattern::Vector { elements, rest } => {
                let mut ir_elements = Vec::new();
                for el in elements {
                    ir_elements.push(self.convert_match_pattern(el)?);
                }
                Ok(IrPattern::Vector {
                    elements: ir_elements,
                    rest: rest.map(|s| s.0),
                })
            }
            MatchPattern::Map { entries, rest } => {
                let mut ir_entries = Vec::new();
                for entry in entries {
                    ir_entries.push(IrMapPatternEntry {
                        key: entry.key,
                        pattern: self.convert_match_pattern(*entry.pattern)?,
                    });
                }
                Ok(IrPattern::Map {
                    entries: ir_entries,
                    rest: rest.map(|s| s.0),
                })
            }
            MatchPattern::Type(type_expr, _) => {
                let ir_type = self.convert_type_annotation(type_expr)?;
                Ok(IrPattern::Type(ir_type))
            }
            MatchPattern::As(_, pattern) => {
                // This is tricky. The `as` pattern binds the whole value.
                // For now, we ignore the `as` binding and just convert the inner pattern.
                // A more complete implementation would handle this binding.
                self.convert_match_pattern(*pattern)
            }
        }
    }

    pub fn into_bindings(mut self) -> HashMap<String, BindingInfo> {
        if self.scope_stack.is_empty() {
            HashMap::new()
        } else {
            self.scope_stack.remove(0)
        }
    }
}
