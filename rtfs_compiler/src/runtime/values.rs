// Runtime value system for RTFS
// Represents values during execution (different from AST which represents parsed code)

use crate::ast::{Expression, Keyword, Literal, MapKey, Symbol};
use crate::ir::core::IrNode;
use crate::runtime::environment::Environment;
use crate::runtime::error::{RuntimeError, RuntimeResult};
use crate::runtime::Evaluator;
use crate::runtime::{IrEnvironment, IrRuntime};
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
    List(Vec<Value>),
    Map(HashMap<MapKey, Value>),
    Function(Function),
    FunctionPlaceholder(Rc<RefCell<Value>>),
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
    IrLambda(Rc<IrLambda>),
    Native(BuiltinFunction),
    Rtfs(Rc<Closure>),
    Ir(Rc<IrLambda>),
}

impl Function {
    pub fn new_closure(
        params: Vec<Symbol>,
        body: Box<Expression>,
        env: Rc<Environment>,
    ) -> Function {
        Function::Closure(Rc::new(Closure { params, body, env }))
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
            Function::IrLambda(_) => write!(f, "IrLambda"),
            Function::Native(_) => write!(f, "NativeFunction"),
            Function::Rtfs(_) => write!(f, "RtfsFunction"),
            Function::Ir(_) => write!(f, "IrFunction"),
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
            (Function::Rtfs(a), Function::Rtfs(b)) => Rc::ptr_eq(a, b),
            (Function::IrLambda(a), Function::IrLambda(b)) => Rc::ptr_eq(a, b),
            (Function::Ir(a), Function::Ir(b)) => Rc::ptr_eq(a, b),
            _ => false,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Closure {
    pub params: Vec<Symbol>,
    pub body: Box<Expression>,
    pub env: Rc<Environment>,
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
            _ => unimplemented!(),
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
