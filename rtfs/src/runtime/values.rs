// Runtime value system for RTFS
// Represents values during execution (different from AST which represents parsed code)

use crate::ast::{Expression, Keyword, Literal, MapKey, Symbol};
use crate::ir::core::IrNode;
use crate::runtime::environment::Environment;
use crate::runtime::error::RuntimeResult;
use crate::runtime::Evaluator;
use crate::runtime::IrEnvironment;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, RwLock};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Value {
    Nil,
    Boolean(bool),
    Integer(i64),
    Float(f64),
    String(String),
    Timestamp(String),
    Uuid(String),
    ResourceHandle(String),
    /// Removed atom functionality - use host state capabilities instead
    Symbol(Symbol),
    Keyword(Keyword),
    Vector(Vec<Value>),
    List(Vec<Value>),
    Map(HashMap<MapKey, Value>),
    #[serde(skip_serializing, skip_deserializing)]
    Function(Function),
    #[serde(skip_serializing, skip_deserializing)]
    FunctionPlaceholder(Arc<RwLock<Value>>),
    Error(ErrorValue),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ErrorValue {
    pub message: String,
    pub stack_trace: Option<Vec<String>>,
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Nil => write!(f, "nil"),
            Value::Boolean(b) => write!(f, "{}", b),
            Value::Integer(i) => write!(f, "{}", i),
            Value::Float(fl) => write!(f, "{}", fl),
            Value::String(s) => write!(f, "\"{}\"", s),
            Value::Timestamp(t) => write!(f, "#timestamp(\"{}\")", t),
            Value::Uuid(u) => write!(f, "#uuid(\"{}\")", u),
            Value::ResourceHandle(rh) => write!(f, "#resource-handle(\"{}\")", rh),
            Value::Symbol(s) => write!(f, "{}", s.0),
            Value::Keyword(k) => write!(f, ":{}", k.0),
            Value::Vector(v) => {
                let items: Vec<String> = v.iter().map(|item| format!("{}", item)).collect();
                write!(f, "[{}]", items.join(" "))
            }
            Value::List(l) => {
                let items: Vec<String> = l.iter().map(|item| format!("{}", item)).collect();
                write!(f, "({})", items.join(" "))
            }
            Value::Map(m) => {
                let items: Vec<String> = m.iter().map(|(k, v)| format!("{} {}", k, v)).collect();
                write!(f, "{{{}}}", items.join(" "))
            }
            Value::Function(_) => write!(f, "#<function>"),
            Value::FunctionPlaceholder(_) => write!(f, "#<function-placeholder>"),
            Value::Error(e) => write!(f, "#<error: {}>", e.message),
        }
    }
}

impl Value {
    pub fn is_truthy(&self) -> bool {
        match self {
            Value::Boolean(b) => *b,
            Value::Nil => false,
            _ => true,
        }
    }

    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Nil => "nil",
            Value::Boolean(_) => "boolean",
            Value::Integer(_) => "integer",
            Value::Float(_) => "float",
            Value::String(_) => "string",
            Value::Timestamp(_) => "timestamp",
            Value::Uuid(_) => "uuid",
            Value::ResourceHandle(_) => "resource-handle",
            Value::Symbol(_) => "symbol",
            Value::Keyword(_) => "keyword",
            Value::Vector(_) => "vector",
            Value::List(_) => "list",
            Value::Map(_) => "map",
            Value::Function(_) => "function",
            Value::FunctionPlaceholder(_) => "function-placeholder",
            Value::Error(_) => "error",
        }
    }

    /// Compare two values for ordering
    pub fn compare(&self, other: &Value) -> std::cmp::Ordering {
        match (self, other) {
            (Value::Nil, Value::Nil) => std::cmp::Ordering::Equal,
            (Value::Nil, _) => std::cmp::Ordering::Less,
            (_, Value::Nil) => std::cmp::Ordering::Greater,

            (Value::Boolean(a), Value::Boolean(b)) => a.cmp(b),
            (Value::Boolean(_), _) => std::cmp::Ordering::Less,
            (_, Value::Boolean(_)) => std::cmp::Ordering::Greater,

            (Value::Integer(a), Value::Integer(b)) => a.cmp(b),
            (Value::Integer(a), Value::Float(b)) => (*a as f64)
                .partial_cmp(b)
                .unwrap_or(std::cmp::Ordering::Equal),
            (Value::Integer(_), _) => std::cmp::Ordering::Less,
            (Value::Float(a), Value::Integer(b)) => a
                .partial_cmp(&(*b as f64))
                .unwrap_or(std::cmp::Ordering::Equal),
            (Value::Float(a), Value::Float(b)) => {
                a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal)
            }
            (Value::Float(_), _) => std::cmp::Ordering::Less,
            (_, Value::Integer(_) | Value::Float(_)) => std::cmp::Ordering::Greater,

            (Value::String(a), Value::String(b)) => a.cmp(b),
            (Value::String(_), _) => std::cmp::Ordering::Less,
            (_, Value::String(_)) => std::cmp::Ordering::Greater,

            (Value::Keyword(a), Value::Keyword(b)) => a.0.cmp(&b.0),
            (Value::Keyword(_), _) => std::cmp::Ordering::Less,
            (_, Value::Keyword(_)) => std::cmp::Ordering::Greater,

            (Value::Symbol(a), Value::Symbol(b)) => a.0.cmp(&b.0),
            (Value::Symbol(_), _) => std::cmp::Ordering::Less,
            (_, Value::Symbol(_)) => std::cmp::Ordering::Greater,

            (Value::Vector(a), Value::Vector(b)) => {
                // Compare vectors element by element
                for (a_elem, b_elem) in a.iter().zip(b.iter()) {
                    match a_elem.compare(b_elem) {
                        std::cmp::Ordering::Equal => continue,
                        other => return other,
                    }
                }
                a.len().cmp(&b.len())
            }
            (Value::Vector(_), _) => std::cmp::Ordering::Less,
            (_, Value::Vector(_)) => std::cmp::Ordering::Greater,

            (Value::List(a), Value::List(b)) => {
                // Compare lists element by element
                for (a_elem, b_elem) in a.iter().zip(b.iter()) {
                    match a_elem.compare(b_elem) {
                        std::cmp::Ordering::Equal => continue,
                        other => return other,
                    }
                }
                a.len().cmp(&b.len())
            }
            (Value::List(_), _) => std::cmp::Ordering::Less,
            (_, Value::List(_)) => std::cmp::Ordering::Greater,

            (Value::Map(a), Value::Map(b)) => {
                // Convert maps to sorted vectors for comparison
                let mut a_items: Vec<_> = a.iter().collect();
                let mut b_items: Vec<_> = b.iter().collect();

                // Helper function to convert MapKey to string
                fn map_key_to_string(key: &crate::ast::MapKey) -> String {
                    match key {
                        crate::ast::MapKey::String(s) => format!("s:{}", s),
                        crate::ast::MapKey::Keyword(k) => format!("k:{}", k.0),
                        crate::ast::MapKey::Integer(i) => format!("i:{}", i),
                    }
                }

                a_items
                    .sort_by(|(k1, _), (k2, _)| map_key_to_string(k1).cmp(&map_key_to_string(k2)));
                b_items
                    .sort_by(|(k1, _), (k2, _)| map_key_to_string(k1).cmp(&map_key_to_string(k2)));

                // Compare sorted items
                for (a_item, b_item) in a_items.iter().zip(b_items.iter()) {
                    match map_key_to_string(a_item.0).cmp(&map_key_to_string(b_item.0)) {
                        std::cmp::Ordering::Equal => match a_item.1.compare(b_item.1) {
                            std::cmp::Ordering::Equal => continue,
                            other => return other,
                        },
                        other => return other,
                    }
                }
                a_items.len().cmp(&b_items.len())
            }
            (Value::Map(_), _) => std::cmp::Ordering::Less,
            (_, Value::Map(_)) => std::cmp::Ordering::Greater,

            // For other types, use string representation
            _ => self.to_string().cmp(&other.to_string()),
        }
    }

    pub fn as_number(&self) -> Option<f64> {
        match self {
            Value::Integer(i) => Some(*i as f64),
            Value::Float(f) => Some(*f),
            _ => None,
        }
    }

    pub fn as_string(&self) -> Option<&str> {
        match self {
            Value::String(s) => Some(s),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Arity {
    Fixed(usize),
    Variadic(usize),     // minimum number of arguments
    Range(usize, usize), // min, max
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ResourceState {
    Active,
    Released,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ResourceHandle {
    pub id: String,
    pub state: ResourceState,
}

#[derive(Clone)]
pub struct BuiltinFunction {
    pub name: String,
    pub arity: Arity,
    pub func: Arc<dyn Fn(Vec<Value>) -> RuntimeResult<Value> + Send + Sync>,
}

#[derive(Clone)]
pub struct BuiltinFunctionWithContext {
    pub name: String,
    pub arity: Arity,
    pub func:
        Arc<dyn Fn(Vec<Value>, &Evaluator, &mut Environment) -> RuntimeResult<Value> + Send + Sync>,
}

#[derive(Clone)]
pub enum Function {
    Builtin(BuiltinFunction),
    BuiltinWithContext(BuiltinFunctionWithContext),
    Closure(Arc<Closure>),
    Native(BuiltinFunction),
    Ir(Arc<IrLambda>),
}

impl Function {
    pub fn new_closure(
        params: Vec<Symbol>,
        param_patterns: Vec<crate::ast::Pattern>,
        param_type_annotations: Vec<Option<crate::ast::TypeExpr>>,
        variadic_param: Option<Symbol>,
        variadic_param_type: Option<crate::ast::TypeExpr>,
        body: Box<Expression>,
        env: Arc<Environment>,
        delegation_hint: Option<crate::ast::DelegationHint>,
        return_type: Option<crate::ast::TypeExpr>,
    ) -> Function {
        Function::Closure(Arc::new(Closure {
            params,
            param_patterns,
            param_type_annotations,
            variadic_param,
            variadic_param_type,
            body,
            env,
            delegation_hint,
            return_type,
        }))
    }

    pub fn new_ir_lambda(
        params: Vec<IrNode>,
        param_type_annotations: Vec<Option<crate::ir::core::IrType>>,
        variadic_param: Option<Box<IrNode>>,
        variadic_param_type: Option<crate::ir::core::IrType>,
        body: Vec<IrNode>,
        return_type: Option<crate::ir::core::IrType>,
        closure_env: Box<IrEnvironment>,
    ) -> Function {
        Function::Ir(Arc::new(IrLambda {
            params,
            param_type_annotations,
            variadic_param,
            variadic_param_type,
            body,
            return_type,
            closure_env,
        }))
    }
}

impl fmt::Debug for Function {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Function::Builtin(_) => write!(f, "BuiltinFunction"),
            Function::BuiltinWithContext(_) => write!(f, "BuiltinFunctionWithContext"),
            Function::Closure(_) => write!(f, "Closure"),
            Function::Native(_) => write!(f, "NativeFunction"),
            Function::Ir(_) => write!(f, "Closure"), // Display IR functions as Closure for compatibility
        }
    }
}

impl PartialEq for Function {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Function::Builtin(a), Function::Builtin(b)) => a.name == b.name && a.arity == b.arity,
            (Function::Native(a), Function::Native(b)) => a.name == b.name && a.arity == b.arity,
            (Function::BuiltinWithContext(a), Function::BuiltinWithContext(b)) => {
                a.name == b.name && a.arity == b.arity
            }
            (Function::Closure(a), Function::Closure(b)) => Arc::ptr_eq(a, b),
            (Function::Ir(a), Function::Ir(b)) => Arc::ptr_eq(a, b),
            _ => false,
        }
    }
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        use Value::*;
        match (self, other) {
            (Nil, Nil) => true,
            (Boolean(a), Boolean(b)) => a == b,
            (Integer(a), Integer(b)) => a == b,
            (Float(a), Float(b)) => a == b,
            (String(a), String(b)) => a == b,
            (Timestamp(a), Timestamp(b)) => a == b,
            (Uuid(a), Uuid(b)) => a == b,
            (ResourceHandle(a), ResourceHandle(b)) => a == b,
            (Symbol(a), Symbol(b)) => a == b,
            (Keyword(a), Keyword(b)) => a.0 == b.0,
            (Vector(a), Vector(b)) => a == b,
            (List(a), List(b)) => a == b,
            (Map(a), Map(b)) => a == b,
            (Error(a), Error(b)) => a == b,
            (Function(a), Function(b)) => a == b,
            (FunctionPlaceholder(a), FunctionPlaceholder(b)) => {
                // If pointers equal, they're equal
                if Arc::ptr_eq(a, b) {
                    return true;
                }

                // Acquire read locks in address order to avoid deadlocks
                let pa = Arc::as_ptr(a) as usize;
                let pb = Arc::as_ptr(b) as usize;

                if pa <= pb {
                    let ra = a.read().unwrap_or_else(|e| e.into_inner());
                    let rb = b.read().unwrap_or_else(|e| e.into_inner());
                    *ra == *rb
                } else {
                    let rb = b.read().unwrap_or_else(|e| e.into_inner());
                    let ra = a.read().unwrap_or_else(|e| e.into_inner());
                    *ra == *rb
                }
            }
            _ => false,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Closure {
    pub params: Vec<Symbol>,
    // Full parameter patterns to support destructuring during invocation
    pub param_patterns: Vec<crate::ast::Pattern>,
    // Optional type annotations for each parameter (aligned with param_patterns)
    pub param_type_annotations: Vec<Option<crate::ast::TypeExpr>>,
    // Variadic parameter symbol for functions like [& rest]
    pub variadic_param: Option<Symbol>,
    // Optional type annotation for the variadic parameter
    pub variadic_param_type: Option<crate::ast::TypeExpr>,
    pub body: Box<Expression>,
    pub env: Arc<Environment>,
    pub delegation_hint: Option<crate::ast::DelegationHint>,
    // Optional return type annotation for the function
    pub return_type: Option<crate::ast::TypeExpr>,
}

#[derive(Clone, Debug)]
pub struct IrLambda {
    pub params: Vec<IrNode>,
    pub param_type_annotations: Vec<Option<crate::ir::core::IrType>>,
    pub variadic_param: Option<Box<IrNode>>,
    pub variadic_param_type: Option<crate::ir::core::IrType>,
    pub body: Vec<IrNode>,
    pub return_type: Option<crate::ir::core::IrType>,
    pub closure_env: Box<IrEnvironment>,
}

impl From<Expression> for Value {
    fn from(expr: Expression) -> Self {
        match expr {
            Expression::Literal(lit) => Value::from(lit),
            Expression::Symbol(sym) => Value::Symbol(sym),
            Expression::List(exprs) => {
                let values = exprs.into_iter().map(Value::from).collect();
                Value::Vector(values)
            }
            Expression::Vector(exprs) => {
                let values = exprs.into_iter().map(Value::from).collect();
                Value::Vector(values)
            }
            Expression::Map(map) => {
                let values = map.into_iter().map(|(k, v)| (k, Value::from(v))).collect();
                Value::Map(values)
            }
            Expression::FunctionCall { callee, arguments } => {
                // For now, return a placeholder function value
                // In a real implementation, this would evaluate the function call
                Value::String(format!(
                    "#<function-call: {:?} {}>",
                    callee,
                    arguments.len()
                ))
            }
            Expression::If(_) => {
                // For now, return a placeholder for if expressions
                Value::String("#<if-expression>".to_string())
            }
            Expression::Let(_) => {
                // For now, return a placeholder for let expressions
                Value::String("#<let-expression>".to_string())
            }
            Expression::Do(_) => {
                // For now, return a placeholder for do expressions
                Value::String("#<do-expression>".to_string())
            }
            Expression::Fn(_) => {
                // For now, return a placeholder for function expressions
                Value::String("#<fn-expression>".to_string())
            }
            Expression::Def(def_expr) => {
                // For now, return a placeholder for def expressions
                Value::String(format!("#<def: {}>", def_expr.symbol.0))
            }
            Expression::Defn(defn_expr) => {
                // For now, return a placeholder for defn expressions
                Value::String(format!("#<defn: {}>", defn_expr.name.0))
            }
            Expression::TryCatch(_) => {
                // For now, return a placeholder for try-catch expressions
                Value::String("#<try-catch>".to_string())
            }
            Expression::Match(_) => {
                // For now, return a placeholder for match expressions
                Value::String("#<match>".to_string())
            }
            Expression::ResourceRef(resource_name) => {
                // Return the resource name as a string
                Value::String(resource_name)
            }
            Expression::Defstruct(_defstruct_expr) => {
                // For now, return a placeholder for defstruct expressions
                Value::String("#<defstruct>".to_string())
            }
            Expression::For(_for_expr) => {
                // For now, return a placeholder for for expressions
                Value::String("#<for>".to_string())
            }
            Expression::Deref(_expr) => {
                // For now, return a placeholder for deref expressions
                Value::String("#<deref>".to_string())
            }
            Expression::WithMetadata { meta: _meta, expr } => {
                // Return the inner expression's value; metadata is not converted to runtime values here.
                Value::from(*expr)
            }
            // Macro-related expressions should have been expanded before evaluation
            Expression::Quasiquote(_)
            | Expression::Unquote(_)
            | Expression::UnquoteSplicing(_)
            | Expression::Defmacro(_) => Value::String("#<macro-expression>".to_string()),
        }
    }
}

impl From<Literal> for Value {
    fn from(lit: Literal) -> Self {
        match lit {
            Literal::Integer(n) => Value::Integer(n),
            Literal::Float(f) => Value::Float(f),
            Literal::String(s) => Value::String(s),
            Literal::Boolean(b) => Value::Boolean(b),
            Literal::Keyword(k) => Value::Keyword(k),
            Literal::Symbol(s) => Value::Symbol(s),
            Literal::Nil => Value::Nil,
            Literal::Timestamp(ts) => Value::Timestamp(ts),
            Literal::Uuid(uuid) => Value::Uuid(uuid),
            Literal::ResourceHandle(handle) => Value::ResourceHandle(handle),
        }
    }
}
