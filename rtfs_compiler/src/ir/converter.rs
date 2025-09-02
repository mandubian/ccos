// Complete AST to IR Converter Implementation
// Provides full conversion from parsed AST to optimized IR

use crate::ast::*;
use crate::ir::core::*;
use crate::runtime::module_runtime::ModuleRegistry;

use std::collections::HashMap;
use std::rc::Rc;

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
    },
    InvalidTypeAnnotation {
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
    Type,
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
        self.bindings
            .get(name)
            .or_else(|| self.parent.as_ref().and_then(|p| p.lookup(name)))
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
            (
                "+",
                IrType::Function {
                    param_types: vec![IrType::Int, IrType::Int],
                    variadic_param_type: Some(Box::new(IrType::Int)),
                    return_type: Box::new(IrType::Int),
                },
            ),
            (
                "-",
                IrType::Function {
                    param_types: vec![IrType::Int, IrType::Int],
                    variadic_param_type: None,
                    return_type: Box::new(IrType::Int),
                },
            ),
            (
                "*",
                IrType::Function {
                    param_types: vec![IrType::Int, IrType::Int],
                    variadic_param_type: Some(Box::new(IrType::Int)),
                    return_type: Box::new(IrType::Int),
                },
            ),
            (
                "/",
                IrType::Function {
                    param_types: vec![IrType::Int, IrType::Int],
                    variadic_param_type: None,
                    return_type: Box::new(IrType::Int),
                },
            ),
            // Collection helpers
            (
                "keys",
                IrType::Function {
                    param_types: vec![IrType::Map { entries: vec![], wildcard: Some(Box::new(IrType::Any)) }],
                    variadic_param_type: None,
                    return_type: Box::new(IrType::Vector(Box::new(IrType::Any))),
                },
            ),
            (
                "update",
                IrType::Function {
                    param_types: vec![
                        IrType::Map { entries: vec![], wildcard: Some(Box::new(IrType::Any)) },
                        IrType::Any,
                        IrType::Function {
                            param_types: vec![IrType::Any],
                            variadic_param_type: None,
                            return_type: Box::new(IrType::Any),
                        },
                    ],
                    variadic_param_type: None,
                    return_type: Box::new(IrType::Map { entries: vec![], wildcard: Some(Box::new(IrType::Any)) }),
                },
            ),
            (
                "thread/sleep",
                IrType::Function {
                    param_types: vec![IrType::Int],
                    variadic_param_type: None,
                    return_type: Box::new(IrType::Nil),
                },
            ),
            // Comparison operators
            (
                "=",
                IrType::Function {
                    param_types: vec![IrType::Any, IrType::Any],
                    variadic_param_type: None,
                    return_type: Box::new(IrType::Bool),
                },
            ),
            (
                "!=",
                IrType::Function {
                    param_types: vec![IrType::Any, IrType::Any],
                    variadic_param_type: None,
                    return_type: Box::new(IrType::Bool),
                },
            ),
            (
                ">",
                IrType::Function {
                    param_types: vec![IrType::Any, IrType::Any],
                    variadic_param_type: None,
                    return_type: Box::new(IrType::Bool),
                },
            ),
            (
                "<",
                IrType::Function {
                    param_types: vec![IrType::Any, IrType::Any],
                    variadic_param_type: None,
                    return_type: Box::new(IrType::Bool),
                },
            ),
            (
                ">=",
                IrType::Function {
                    param_types: vec![IrType::Any, IrType::Any],
                    variadic_param_type: None,
                    return_type: Box::new(IrType::Bool),
                },
            ),
            (
                "<=",
                IrType::Function {
                    param_types: vec![IrType::Any, IrType::Any],
                    variadic_param_type: None,
                    return_type: Box::new(IrType::Bool),
                },
            ),
            // Logical operators
            (
                "and",
                IrType::Function {
                    param_types: vec![IrType::Any],
                    variadic_param_type: Some(Box::new(IrType::Any)),
                    return_type: Box::new(IrType::Any),
                },
            ),
            (
                "or",
                IrType::Function {
                    param_types: vec![IrType::Any],
                    variadic_param_type: Some(Box::new(IrType::Any)),
                    return_type: Box::new(IrType::Any),
                },
            ),
            (
                "not",
                IrType::Function {
                    param_types: vec![IrType::Any],
                    variadic_param_type: None,
                    return_type: Box::new(IrType::Bool),
                },
            ),
            // String functions
            (
                "str",
                IrType::Function {
                    param_types: vec![IrType::Any],
                    variadic_param_type: Some(Box::new(IrType::Any)),
                    return_type: Box::new(IrType::String),
                },
            ),
            (
                "string?",
                IrType::Function {
                    param_types: vec![IrType::Any],
                    variadic_param_type: None,
                    return_type: Box::new(IrType::Bool),
                },
            ),
            (
                "string-length",
                IrType::Function {
                    param_types: vec![IrType::String],
                    variadic_param_type: None,
                    return_type: Box::new(IrType::Int),
                },
            ),
            (
                "substring",
                IrType::Function {
                    param_types: vec![IrType::String, IrType::Int],
                    variadic_param_type: None,
                    return_type: Box::new(IrType::String),
                },
            ),
            // Collection functions
            (
                "map",
                IrType::Function {
                    param_types: vec![IrType::Any],
                    variadic_param_type: Some(Box::new(IrType::Any)),
                    return_type: Box::new(IrType::Any),
                },
            ),
            (
                "vector",
                IrType::Function {
                    param_types: vec![],
                    variadic_param_type: Some(Box::new(IrType::Any)),
                    return_type: Box::new(IrType::Vector(Box::new(IrType::Any))),
                },
            ),
            (
                "vector?",
                IrType::Function {
                    param_types: vec![IrType::Any],
                    variadic_param_type: None,
                    return_type: Box::new(IrType::Bool),
                },
            ),
            (
                "map?",
                IrType::Function {
                    param_types: vec![IrType::Any],
                    variadic_param_type: None,
                    return_type: Box::new(IrType::Bool),
                },
            ),
            (
                "count",
                IrType::Function {
                    param_types: vec![IrType::Any],
                    variadic_param_type: None,
                    return_type: Box::new(IrType::Int),
                },
            ),
            (
                "get",
                IrType::Function {
                    param_types: vec![IrType::Any, IrType::Any],
                    variadic_param_type: None,
                    return_type: Box::new(IrType::Any),
                },
            ),
            (
                "assoc",
                IrType::Function {
                    param_types: vec![IrType::Any, IrType::Any, IrType::Any],
                    variadic_param_type: Some(Box::new(IrType::Any)),
                    return_type: Box::new(IrType::Any),
                },
            ),
            (
                "dissoc",
                IrType::Function {
                    param_types: vec![IrType::Any, IrType::Any],
                    variadic_param_type: Some(Box::new(IrType::Any)),
                    return_type: Box::new(IrType::Any),
                },
            ),
            (
                "conj",
                IrType::Function {
                    param_types: vec![IrType::Any, IrType::Any],
                    variadic_param_type: Some(Box::new(IrType::Any)),
                    return_type: Box::new(IrType::Any),
                },
            ),
            // Type predicate functions
            (
                "nil?",
                IrType::Function {
                    param_types: vec![IrType::Any],
                    variadic_param_type: None,
                    return_type: Box::new(IrType::Bool),
                },
            ),
            (
                "bool?",
                IrType::Function {
                    param_types: vec![IrType::Any],
                    variadic_param_type: None,
                    return_type: Box::new(IrType::Bool),
                },
            ),
            (
                "int?",
                IrType::Function {
                    param_types: vec![IrType::Any],
                    variadic_param_type: None,
                    return_type: Box::new(IrType::Bool),
                },
            ),
            (
                "float?",
                IrType::Function {
                    param_types: vec![IrType::Any],
                    variadic_param_type: None,
                    return_type: Box::new(IrType::Bool),
                },
            ),
            (
                "number?",
                IrType::Function {
                    param_types: vec![IrType::Any],
                    variadic_param_type: None,
                    return_type: Box::new(IrType::Bool),
                },
            ),
            (
                "fn?",
                IrType::Function {
                    param_types: vec![IrType::Any],
                    variadic_param_type: None,
                    return_type: Box::new(IrType::Bool),
                },
            ),
            (
                "symbol?",
                IrType::Function {
                    param_types: vec![IrType::Any],
                    variadic_param_type: None,
                    return_type: Box::new(IrType::Bool),
                },
            ),
            (
                "keyword?",
                IrType::Function {
                    param_types: vec![IrType::Any],
                    variadic_param_type: None,
                    return_type: Box::new(IrType::Bool),
                },
            ),
            (
                "empty?",
                IrType::Function {
                    param_types: vec![IrType::Any],
                    variadic_param_type: None,
                    return_type: Box::new(IrType::Bool),
                },
            ),
            (
                "even?",
                IrType::Function {
                    param_types: vec![IrType::Int],
                    variadic_param_type: None,
                    return_type: Box::new(IrType::Bool),
                },
            ),
            (
                "odd?",
                IrType::Function {
                    param_types: vec![IrType::Int],
                    variadic_param_type: None,
                    return_type: Box::new(IrType::Bool),
                },
            ),
            (
                "zero?",
                IrType::Function {
                    param_types: vec![IrType::Int],
                    variadic_param_type: None,
                    return_type: Box::new(IrType::Bool),
                },
            ),
            (
                "pos?",
                IrType::Function {
                    param_types: vec![IrType::Int],
                    variadic_param_type: None,
                    return_type: Box::new(IrType::Bool),
                },
            ),
            (
                "neg?",
                IrType::Function {
                    param_types: vec![IrType::Int],
                    variadic_param_type: None,
                    return_type: Box::new(IrType::Bool),
                },
            ),
            (
                "inc",
                IrType::Function {
                    param_types: vec![IrType::Int],
                    variadic_param_type: None,
                    return_type: Box::new(IrType::Int),
                },
            ),
            (
                "dec",
                IrType::Function {
                    param_types: vec![IrType::Int],
                    variadic_param_type: None,
                    return_type: Box::new(IrType::Int),
                },
            ),
            (
                "partition",
                IrType::Function {
                    param_types: vec![IrType::Int, IrType::Any],
                    variadic_param_type: None,
                    return_type: Box::new(IrType::Vector(Box::new(IrType::Any))),
                },
            ),
            (
                "first",
                IrType::Function {
                    param_types: vec![IrType::Any],
                    variadic_param_type: None,
                    return_type: Box::new(IrType::Any),
                },
            ),
            (
                "rest",
                IrType::Function {
                    param_types: vec![IrType::Any],
                    variadic_param_type: None,
                    return_type: Box::new(IrType::Any),
                },
            ),
            (
                "cons",
                IrType::Function {
                    param_types: vec![IrType::Any, IrType::Any],
                    variadic_param_type: None,
                    return_type: Box::new(IrType::Any),
                },
            ),
            (
                "concat",
                IrType::Function {
                    param_types: vec![IrType::Any],
                    variadic_param_type: Some(Box::new(IrType::Any)),
                    return_type: Box::new(IrType::Any),
                },
            ),
            (
                "nth",
                IrType::Function {
                    param_types: vec![IrType::Any, IrType::Int],
                    variadic_param_type: None,
                    return_type: Box::new(IrType::Any),
                },
            ),
            (
                "contains?",
                IrType::Function {
                    param_types: vec![IrType::Any, IrType::Any],
                    variadic_param_type: None,
                    return_type: Box::new(IrType::Bool),
                },
            ),
            // Higher-order functions
            (
                "map-fn",
                IrType::Function {
                    param_types: vec![IrType::Any, IrType::Any],
                    variadic_param_type: Some(Box::new(IrType::Any)),
                    return_type: Box::new(IrType::Any),
                },
            ),
            (
                "filter",
                IrType::Function {
                    param_types: vec![IrType::Any, IrType::Any],
                    variadic_param_type: None,
                    return_type: Box::new(IrType::Any),
                },
            ),
            (
                "reduce",
                IrType::Function {
                    param_types: vec![IrType::Any, IrType::Any],
                    variadic_param_type: None,
                    return_type: Box::new(IrType::Any),
                },
            ),
            // Agent system functions
            (
                "discover-agents",
                IrType::Function {
                    param_types: vec![IrType::Map {
                        entries: vec![],
                        wildcard: Some(Box::new(IrType::Any)),
                    }],
                    variadic_param_type: None,
                    return_type: Box::new(IrType::Vector(Box::new(IrType::Any))),
                },
            ),
            (
                "task",
                IrType::Function {
                    param_types: vec![IrType::Any],
                    variadic_param_type: Some(Box::new(IrType::Any)),
                    return_type: Box::new(IrType::Any),
                },
            ),
            // Mathematical functions
            (
                "fact",
                IrType::Function {
                    param_types: vec![IrType::Int],
                    variadic_param_type: None,
                    return_type: Box::new(IrType::Int),
                },
            ),
            (
                "max",
                IrType::Function {
                    param_types: vec![IrType::Any],
                    variadic_param_type: Some(Box::new(IrType::Any)),
                    return_type: Box::new(IrType::Any),
                },
            ),
            (
                "min",
                IrType::Function {
                    param_types: vec![IrType::Any],
                    variadic_param_type: Some(Box::new(IrType::Any)),
                    return_type: Box::new(IrType::Any),
                },
            ),
            (
                "length",
                IrType::Function {
                    param_types: vec![IrType::Any],
                    variadic_param_type: None,
                    return_type: Box::new(IrType::Int),
                },
            ),
            // Agent system functions - Advanced
            (
                "discover-and-assess-agents",
                IrType::Function {
                    param_types: vec![IrType::Any],
                    variadic_param_type: Some(Box::new(IrType::Any)),
                    return_type: Box::new(IrType::Vector(Box::new(IrType::Any))),
                },
            ),
            (
                "establish-system-baseline",
                IrType::Function {
                    param_types: vec![IrType::Any],
                    variadic_param_type: Some(Box::new(IrType::Any)),
                    return_type: Box::new(IrType::Map {
                        entries: vec![],
                        wildcard: Some(Box::new(IrType::Any)),
                    }),
                },
            ),
            // Tool functions for agent coordination
            (
                "tool:current-timestamp-ms",
                IrType::Function {
                    param_types: vec![],
                    variadic_param_type: None,
                    return_type: Box::new(IrType::Int),
                },
            ),
            (
                "tool:log",
                IrType::Function {
                    param_types: vec![IrType::Any],
                    variadic_param_type: None,
                    return_type: Box::new(IrType::Nil),
                },
            ),
            // Additional tool functions
            (
                "println",
                IrType::Function {
                    param_types: vec![IrType::Any],
                    variadic_param_type: Some(Box::new(IrType::Any)),
                    return_type: Box::new(IrType::Nil),
                },
            ),
            (
                "tool:print",
                IrType::Function {
                    param_types: vec![IrType::Any],
                    variadic_param_type: Some(Box::new(IrType::Any)),
                    return_type: Box::new(IrType::Nil),
                },
            ),
            (
                "tool:http-fetch",
                IrType::Function {
                    param_types: vec![IrType::String],
                    variadic_param_type: Some(Box::new(IrType::Any)),
                    return_type: Box::new(IrType::Any),
                },
            ),
            (
                "tool:parse-json",
                IrType::Function {
                    param_types: vec![IrType::String],
                    variadic_param_type: None,
                    return_type: Box::new(IrType::Any),
                },
            ),
            (
                "tool:serialize-json",
                IrType::Function {
                    param_types: vec![IrType::Any],
                    variadic_param_type: None,
                    return_type: Box::new(IrType::String),
                },
            ),
            (
                "tool:get-env",
                IrType::Function {
                    param_types: vec![IrType::String],
                    variadic_param_type: None,
                    return_type: Box::new(IrType::String),
                },
            ),
            (
                "tool:file-exists?",
                IrType::Function {
                    param_types: vec![IrType::String],
                    variadic_param_type: None,
                    return_type: Box::new(IrType::Bool),
                },
            ),
            (
                "tool:current-time",
                IrType::Function {
                    param_types: vec![],
                    variadic_param_type: None,
                    return_type: Box::new(IrType::String),
                },
            ),
            // Additional stdlib functions
            (
                "string-contains",
                IrType::Function {
                    param_types: vec![IrType::String, IrType::String],
                    variadic_param_type: None,
                    return_type: Box::new(IrType::Bool),
                },
            ),
            (
                "type-name",
                IrType::Function {
                    param_types: vec![IrType::Any],
                    variadic_param_type: None,
                    return_type: Box::new(IrType::String),
                },
            ),
            (
                "range",
                IrType::Function {
                    param_types: vec![IrType::Int],
                    variadic_param_type: Some(Box::new(IrType::Int)),
                    return_type: Box::new(IrType::Vector(Box::new(IrType::Int))),
                },
            ),
            (
                "hash-map",
                IrType::Function {
                    param_types: vec![],
                    variadic_param_type: Some(Box::new(IrType::Any)),
                    return_type: Box::new(IrType::Map {
                        entries: vec![],
                        wildcard: Some(Box::new(IrType::Any)),
                    }),
                },
            ),
            (
                "get-in",
                IrType::Function {
                    param_types: vec![IrType::Any, IrType::Vector(Box::new(IrType::Any))],
                    variadic_param_type: None,
                    return_type: Box::new(IrType::Any),
                },
            ),
            (
                "current-timestamp-ms",
                IrType::Function {
                    param_types: vec![],
                    variadic_param_type: None,
                    return_type: Box::new(IrType::Int),
                },
            ),
            (
                "discover-agents",
                IrType::Function {
                    param_types: vec![IrType::Any],
                    variadic_param_type: Some(Box::new(IrType::Any)),
                    return_type: Box::new(IrType::Vector(Box::new(IrType::Any))),
                },
            ),
            (
                "task-coordination",
                IrType::Function {
                    param_types: vec![IrType::Any],
                    variadic_param_type: Some(Box::new(IrType::Any)),
                    return_type: Box::new(IrType::Any),
                },
            ),
            (
                "factorial",
                IrType::Function {
                    param_types: vec![IrType::Int],
                    variadic_param_type: None,
                    return_type: Box::new(IrType::Int),
                },
            ),
            (
                "discover-and-assess-agents",
                IrType::Function {
                    param_types: vec![IrType::Any],
                    variadic_param_type: Some(Box::new(IrType::Any)),
                    return_type: Box::new(IrType::Vector(Box::new(IrType::Any))),
                },
            ),
            (
                "establish-system-baseline",
                IrType::Function {
                    param_types: vec![IrType::Any],
                    variadic_param_type: Some(Box::new(IrType::Any)),
                    return_type: Box::new(IrType::Map {
                        entries: vec![],
                        wildcard: Some(Box::new(IrType::Any)),
                    }),
                },
            ),
            // Special forms
            (
                "lambda",
                IrType::Function {
                    param_types: vec![IrType::Any, IrType::Any],
                    variadic_param_type: Some(Box::new(IrType::Any)),
                    return_type: Box::new(IrType::Function {
                        param_types: vec![],
                        variadic_param_type: Some(Box::new(IrType::Any)),
                        return_type: Box::new(IrType::Any),
                    }),
                },
            ),
            (
                "quote",
                IrType::Function {
                    param_types: vec![IrType::Any],
                    variadic_param_type: None,
                    return_type: Box::new(IrType::Any),
                },
            ),
            (
                "letrec",
                IrType::Function {
                    param_types: vec![IrType::Vector(Box::new(IrType::Any)), IrType::Any],
                    variadic_param_type: Some(Box::new(IrType::Any)),
                    return_type: Box::new(IrType::Any),
                },
            ),
            (
                "if",
                IrType::Function {
                    param_types: vec![IrType::Any, IrType::Any],
                    variadic_param_type: Some(Box::new(IrType::Any)),
                    return_type: Box::new(IrType::Any),
                },
            ),
            (
                "let",
                IrType::Function {
                    param_types: vec![IrType::Vector(Box::new(IrType::Any)), IrType::Any],
                    variadic_param_type: Some(Box::new(IrType::Any)),
                    return_type: Box::new(IrType::Any),
                },
            ),
            (
                "do",
                IrType::Function {
                    param_types: vec![IrType::Any],
                    variadic_param_type: Some(Box::new(IrType::Any)),
                    return_type: Box::new(IrType::Any),
                },
            ),
            (
                "def",
                IrType::Function {
                    param_types: vec![IrType::Any, IrType::Any],
                    variadic_param_type: None,
                    return_type: Box::new(IrType::Any),
                },
            ),
            (
                "defn",
                IrType::Function {
                    param_types: vec![
                        IrType::Any,
                        IrType::Vector(Box::new(IrType::Any)),
                        IrType::Any,
                    ],
                    variadic_param_type: Some(Box::new(IrType::Any)),
                    return_type: Box::new(IrType::Any),
                },
            ),
            // Capability system functions
            (
                "call",
                IrType::Function {
                    param_types: vec![IrType::Keyword, IrType::Any],
                    variadic_param_type: Some(Box::new(IrType::Any)),
                    return_type: Box::new(IrType::Any),
                },
            ),
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
            Expression::If(if_expr) => self.convert_if(if_expr),
            Expression::Let(let_expr) => self.convert_let(let_expr),
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
            Expression::DiscoverAgents(discover_expr) => {
                self.convert_discover_agents(discover_expr)
            }
            Expression::Def(def_expr) => self.convert_def(*def_expr),
            Expression::Defn(defn_expr) => self.convert_defn(*defn_expr),
            Expression::Defstruct(defstruct_expr) => self.convert_defstruct(*defstruct_expr),
            Expression::For(for_expr) => self.convert_for(*for_expr),
            Expression::Deref(_) => Err(IrConversionError::InvalidSpecialForm {
                form: "deref".to_string(),
                message: "Deref sugar @atom not yet implemented in IR converter".to_string(),
            }),
            Expression::ResourceRef(resource_ref) => {
                let id = self.next_id();
                let resource_name = resource_ref.clone();

                // Look up the resource in the current scope
                if let Some(binding_info) = self.lookup_symbol(&resource_name) {
                    Ok(IrNode::ResourceRef {
                        id,
                        name: resource_name,
                        binding_id: binding_info.binding_id,
                        ir_type: binding_info.ir_type.clone(),
                        source_location: None, // TODO: Add source location
                    })
                } else {
                    Err(IrConversionError::UndefinedSymbol {
                        symbol: resource_name,
                        location: None, // TODO: Add source location
                    })
                }
            }
            // Plan is not a core RTFS expression; handled in CCOS layer
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
            Literal::Timestamp(_) => IrType::Any, // TODO: Add specific timestamp type
            Literal::Uuid(_) => IrType::Any,      // TODO: Add specific UUID type
            Literal::ResourceHandle(_) => IrType::Any, // TODO: Add specific resource handle type
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
                let symbol_name = &name[index + 1..];

                if let Some(registry) = self.module_registry {
                    if let Some(module) = registry.get_module(module_name) {
                        // Check exported functions/values
                        if let Some(export) = module.exports.read().unwrap().get(symbol_name) {
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
            Some(binding_info) => Ok(IrNode::VariableRef {
                id,
                name,
                binding_id: binding_info.binding_id,
                ir_type: binding_info.ir_type.clone(),
                source_location: None,
            }),
            None => {
                // Check if it's available in the stdlib module
                if let Some(registry) = self.module_registry {
                    if let Some(stdlib_module) = registry.get_module("stdlib") {
                        if let Some(export) = stdlib_module.exports.read().unwrap().get(&name) {
                            return Ok(IrNode::QualifiedSymbolRef {
                                id,
                                module: "stdlib".to_string(),
                                symbol: name,
                                ir_type: export.ir_type.clone(),
                                source_location: None,
                            });
                        }
                    }
                }
                
                Err(IrConversionError::UndefinedSymbol {
                    symbol: name,
                    location: None,
                })
            }
        }
    }

    /// Convert function call
    fn convert_function_call(
        &mut self,
        callee: Expression,
        arguments: Vec<Expression>,
    ) -> IrConversionResult<IrNode> {
        // Check for special forms first
        if let Expression::Symbol(Symbol(ref name)) = callee {
            match name.as_str() {
                "lambda" => {
                    return self.convert_lambda_special_form(arguments);
                }
                "step-if" => {
                    return self.convert_step_if_special_form(arguments);
                }
                "step-loop" => {
                    return self.convert_step_loop_special_form(arguments);
                }
                "step-parallel" => {
                    return self.convert_step_parallel_special_form(arguments);
                }
                "step" => {
                    return self.convert_step_special_form(arguments);
                }
                "set!" => {
                    return self.convert_set_special_form(arguments);
                }
                "dotimes" => {
                    return self.convert_dotimes_special_form(arguments);
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

    /// Convert set! special form: (set! symbol expr)
    fn convert_set_special_form(&mut self, arguments: Vec<Expression>) -> IrConversionResult<IrNode> {
        if arguments.len() != 2 {
            return Err(IrConversionError::InvalidSpecialForm {
                form: "set!".to_string(),
                message: "set! requires exactly 2 arguments: symbol and expression".to_string(),
            });
        }

        // First argument must be a symbol
        let sym = if let Expression::Symbol(Symbol(name)) = &arguments[0] {
            name.clone()
        } else {
            return Err(IrConversionError::InvalidSpecialForm {
                form: "set!".to_string(),
                message: "first argument to set! must be a symbol".to_string(),
            });
        };

        // Convert the RHS expression
        let value_node = Box::new(self.convert_expression(arguments[1].clone())?);

        // Create an assignment node using IrNode::Apply with a builtin function call
        let id = self.next_id();
        Ok(IrNode::Apply {
            id,
            function: Box::new(IrNode::VariableBinding {
                id: 0,
                name: "set!".to_string(),
                ir_type: IrType::Any,
                source_location: None,
            }),
            arguments: vec![
                IrNode::VariableRef {
                    id: self.next_id(),
                    name: sym,
                    binding_id: 0, // Will be resolved during execution
                    ir_type: IrType::Any,
                    source_location: None,
                },
                *value_node,
            ],
            ir_type: IrType::Any,
            source_location: None,
        })
    }

    /// Convert lambda special form: (lambda [params] body...)
    fn convert_lambda_special_form(
        &mut self,
        arguments: Vec<Expression>,
    ) -> IrConversionResult<IrNode> {
        if arguments.len() < 2 {
            return Err(IrConversionError::InvalidSpecialForm {
                form: "lambda".to_string(),
                message: "lambda requires at least 2 arguments: parameter list and body"
                    .to_string(),
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

        // Analyze captures BEFORE exiting lambda scope
        let captures = self.analyze_captures(&body_exprs);

        // Exit lambda scope
        self.exit_scope();

        // Determine return type from last body expression
        let return_type = body_exprs
            .last()
            .and_then(|expr| expr.ir_type())
            .cloned()
            .unwrap_or(IrType::Any);

        // Build function type
        let param_types: Vec<IrType> = params.iter().filter_map(|p| p.ir_type()).cloned().collect();

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
            captures,
            ir_type: function_type,
            source_location: None,
        })
    }

    /// Convert step-if special form: (step-if condition then-branch else-branch?)
    fn convert_step_if_special_form(
        &mut self,
        arguments: Vec<Expression>,
    ) -> IrConversionResult<IrNode> {
        if arguments.len() < 2 {
            return Err(IrConversionError::InvalidSpecialForm {
                form: "step-if".to_string(),
                message: "step-if requires at least 2 arguments: condition and then-branch".to_string(),
            });
        }

        let id = self.next_id();
        let condition = Box::new(self.convert_expression(arguments[0].clone())?);
        let then_branch = Box::new(self.convert_expression(arguments[1].clone())?);
        let else_branch = if arguments.len() > 2 {
            Some(Box::new(self.convert_expression(arguments[2].clone())?))
        } else {
            None
        };

        // Infer result type as union of branches
        let result_type = match (
            then_branch.ir_type(),
            else_branch.as_ref().and_then(|e| e.ir_type()),
        ) {
            (Some(then_type), Some(else_type)) if then_type == else_type => then_type.clone(),
            (Some(then_type), Some(else_type)) => {
                IrType::Union(vec![then_type.clone(), else_type.clone()])
            }
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

    /// Convert step-loop special form: (step-loop condition body...)
    fn convert_step_loop_special_form(
        &mut self,
        arguments: Vec<Expression>,
    ) -> IrConversionResult<IrNode> {
        if arguments.len() < 2 {
            return Err(IrConversionError::InvalidSpecialForm {
                form: "step-loop".to_string(),
                message: "step-loop requires at least 2 arguments: condition and body".to_string(),
            });
        }

        let id = self.next_id();
        let condition = Box::new(self.convert_expression(arguments[0].clone())?);
        
        // Convert body expressions
        let body_exprs: Vec<IrNode> = arguments[1..]
            .iter()
            .map(|expr| self.convert_expression(expr.clone()))
            .collect::<Result<_, _>>()?;

        // Create a do block for the body
        let body_do = IrNode::Do {
            id: self.next_id(),
            expressions: body_exprs,
            ir_type: IrType::Any,
            source_location: None,
        };

        // For now, we'll create a simple if that evaluates the condition
        // In a full implementation, this would need to be a proper loop construct
        // For the IR runtime, we'll just evaluate the condition and return the body result
        let then_branch = Box::new(body_do);
        let else_branch = None; // No else branch for step-loop

        Ok(IrNode::If {
            id,
            condition,
            then_branch,
            else_branch,
            ir_type: IrType::Any,
            source_location: None,
        })
    }

    /// Convert step-parallel special form: (step-parallel expr1 expr2 ...)
    fn convert_step_parallel_special_form(
        &mut self,
        arguments: Vec<Expression>,
    ) -> IrConversionResult<IrNode> {
        if arguments.is_empty() {
            return Err(IrConversionError::InvalidSpecialForm {
                form: "step-parallel".to_string(),
                message: "step-parallel requires at least one argument".to_string(),
            });
        }

        let id = self.next_id();
        let mut bindings = Vec::new();

        // Convert each argument to a parallel binding
        for (i, arg) in arguments.iter().enumerate() {
            let init_expr = self.convert_expression(arg.clone())?;
            let binding_name = format!("parallel_{}", i);
            
            let binding_node = IrNode::VariableBinding {
                id: self.next_id(),
                name: binding_name.clone(),
                ir_type: init_expr.ir_type().cloned().unwrap_or(IrType::Any),
                source_location: None,
            };
            
            bindings.push(IrParallelBinding {
                binding: binding_node,
                init_expr,
            });
        }

        Ok(IrNode::Parallel {
            id,
            bindings,
            ir_type: IrType::Vector(Box::new(IrType::Any)),
            source_location: None,
        })
    }

    /// Convert step special form: (step "name" [options as keyword/value pairs] body...)
    fn convert_step_special_form(&mut self, arguments: Vec<Expression>) -> IrConversionResult<IrNode> {
        if arguments.is_empty() {
            return Err(IrConversionError::InvalidSpecialForm { form: "step".to_string(), message: "step requires at least a name string".to_string() });
        }

        let id = self.next_id();

        // First arg must be a string literal name
        let name = match &arguments[0] {
            Expression::Literal(Literal::String(s)) => s.clone(),
            _ => {
                return Err(IrConversionError::InvalidSpecialForm { form: "step".to_string(), message: "first argument must be a string name".to_string() });
            }
        };

        // Parse optional keyword options similar to evaluator: :expose-context, :context-keys, :params
        use crate::ast::Literal as Lit;
        let mut i = 1usize;
        let mut expose_override: Option<IrNode> = None;
        let mut context_keys_override: Option<IrNode> = None;
        let mut params_node: Option<IrNode> = None;

        while i + 1 <= arguments.len() {
            if i >= arguments.len() { break; }
            match &arguments[i] {
                Expression::Literal(Lit::Keyword(k)) => {
                    let key = k.0.as_str();
                    if i + 1 >= arguments.len() { break; }
                    let val_expr = arguments[i + 1].clone();
                    match key {
                        "expose-context" => {
                            expose_override = Some(self.convert_expression(val_expr)?);
                            i += 2; continue;
                        }
                        "context-keys" => {
                            context_keys_override = Some(self.convert_expression(val_expr)?);
                            i += 2; continue;
                        }
                        "params" => {
                            // Expect a map expression; convert it to IR map node
                            params_node = Some(self.convert_expression(val_expr)?);
                            i += 2; continue;
                        }
                        _ => { break; }
                    }
                }
                _ => break,
            }
        }

        // Remaining expressions are the body
        let mut body_exprs = Vec::new();
        for expr in &arguments[i..] {
            body_exprs.push(self.convert_expression(expr.clone())?);
        }

        Ok(IrNode::Step {
            id,
            name,
            expose_override: expose_override.map(Box::new),
            context_keys_override: context_keys_override.map(Box::new),
            params: params_node.map(Box::new),
            body: body_exprs,
            ir_type: IrType::Any,
            source_location: None,
        })
    }

    /// Convert dotimes special form: (dotimes [i n] body)
    fn convert_dotimes_special_form(&mut self, arguments: Vec<Expression>) -> IrConversionResult<IrNode> {
        if arguments.len() != 2 {
            return Err(IrConversionError::InvalidSpecialForm {
                form: "dotimes".to_string(),
                message: "dotimes requires exactly 2 arguments: [symbol count] and body".to_string(),
            });
        }

        // First argument must be a vector with [symbol count]
        let binding_vector = if let Expression::Vector(v) = &arguments[0] {
            if v.len() != 2 {
                return Err(IrConversionError::InvalidSpecialForm {
                    form: "dotimes".to_string(),
                    message: "dotimes binding vector must have exactly 2 elements: [symbol count]".to_string(),
                });
            }
            v.clone()
        } else {
            return Err(IrConversionError::InvalidSpecialForm {
                form: "dotimes".to_string(),
                message: "dotimes first argument must be a vector [symbol count]".to_string(),
            });
        };

        // Extract symbol and count
        let loop_var = if let Expression::Symbol(s) = &binding_vector[0] {
            s.clone()
        } else {
            return Err(IrConversionError::InvalidSpecialForm {
                form: "dotimes".to_string(),
                message: "dotimes binding vector first element must be a symbol".to_string(),
            });
        };

        let count_expr = Box::new(self.convert_expression(binding_vector[1].clone())?);
        let body_expr = Box::new(self.convert_expression(arguments[1].clone())?);

        // For now, create a simple loop structure
        // In a full implementation, this would create a proper loop IR node
        let id = self.next_id();
        Ok(IrNode::Apply {
            id,
            function: Box::new(IrNode::VariableBinding {
                id: 0,
                name: "dotimes".to_string(),
                ir_type: IrType::Any,
                source_location: None,
            }),
            arguments: vec![
                IrNode::VariableRef {
                    id: self.next_id(),
                    name: loop_var.0,
                    binding_id: 0,
                    ir_type: IrType::Any,
                    source_location: None,
                },
                *count_expr,
                *body_expr,
            ],
            ir_type: IrType::Any,
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
        let result_type = match (
            then_branch.ir_type(),
            else_branch.as_ref().and_then(|e| e.ir_type()),
        ) {
            (Some(then_type), Some(else_type)) if then_type == else_type => then_type.clone(),
            (Some(then_type), Some(else_type)) => {
                IrType::Union(vec![then_type.clone(), else_type.clone()])
            }
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
            // Do NOT enter a new scope here; the placeholder is already in the current scope
            let init_expr = match *binding.value {
                Expression::Fn(fn_expr) => {
                    // Patch: do not enter a new scope in convert_fn, so the placeholder is visible
                    // Instead, convert params and body in the current scope
                    let id = self.next_id();
                    let mut params = Vec::new();
                    for p_def in fn_expr.params {
                        if let Pattern::Symbol(s) = p_def.pattern {
                            let param_id = self.next_id();
                            let param_type =
                                self.convert_type_annotation_option(p_def.type_annotation)?;
                            let binding_info = BindingInfo {
                                name: s.0.clone(),
                                binding_id: param_id,
                                ir_type: param_type.clone().unwrap_or(IrType::Any),
                                kind: BindingKind::Parameter,
                            };
                            self.define_binding(s.0.clone(), binding_info);
                            params.push(IrNode::Param {
                                id: param_id,
                                binding: Box::new(IrNode::VariableBinding {
                                    id: param_id,
                                    name: s.0,
                                    ir_type: param_type.clone().unwrap_or(IrType::Any),
                                    source_location: None,
                                }),
                                type_annotation: param_type.clone(),
                                ir_type: param_type.clone().unwrap_or(IrType::Any),
                                source_location: None,
                            });
                        }
                    }
                    let variadic_param = None; // TODO: handle variadic
                    let mut body = Vec::new();
                    for expr in fn_expr.body {
                        body.push(self.convert_expression(expr)?);
                    }
                    let return_type = body
                        .last()
                        .and_then(|n| n.ir_type())
                        .cloned()
                        .unwrap_or(IrType::Any);
                    let param_types: Vec<IrType> =
                        params.iter().filter_map(|p| p.ir_type()).cloned().collect();
                    let function_type = IrType::Function {
                        param_types,
                        variadic_param_type: None,
                        return_type: Box::new(return_type),
                    };
                    IrNode::Lambda {
                        id,
                        params,
                        variadic_param,
                        body,
                        captures: Vec::new(),
                        ir_type: function_type,
                        source_location: None,
                    }
                }
                _ => self.convert_expression(*binding.value)?,
            };
            let binding_type = init_expr.ir_type().cloned().unwrap_or(IrType::Any);
            let pattern_node =
                self.convert_pattern(binding.pattern, binding_id, binding_type.clone())?;
            bindings.push(IrLetBinding {
                pattern: pattern_node,
                type_annotation: binding
                    .type_annotation
                    .map(|t| self.convert_type_annotation(t))
                    .transpose()?,
                init_expr,
            });
        }

        // Process non-function bindings sequentially
        for binding in other_bindings {
            let binding_id = self.next_id();
            let pattern_clone = binding.pattern.clone();
            let init_expr = self.convert_expression(*binding.value)?;
            let binding_type = init_expr.ir_type().cloned().unwrap_or(IrType::Any);
            let pattern_node =
                self.convert_pattern(binding.pattern, binding_id, binding_type.clone())?;

            // Add all symbols from the pattern to scope after converting init expression
            let pattern_symbols = self.extract_pattern_symbols(&pattern_clone);
            for symbol_name in pattern_symbols {
                let symbol_binding_id = self.next_id();
                let binding_info = BindingInfo {
                    name: symbol_name.clone(),
                    binding_id: symbol_binding_id,
                    ir_type: binding_type.clone(),
                    kind: BindingKind::Variable,
                };
                self.define_binding(symbol_name, binding_info);
            }

            bindings.push(IrLetBinding {
                pattern: pattern_node,
                type_annotation: binding
                    .type_annotation
                    .map(|t| self.convert_type_annotation(t))
                    .transpose()?,
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
        let result_type = body_exprs
            .last()
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

    fn convert_do(&mut self, do_expr: DoExpr) -> IrConversionResult<IrNode> {
        let id = self.next_id();
        let mut expressions = Vec::new();
        for expr in do_expr.expressions {
            expressions.push(self.convert_expression(expr)?);
        }
        let ir_type = expressions
            .last()
            .and_then(|n| n.ir_type())
            .cloned()
            .unwrap_or(IrType::Nil);
        Ok(IrNode::Do {
            id,
            expressions,
            ir_type,
            source_location: None,
        })
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
                    ir_type: param_type.clone().unwrap_or(IrType::Any),
                    kind: BindingKind::Parameter,
                };
                self.define_binding(s.0.clone(), binding_info);

                params.push(IrNode::Param {
                    id: param_id,
                    binding: Box::new(IrNode::VariableBinding {
                        id: param_id,
                        name: s.0,
                        ir_type: param_type.clone().unwrap_or(IrType::Any),
                        source_location: None,
                    }),
                    type_annotation: param_type.clone(),
                    ir_type: param_type.clone().unwrap_or(IrType::Any),
                    source_location: None,
                });
            }
            // TODO: Handle other patterns in params
        } // Handle variadic parameter
        let variadic_param = if let Some(variadic_param_def) = fn_expr.variadic_param {
            if let Pattern::Symbol(s) = variadic_param_def.pattern {
                let param_id = self.next_id();
                let param_type =
                    self.convert_type_annotation_option(variadic_param_def.type_annotation)?;

                let binding_info = BindingInfo {
                    name: s.0.clone(),
                    binding_id: param_id,
                    ir_type: param_type.clone().unwrap_or(IrType::Any),
                    kind: BindingKind::Parameter,
                };
                self.define_binding(s.0.clone(), binding_info);

                Some(Box::new(IrNode::Param {
                    id: param_id,
                    binding: Box::new(IrNode::VariableBinding {
                        id: param_id,
                        name: s.0,
                        ir_type: param_type.clone().unwrap_or(IrType::Any),
                        source_location: None,
                    }),
                    type_annotation: param_type.clone(),
                    ir_type: param_type.clone().unwrap_or(IrType::Any),
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

        // Analyze captures BEFORE exiting function scope
        let captures = self.analyze_captures(&body);

        self.exit_scope();

        let return_type = if let Some(ret_type_expr) = fn_expr.return_type {
            self.convert_type_annotation(ret_type_expr)?
        } else {
            body.last()
                .and_then(|n| n.ir_type())
                .cloned()
                .unwrap_or(IrType::Any)
        };

        let param_types = params
            .iter()
            .map(|p| p.ir_type().cloned().unwrap_or(IrType::Any))
            .collect();
        let variadic_param_type = variadic_param
            .as_ref()
            .and_then(|p| p.ir_type().cloned())
            .map(|t| Box::new(t));

        Ok(IrNode::Lambda {
            id,
            params,
            variadic_param,
            body,
            captures,
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
            clauses.push(IrMatchClause {
                pattern,
                guard,
                body,
            });
        }
        let ir_type = clauses
            .first()
            .map(|c| c.body.ir_type().cloned().unwrap_or(IrType::Any))
            .unwrap_or(IrType::Any); // Simplified
        Ok(IrNode::Match {
            id,
            expression,
            clauses,
            ir_type,
            source_location: None,
        })
    }

    fn convert_vector(&mut self, exprs: Vec<Expression>) -> IrConversionResult<IrNode> {
        let id = self.next_id();
        let mut elements = Vec::new();
        for expr in exprs {
            elements.push(self.convert_expression(expr)?);
        }
        Ok(IrNode::Vector {
            id,
            elements,
            ir_type: IrType::Vector(Box::new(IrType::Any)),
            source_location: None,
        })
    }

    fn convert_map(&mut self, map: HashMap<MapKey, Expression>) -> IrConversionResult<IrNode> {
        let id = self.next_id();
        let mut entries = Vec::new();
        let mut map_type_entries = Vec::new();

        for (key, value) in map {
            let ir_key = self.convert_map_key(key.clone())?;
            let ir_value = self.convert_expression(value)?;

            if let (Some(_key_type), Some(value_type)) = (ir_key.ir_type(), ir_value.ir_type()) {
                if let IrNode::Literal {
                    value: Literal::Keyword(kw),
                    ..
                } = &ir_key
                {
                    map_type_entries.push(IrMapTypeEntry {
                        key: kw.clone(),
                        value_type: value_type.clone(),
                        optional: false,
                    });
                }
            }

            entries.push(IrMapEntry {
                key: ir_key,
                value: ir_value,
            });
        }

        let ir_type = IrType::Map {
            entries: map_type_entries,
            wildcard: None,
        };

        Ok(IrNode::Map {
            id,
            entries,
            ir_type,
            source_location: None,
        })
    }

    fn convert_map_key(&mut self, key: MapKey) -> IrConversionResult<IrNode> {
        let id = self.next_id();
        match key {
            MapKey::Keyword(k) => Ok(IrNode::Literal {
                id,
                value: Literal::Keyword(k),
                ir_type: IrType::Keyword,
                source_location: None,
            }),
            MapKey::String(s) => Ok(IrNode::Literal {
                id,
                value: Literal::String(s),
                ir_type: IrType::String,
                source_location: None,
            }),
            MapKey::Integer(i) => Ok(IrNode::Literal {
                id,
                value: Literal::Integer(i),
                ir_type: IrType::Int,
                source_location: None,
            }),
        }
    }

    fn convert_list_as_application(
        &mut self,
        exprs: Vec<Expression>,
    ) -> IrConversionResult<IrNode> {
        if exprs.is_empty() {
            return Err(IrConversionError::InvalidSpecialForm {
                form: "()".to_string(),
                message: "Empty list cannot be called".to_string(),
            });
        }
        let callee = exprs[0].clone();
        let arguments = exprs[1..].to_vec();
        self.convert_function_call(callee, arguments)
    }

    fn convert_try_catch(&mut self, try_expr: TryCatchExpr) -> IrConversionResult<IrNode> {
        let id = self.next_id();

        let try_body_expressions = try_expr
            .try_body
            .into_iter()
            .map(|e| self.convert_expression(e))
            .collect::<Result<_, _>>()?;

        let mut catch_clauses = Vec::new();
        for clause in try_expr.catch_clauses {
            let pattern = match clause.pattern {
                CatchPattern::Keyword(k) => IrPattern::Literal(Literal::Keyword(k)),
                CatchPattern::Type(t) => IrPattern::Type(self.convert_type_annotation(t)?),
                CatchPattern::Symbol(s) => IrPattern::Variable(s.0),
                CatchPattern::Wildcard => IrPattern::Wildcard,
            };
            let body = clause
                .body
                .into_iter()
                .map(|e| self.convert_expression(e))
                .collect::<Result<_, _>>()?;
            catch_clauses.push(IrCatchClause {
                error_pattern: pattern,
                binding: Some(clause.binding.0),
                body,
            });
        }

        let finally_body = if let Some(fb) = try_expr.finally_body {
            Some(
                fb.into_iter()
                    .map(|e| self.convert_expression(e))
                    .collect::<Result<_, _>>()?,
            )
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
        Ok(IrNode::Parallel {
            id,
            bindings,
            ir_type: IrType::Vector(Box::new(IrType::Any)),
            source_location: None,
        })
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

        let body_expressions = with_expr
            .body
            .into_iter()
            .map(|e| self.convert_expression(e))
            .collect::<Result<_, _>>()?;

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
        let values = log_expr
            .values
            .into_iter()
            .map(|e| self.convert_expression(e))
            .collect::<Result<_, _>>()?;
        Ok(IrNode::LogStep {
            id,
            values,
            level: log_expr.level.unwrap_or(Keyword("info".to_string())),
            location: log_expr.location,
            ir_type: IrType::Nil,
            source_location: None,
        })
    }

    fn convert_discover_agents(
        &mut self,
        discover_expr: DiscoverAgentsExpr,
    ) -> IrConversionResult<IrNode> {
        let id = self.next_id();
        let criteria = Box::new(self.convert_expression(*discover_expr.criteria)?);
        // TODO: Handle options
        Ok(IrNode::DiscoverAgents {
            id,
            criteria,
            ir_type: IrType::Vector(Box::new(IrType::Any)),
            source_location: None,
        })
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
            delegation_hint: defn_expr.delegation_hint,
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

    fn convert_defstruct(&mut self, defstruct_expr: DefstructExpr) -> IrConversionResult<IrNode> {
        let id = self.next_id();
        let name = defstruct_expr.name.0.clone();
        
        // Create the struct type from the field definitions
        let mut map_entries = Vec::new();
        for field in &defstruct_expr.fields {
            // Convert the field type annotation to IrType
            let field_type = self.convert_type_annotation(field.field_type.clone())?;
            map_entries.push(IrMapTypeEntry {
                key: field.key.clone(),
                value_type: field_type,
                optional: false, // defstruct fields are required by default
            });
        }
        
        let struct_type = IrType::Map {
            entries: map_entries,
            wildcard: None, // defstruct creates closed maps
        };
        
        // Create a constructor function that validates inputs
        // The constructor takes a map and validates it against the struct definition
        let param_id = self.next_id();
        let constructor_param = IrNode::Param {
            id: param_id,
            binding: Box::new(IrNode::VariableBinding {
                id: param_id,
                name: "input_map".to_string(),
                ir_type: IrType::Map { entries: vec![], wildcard: Some(Box::new(IrType::Any)) },
                source_location: None,
            }),
            type_annotation: Some(IrType::Map { entries: vec![], wildcard: Some(Box::new(IrType::Any)) }),
            ir_type: IrType::Map { entries: vec![], wildcard: Some(Box::new(IrType::Any)) },
            source_location: None,
        };
        
        // Constructor body: for now, just return the input map
        // In a full implementation, this would contain validation logic
        let constructor_body = vec![
            IrNode::VariableRef {
                id: self.next_id(),
                name: "input_map".to_string(),
                binding_id: param_id,
                ir_type: IrType::Map { entries: vec![], wildcard: Some(Box::new(IrType::Any)) },
                source_location: None,
            }
        ];
        
        // TODO: Full defstruct IR implementation should include:
        // 1. Field validation logic in constructor body (instead of just returning input_map)
        // 2. Proper typed field access methods/getters for each struct field
        // 3. Type checking for each field against its declared type annotation
        // 4. Integration with type refinement system for advanced constraints
        // 5. Compile-time optimizations for known struct field access patterns
        // 6. Error handling and reporting for invalid field values
        // 7. Support for default values and optional fields
        // 8. Immutable update operations (assoc, dissoc, update-in style)
        
        // Create the constructor function type
        let constructor_type = IrType::Function {
            param_types: vec![IrType::Map { entries: vec![], wildcard: Some(Box::new(IrType::Any)) }],
            variadic_param_type: None,
            return_type: Box::new(struct_type.clone()),
        };
        
        // Create the constructor lambda
        let constructor_lambda = IrNode::Lambda {
            id: self.next_id(),
            params: vec![constructor_param],
            variadic_param: None,
            body: constructor_body,
            captures: Vec::new(),
            ir_type: constructor_type.clone(),
            source_location: None,
        };
        
        // Define the constructor function in the environment
        let binding_info = BindingInfo {
            name: name.clone(),
            binding_id: id,
            ir_type: constructor_type.clone(),
            kind: BindingKind::Function,
        };
        self.define_binding(name.clone(), binding_info);
        
        // Return a function definition that binds the constructor to the struct name
        Ok(IrNode::FunctionDef {
            id,
            name: name.clone(),
            lambda: Box::new(constructor_lambda),
            ir_type: constructor_type,
            source_location: None,
        })
    }

    /// Convert pattern to IR node
    fn convert_pattern(
        &mut self,
        pattern: Pattern,
        binding_id: NodeId,
        ir_type: IrType,
    ) -> IrConversionResult<IrNode> {
        match pattern {
            Pattern::Symbol(sym) => Ok(IrNode::VariableBinding {
                id: binding_id,
                name: sym.0,
                ir_type,
                source_location: None,
            }),
            Pattern::Wildcard => Ok(IrNode::VariableBinding {
                id: binding_id,
                name: "_".to_string(),
                ir_type,
                source_location: None,
            }),
            Pattern::VectorDestructuring {
                elements,
                rest,
                as_symbol: _,
            } => {
                // Convert to IrPattern
                let ir_pattern = IrPattern::Vector {
                    elements: elements
                        .into_iter()
                        .map(|p| self.convert_pattern_to_irpattern(p))
                        .collect::<Result<_, _>>()?,
                    rest: rest.map(|s| s.0),
                };
                Ok(IrNode::Destructure {
                    id: binding_id,
                    pattern: ir_pattern,
                    value: Box::new(IrNode::VariableRef {
                        id: binding_id + 1,
                        name: format!("__destructure_value_{}", binding_id),
                        binding_id,
                        ir_type: ir_type.clone(),
                        source_location: None,
                    }),
                    ir_type,
                    source_location: None,
                })
            }
            Pattern::MapDestructuring {
                entries,
                rest,
                as_symbol: _,
            } => {
                let mut ir_entries = Vec::new();
                for entry in entries {
                    match entry {
                        MapDestructuringEntry::KeyBinding { key, pattern } => {
                            let pattern = self.convert_pattern_to_irpattern(*pattern)?;
                            ir_entries.push(IrMapPatternEntry { key, pattern });
                        }
                        MapDestructuringEntry::Keys(symbols) => {
                            for sym in symbols {
                                let sym_text = sym.0.clone();
                                // Keywords are represented without leading ':' in AST/Values
                                ir_entries.push(IrMapPatternEntry {
                                    key: MapKey::Keyword(Keyword(sym_text.clone())),
                                    pattern: IrPattern::Variable(sym_text),
                                });
                            }
                        }
                    }
                }
                let ir_pattern = IrPattern::Map {
                    entries: ir_entries,
                    rest: rest.map(|s| s.0),
                };
                Ok(IrNode::Destructure {
                    id: binding_id,
                    pattern: ir_pattern,
                    value: Box::new(IrNode::VariableRef {
                        id: binding_id + 1,
                        name: format!("__destructure_value_{}", binding_id),
                        binding_id,
                        ir_type: ir_type.clone(),
                        source_location: None,
                    }),
                    ir_type,
                    source_location: None,
                })
            }
        }
    }

    fn convert_pattern_to_irpattern(&mut self, pattern: Pattern) -> IrConversionResult<IrPattern> {
        match pattern {
            Pattern::Symbol(sym) => Ok(IrPattern::Variable(sym.0)),
            Pattern::Wildcard => Ok(IrPattern::Wildcard),
            Pattern::VectorDestructuring {
                elements,
                rest,
                as_symbol: _,
            } => Ok(IrPattern::Vector {
                elements: elements
                    .into_iter()
                    .map(|p| self.convert_pattern_to_irpattern(p))
                    .collect::<Result<_, _>>()?,
                rest: rest.map(|s| s.0),
            }),
            Pattern::MapDestructuring {
                entries,
                rest,
                as_symbol: _,
            } => {
                let mut ir_entries = Vec::new();
                for entry in entries {
                    match entry {
                        MapDestructuringEntry::KeyBinding { key, pattern } => {
                            let pattern = self.convert_pattern_to_irpattern(*pattern)?;
                            ir_entries.push(IrMapPatternEntry { key, pattern });
                        }
                        MapDestructuringEntry::Keys(symbols) => {
                            for sym in symbols {
                                let sym_text = sym.0.clone();
                                // Keywords are represented without leading ':' in AST/Values
                                ir_entries.push(IrMapPatternEntry {
                                    key: MapKey::Keyword(Keyword(sym_text.clone())),
                                    pattern: IrPattern::Variable(sym_text),
                                });
                            }
                        }
                    }
                }
                Ok(IrPattern::Map {
                    entries: ir_entries,
                    rest: rest.map(|s| s.0),
                })
            }
        }
    }

    /// Analyze which variables from outer scopes are captured by a lambda
    fn analyze_captures(&self, body: &[IrNode]) -> Vec<IrCapture> {
        let mut captures = Vec::new();
        let mut captured_names = std::collections::HashSet::new();

        // Collect all variable references in the body
        for node in body {
            self.collect_variable_references(node, &mut captured_names);
        }

        // Convert captured names to IrCapture objects
        for name in captured_names {
            if let Some(binding_info) = self.lookup_symbol(&name) {
                captures.push(IrCapture {
                    name: name.clone(),
                    binding_id: binding_info.binding_id,
                    ir_type: binding_info.ir_type.clone(),
                });
            }
        }

        captures
    }

    /// Recursively collect variable references from an IR node
    fn collect_variable_references(
        &self,
        node: &IrNode,
        captured_names: &mut std::collections::HashSet<String>,
    ) {
        match node {
            IrNode::VariableRef { name, .. } => {
                // Check if this variable is from an outer scope
                if let Some(binding_info) = self.lookup_symbol(name) {
                    // If it's not a parameter (which would be in current scope), it's captured
                    if binding_info.kind != BindingKind::Parameter {
                        captured_names.insert(name.clone());
                    }
                }
            }
            IrNode::Lambda {  .. } => {
                // Don't traverse into nested lambdas for capture analysis
                // Each lambda should do its own capture analysis
                return;
            }
            IrNode::Apply {
                function,
                arguments,
                ..
            } => {
                self.collect_variable_references(function, captured_names);
                for arg in arguments {
                    self.collect_variable_references(arg, captured_names);
                }
            }
            IrNode::If {
                condition,
                then_branch,
                else_branch,
                ..
            } => {
                self.collect_variable_references(condition, captured_names);
                self.collect_variable_references(then_branch, captured_names);
                if let Some(else_branch) = else_branch {
                    self.collect_variable_references(else_branch, captured_names);
                }
            }
            IrNode::Let { bindings, body, .. } => {
                for binding in bindings {
                    self.collect_variable_references(&binding.init_expr, captured_names);
                }
                for expr in body {
                    self.collect_variable_references(expr, captured_names);
                }
            }
            IrNode::Do { expressions, .. } => {
                for expr in expressions {
                    self.collect_variable_references(expr, captured_names);
                }
            }
            IrNode::Vector { elements, .. } => {
                for element in elements {
                    self.collect_variable_references(element, captured_names);
                }
            }
            IrNode::Map { entries, .. } => {
                for entry in entries {
                    self.collect_variable_references(&entry.key, captured_names);
                    self.collect_variable_references(&entry.value, captured_names);
                }
            }
            IrNode::TryCatch {
                try_body,
                catch_clauses,
                finally_body,
                ..
            } => {
                for expr in try_body {
                    self.collect_variable_references(expr, captured_names);
                }
                for clause in catch_clauses {
                    for expr in &clause.body {
                        self.collect_variable_references(expr, captured_names);
                    }
                }
                if let Some(finally_body) = finally_body {
                    for expr in finally_body {
                        self.collect_variable_references(expr, captured_names);
                    }
                }
            }
            IrNode::WithResource {
                binding,
                init_expr,
                body,
                ..
            } => {
                self.collect_variable_references(binding, captured_names);
                self.collect_variable_references(init_expr, captured_names);
                for expr in body {
                    self.collect_variable_references(expr, captured_names);
                }
            }
            IrNode::LogStep { values, .. } => {
                for value in values {
                    self.collect_variable_references(value, captured_names);
                }
            }
            IrNode::Step { params, body, .. } => {
                if let Some(p) = params {
                    self.collect_variable_references(p, captured_names);
                }
                for expr in body {
                    self.collect_variable_references(expr, captured_names);
                }
            }
            IrNode::DiscoverAgents { criteria, .. } => {
                self.collect_variable_references(criteria, captured_names);
            }
            IrNode::VariableDef { init_expr, .. } => {
                self.collect_variable_references(init_expr, captured_names);
            }
            IrNode::FunctionDef { lambda, .. } => {
                self.collect_variable_references(lambda, captured_names);
            }
            IrNode::Task {
                intent,
                contracts,
                plan,
                execution_trace,
                ..
            } => {
                self.collect_variable_references(intent, captured_names);
                self.collect_variable_references(contracts, captured_names);
                self.collect_variable_references(plan, captured_names);
                for expr in execution_trace {
                    self.collect_variable_references(expr, captured_names);
                }
            }
            IrNode::Destructure { value, .. } => {
                self.collect_variable_references(value, captured_names);
            }
            // These nodes don't contain variable references
            IrNode::Literal { .. }
            | IrNode::VariableBinding { .. }
            | IrNode::ResourceRef { .. }
            | IrNode::QualifiedSymbolRef { .. }
            | IrNode::Param { .. }
            | IrNode::Match { .. }
            | IrNode::Parallel { .. }
            | IrNode::Module { .. }
            | IrNode::Import { .. }

            | IrNode::Program { .. } => {
                // No variable references in these node types
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

    /// Convert TypeExpr to IrType
    fn convert_type_annotation(&mut self, t: TypeExpr) -> IrConversionResult<IrType> {
        match t {
            TypeExpr::Primitive(prim) => Ok(match prim {
                PrimitiveType::Int => IrType::Int,
                PrimitiveType::Float => IrType::Float,
                PrimitiveType::String => IrType::String,
                PrimitiveType::Bool => IrType::Bool,
                PrimitiveType::Nil => IrType::Nil,
                PrimitiveType::Keyword => IrType::Keyword,
                PrimitiveType::Symbol => IrType::Symbol,
                PrimitiveType::Custom(kw) => IrType::TypeRef(kw.0), // Custom types become type references
            }),
            TypeExpr::Any => Ok(IrType::Any),
            TypeExpr::Never => Ok(IrType::Never),
            TypeExpr::Alias(symbol) => Ok(IrType::TypeRef(symbol.0)),
            TypeExpr::Vector(element_type) => {
                let element_ir_type = self.convert_type_annotation(*element_type)?;
                Ok(IrType::Vector(Box::new(element_ir_type)))
            },
            TypeExpr::Tuple(element_types) => {
                let ir_types: Result<Vec<_>, _> = element_types
                    .into_iter()
                    .map(|t| self.convert_type_annotation(t))
                    .collect();
                Ok(IrType::Tuple(ir_types?))
            },
            TypeExpr::Map { entries, wildcard } => {
                let mut ir_entries = Vec::new();
                for entry in entries {
                    let value_type = self.convert_type_annotation(*entry.value_type)?;
                    ir_entries.push(IrMapTypeEntry {
                        key: entry.key,
                        value_type,
                        optional: entry.optional,
                    });
                }
                let wildcard_type = wildcard
                    .map(|w| self.convert_type_annotation(*w))
                    .transpose()?
                    .map(Box::new);
                Ok(IrType::Map {
                    entries: ir_entries,
                    wildcard: wildcard_type,
                })
            },
            TypeExpr::Function { param_types, variadic_param_type, return_type } => {
                let ir_param_types: Result<Vec<_>, _> = param_types
                    .into_iter()
                    .map(|pt| match pt {
                        ParamType::Simple(t) => self.convert_type_annotation(*t),
                    })
                    .collect();
                let ir_variadic_type = variadic_param_type
                    .map(|vt| self.convert_type_annotation(*vt))
                    .transpose()?
                    .map(Box::new);
                let ir_return_type = Box::new(self.convert_type_annotation(*return_type)?);
                Ok(IrType::Function {
                    param_types: ir_param_types?,
                    variadic_param_type: ir_variadic_type,
                    return_type: ir_return_type,
                })
            },
            TypeExpr::Union(types) => {
                let ir_types: Result<Vec<_>, _> = types
                    .into_iter()
                    .map(|t| self.convert_type_annotation(t))
                    .collect();
                Ok(IrType::Union(ir_types?))
            },
            TypeExpr::Intersection(types) => {
                let ir_types: Result<Vec<_>, _> = types
                    .into_iter()
                    .map(|t| self.convert_type_annotation(t))
                    .collect();
                Ok(IrType::Intersection(ir_types?))
            },
            TypeExpr::Resource(symbol) => Ok(IrType::Resource(symbol.0)),
            TypeExpr::Literal(lit) => Ok(IrType::LiteralValue(lit)),
            TypeExpr::Optional(inner_type) => {
                let inner_ir_type = self.convert_type_annotation(*inner_type)?;
                Ok(IrType::Union(vec![inner_ir_type, IrType::Nil]))
            },
            // For complex types that don't have direct IR equivalents, fall back to Any for now
            TypeExpr::Array { element_type: _, shape: _ } => {
                // TODO: Implement proper array type support in IR
                Ok(IrType::Any)
            },
            TypeExpr::Refined { base_type, predicates: _ } => {
                // For now, just convert the base type and ignore predicates
                // TODO: Implement refined type support in IR
                self.convert_type_annotation(*base_type)
            },
            TypeExpr::Enum(literals) => {
                // Convert enum to union of literal types
                let literal_types: Vec<IrType> = literals
                    .into_iter()
                    .map(IrType::LiteralValue)
                    .collect();
                Ok(IrType::Union(literal_types))
            },
        }
    }

    /// TODO: Implement type annotation option conversion logic
    fn convert_type_annotation_option(
        &mut self,
        t: Option<TypeExpr>,
    ) -> IrConversionResult<Option<IrType>> {
        // TODO: Properly convert/resolve type annotation options
        match t {
            Some(te) => Ok(Some(self.convert_type_annotation(te)?)),
            None => Ok(None),
        }
    }

    fn convert_match_pattern(&mut self, _pat: MatchPattern) -> IrConversionResult<IrPattern> {
        // TODO: Implement real match pattern conversion
        Ok(IrPattern::Wildcard)
    }

    /// Extract all symbol names from a pattern (including nested destructuring)
    fn extract_pattern_symbols(&self, pattern: &Pattern) -> Vec<String> {
        match pattern {
            Pattern::Symbol(sym) => vec![sym.0.clone()],
            Pattern::Wildcard => vec![], // Wildcards don't bind anything
            Pattern::VectorDestructuring { elements, rest, .. } => {
                let mut symbols = Vec::new();
                for element in elements {
                    symbols.extend(self.extract_pattern_symbols(element));
                }
                if let Some(rest_symbol) = rest {
                    symbols.push(rest_symbol.0.clone());
                }
                symbols
            }
            Pattern::MapDestructuring { entries, rest, .. } => {
                let mut symbols = Vec::new();
                for entry in entries {
                    match entry {
                        MapDestructuringEntry::KeyBinding { pattern, .. } => {
                            symbols.extend(self.extract_pattern_symbols(pattern));
                        }
                        MapDestructuringEntry::Keys(syms) => {
                            for sym in syms {
                                symbols.push(sym.0.clone());
                            }
                        }
                    }
                }
                if let Some(rest_symbol) = rest {
                    symbols.push(rest_symbol.0.clone());
                }
                symbols
            }
        }
    }

    fn convert_for(&mut self, for_expr: ForExpr) -> IrConversionResult<IrNode> {
        // For now, return an error indicating for expressions are not supported in IR conversion
        Err(IrConversionError::InvalidSpecialForm {
            form: "for".to_string(),
            message: "For expressions not yet implemented in IR converter".to_string(),
        })
    }
}
