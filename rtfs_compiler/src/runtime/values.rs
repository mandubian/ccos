// Runtime value system for RTFS
// Represents values during execution (different from AST which represents parsed code)

use crate::ast::{Expression, Keyword, Literal, MapKey, Symbol};
use crate::ir::core::IrNode;
use crate::runtime::environment::Environment;
use crate::runtime::error::RuntimeResult;
use crate::runtime::Evaluator;
use crate::runtime::IrEnvironment;
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Value {
    Nil,
    Boolean(bool),
    Integer(i64),
    Float(f64),
    String(String),
    Timestamp(String),
    Uuid(String),
    ResourceHandle(String),
    /// Mutable reference type used by atom/deref/reset!/swap!
    #[serde(skip_serializing, skip_deserializing)]
    Atom(Rc<RefCell<Value>>),
    Symbol(Symbol),
    Keyword(Keyword),
    Vector(Vec<Value>),
    List(Vec<Value>),
    Map(HashMap<MapKey, Value>),
    #[serde(skip_serializing, skip_deserializing)]
    Function(Function),
    #[serde(skip_serializing, skip_deserializing)]
    FunctionPlaceholder(Rc<RefCell<Value>>),
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
            Value::Atom(_) => write!(f, "#<atom>"),
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
                let items: Vec<String> = m.iter().map(|(k, v)| format!("{:?} {}", k, v)).collect();
                write!(f, "{{{}}}", items.join(", "))
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
            Value::Atom(_) => "atom",
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
            (Value::Integer(a), Value::Float(b)) => (*a as f64).partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal),
            (Value::Integer(_), _) => std::cmp::Ordering::Less,
            (Value::Float(a), Value::Integer(b)) => a.partial_cmp(&(*b as f64)).unwrap_or(std::cmp::Ordering::Equal),
            (Value::Float(a), Value::Float(b)) => a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal),
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
            },
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
            },
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
                
                a_items.sort_by(|(k1, _), (k2, _)| map_key_to_string(k1).cmp(&map_key_to_string(k2)));
                b_items.sort_by(|(k1, _), (k2, _)| map_key_to_string(k1).cmp(&map_key_to_string(k2)));
                
                // Compare sorted items
                for (a_item, b_item) in a_items.iter().zip(b_items.iter()) {
                    match map_key_to_string(a_item.0).cmp(&map_key_to_string(b_item.0)) {
                        std::cmp::Ordering::Equal => {
                            match a_item.1.compare(b_item.1) {
                                std::cmp::Ordering::Equal => continue,
                                other => return other,
                            }
                        },
                        other => return other,
                    }
                }
                a_items.len().cmp(&b_items.len())
            },
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
    pub func: Rc<dyn Fn(Vec<Value>) -> RuntimeResult<Value>>,
}

#[derive(Clone)]
pub struct BuiltinFunctionWithContext {
    pub name: String,
    pub arity: Arity,
    pub func: Rc<dyn Fn(Vec<Value>, &Evaluator, &mut Environment) -> RuntimeResult<Value>>,
}

#[derive(Clone)]
pub enum Function {
    Builtin(BuiltinFunction),
    BuiltinWithContext(BuiltinFunctionWithContext),
    Closure(Rc<Closure>),
    Native(BuiltinFunction),
    Ir(Rc<IrLambda>),
}

impl Function {
    pub fn new_closure(
        params: Vec<Symbol>,
        param_patterns: Vec<crate::ast::Pattern>,
        body: Box<Expression>,
        env: Rc<Environment>,
        delegation_hint: Option<crate::ast::DelegationHint>,
    ) -> Function {
        Function::Closure(Rc::new(Closure {
            params,
            param_patterns,
            body,
            env,
            delegation_hint,
        }))
    }

    pub fn new_ir_lambda(
        params: Vec<IrNode>,
        variadic_param: Option<Box<IrNode>>,
        body: Vec<IrNode>,
        closure_env: Box<IrEnvironment>,
    ) -> Function {
        Function::Ir(Rc::new(IrLambda {
            params,
            variadic_param,
            body,
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
            (Function::Closure(a), Function::Closure(b)) => Rc::ptr_eq(a, b),
            (Function::Ir(a), Function::Ir(b)) => Rc::ptr_eq(a, b),
            _ => false,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Closure {
    pub params: Vec<Symbol>,
    // Full parameter patterns to support destructuring during invocation
    pub param_patterns: Vec<crate::ast::Pattern>,
    pub body: Box<Expression>,
    pub env: Rc<Environment>,
    pub delegation_hint: Option<crate::ast::DelegationHint>,
}

#[derive(Clone, Debug)]
pub struct IrLambda {
    pub params: Vec<IrNode>,
    pub variadic_param: Option<Box<IrNode>>,
    pub body: Vec<IrNode>,
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
            Expression::DiscoverAgents(_) => {
                    // For now, return a placeholder for discover-agents expressions
                Value::String("#<discover-agents>".to_string())
            }
            Expression::TryCatch(_) => {
                    // For now, return a placeholder for try-catch expressions
                Value::String("#<try-catch>".to_string())
            }
            Expression::Parallel(_) => {
                    // For now, return a placeholder for parallel expressions
                Value::String("#<parallel>".to_string())
            }
            Expression::WithResource(with_expr) => {
                // For now, return a placeholder for with-resource expressions
                Value::String(format!("#<with-resource: {}>", with_expr.resource_symbol.0))
            }
            Expression::Match(_) => {
                // For now, return a placeholder for match expressions
                Value::String("#<match>".to_string())
            }
            Expression::ResourceRef(resource_name) => {
                // Return the resource name as a string
                Value::String(resource_name)
            }

            Expression::LogStep(_log_expr) => {
                // For now, return a placeholder for log step expressions
                Value::String("#<log-step>".to_string())
            }
            Expression::Defstruct(_defstruct_expr) => {
                // For now, return a placeholder for defstruct expressions
                Value::String("#<defstruct>".to_string())
            }
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
            Literal::Nil => Value::Nil,
            Literal::Timestamp(ts) => Value::Timestamp(ts),
            Literal::Uuid(uuid) => Value::Uuid(uuid),
            Literal::ResourceHandle(handle) => Value::ResourceHandle(handle),
        }
    }
}
