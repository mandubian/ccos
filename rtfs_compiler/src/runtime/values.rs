// Runtime value system for RTFS
// Represents values during execution (different from AST which represents parsed code)

use crate::ast::{Expression, Keyword, MapKey, Symbol};
use crate::runtime::environment::Environment;
use crate::runtime::error::RuntimeError;
use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Nil,
    Boolean(bool),
    Integer(i64),
    Float(f64),
    String(String),
    Timestamp(String),
    Uuid(String),
    ResourceHandle(String),
    Symbol(Symbol),
    Keyword(Keyword),
    Vector(Vec<Value>),
    Map(HashMap<MapKey, Value>),
    Function(Function),
    FunctionPlaceholder(Rc<RefCell<Option<Function>>>),
    Error(ErrorValue),
}

#[derive(Debug, Clone, PartialEq)]
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
            }            Value::Map(m) => {
                let items: Vec<String> = m
                    .iter()
                    .map(|(k, v)| format!("{:?} {}", k, v))
                    .collect();
                write!(f, "{{{}}}", items.join(", "))
            }
            Value::Function(_) => write!(f, "#<function>"),
            Value::FunctionPlaceholder(_) => write!(f, "#<function-placeholder>"),
            Value::Error(e) => write!(f, "#<error: {}>", e.message),
        }
    }
}

#[derive(Clone)]
pub enum Function {
    Builtin(BuiltinFunction),
    Closure {
        params: Vec<Symbol>,
        body: Box<Expression>,
        env: Rc<Environment>,
    },
}

impl fmt::Debug for Function {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Function::Builtin(_) => write!(f, "BuiltinFunction"),
            Function::Closure { .. } => write!(f, "Closure"),
        }
    }
}

impl PartialEq for Function {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Function::Builtin(a), Function::Builtin(b)) => a == b,
            (Function::Closure { .. }, Function::Closure { .. }) => {
                // Closures are compared by reference, effectively.
                // For simplicity, we'll consider two distinct closures to be unequal.
                false
            }
            _ => false,
        }
    }
}

#[derive(Clone)]
pub struct BuiltinFunction {
    pub name: String,
    pub arity: Arity,
    pub func: Rc<dyn Fn(Vec<Value>) -> Result<Value, RuntimeError>>,
}

impl fmt::Debug for BuiltinFunction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BuiltinFunction")
            .field("name", &self.name)
            .field("arity", &self.arity)
            .finish()
    }
}

impl PartialEq for BuiltinFunction {
    fn eq(&self, other: &Self) -> bool {
        // Compare builtin functions by name and arity, not by function pointer
        self.name == other.name && self.arity == other.arity
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Arity {
    Fixed(usize),
    Variadic(usize), // Minimum number of arguments
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
