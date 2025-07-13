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
        body: Box<Expression>,
        env: Rc<Environment>,
        delegation_hint: Option<crate::ast::DelegationHint>,
    ) -> Function {
        Function::Closure(Rc::new(Closure {
            params,
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
            Expression::If(if_expr) => {
                // For now, return a placeholder for if expressions
                Value::String("#<if-expression>".to_string())
            }
            Expression::Let(let_expr) => {
                // For now, return a placeholder for let expressions
                Value::String("#<let-expression>".to_string())
            }
            Expression::Do(do_expr) => {
                // For now, return a placeholder for do expressions
                Value::String("#<do-expression>".to_string())
            }
            Expression::Fn(fn_expr) => {
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
            Expression::DiscoverAgents(discover_expr) => {
                // For now, return a placeholder for discover-agents expressions
                Value::String("#<discover-agents>".to_string())
            }
            Expression::TryCatch(try_expr) => {
                // For now, return a placeholder for try-catch expressions
                Value::String("#<try-catch>".to_string())
            }
            Expression::Parallel(parallel_expr) => {
                // For now, return a placeholder for parallel expressions
                Value::String("#<parallel>".to_string())
            }
            Expression::WithResource(with_expr) => {
                // For now, return a placeholder for with-resource expressions
                Value::String(format!("#<with-resource: {}>", with_expr.resource_symbol.0))
            }
            Expression::Match(match_expr) => {
                // For now, return a placeholder for match expressions
                Value::String("#<match>".to_string())
            }
            Expression::ResourceRef(resource_name) => {
                // Return the resource name as a string
                Value::String(format!("@{}", resource_name))
            }
            Expression::TaskContextAccess(task_context) => {
                // Return a placeholder for task context access
                Value::String(format!("#<task-context: {}>", task_context.field.0))
            }
            Expression::LogStep(log_expr) => {
                // For now, return a placeholder for log step expressions
                Value::String("#<log-step>".to_string())
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
