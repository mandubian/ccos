use num_bigint::BigInt;
use std::collections::HashMap;
// Removed unused Hasher
use std::hash::Hash;

/// Represents types that are allowed as keys in ACL Maps.
/// Excludes non-hashable/non-Eq types like Float, List, Vector, Map.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MapKey {
    Nil,
    Bool(bool),
    Int(BigInt),
    String(String),
    Symbol(String),
    Keyword(String),
}

/// Represents any value in the ACL language.
#[derive(Debug, Clone)] // Cannot derive PartialEq/Eq/Hash due to f64.
pub enum Value {
    Nil,
    Bool(bool),
    Int(BigInt),
    Float(f64), // Floats are not Eq or Hash
    String(String),
    Symbol(String),
    Keyword(String),
    List(Vec<Value>),
    Vector(Vec<Value>),
    Map(HashMap<MapKey, Value>), // Keys are restricted to MapKey variants
}

// Function to attempt converting a Value into a MapKey
impl Value {
    pub fn into_map_key(self) -> Option<MapKey> {
        match self {
            Value::Nil => Some(MapKey::Nil),
            Value::Bool(b) => Some(MapKey::Bool(b)),
            Value::Int(i) => Some(MapKey::Int(i)),
            Value::String(s) => Some(MapKey::String(s)),
            Value::Symbol(s) => Some(MapKey::Symbol(s)),
            Value::Keyword(s) => Some(MapKey::Keyword(s)),
            // These cannot be keys
            Value::Float(_) | Value::List(_) | Value::Vector(_) | Value::Map(_) => None,
        }
    }

    // Helper method added to Value for testing convenience (optional)
    // Keep this if used in tests
    // pub fn get_vector(&self) -> Option<&Vec<Value>> {
    //     match self {
    //         Value::Vector(v) => Some(v),
    //         _ => None,
    //     }
    // }
}

// Manual PartialEq implementation for Value
impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Nil, Value::Nil) => true,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Int(a), Value::Int(b)) => a == b,
            (Value::Float(a), Value::Float(b)) => (a.is_nan() && b.is_nan()) || (a == b),
            (Value::String(a), Value::String(b)) => a == b,
            (Value::Symbol(a), Value::Symbol(b)) => a == b,
            (Value::Keyword(a), Value::Keyword(b)) => a == b,
            (Value::List(a), Value::List(b)) => a == b,
            (Value::Vector(a), Value::Vector(b)) => a == b,
            // Compare HashMap<MapKey, Value>
            (Value::Map(a), Value::Map(b)) => a == b, // HashMap implements PartialEq if K, V do. MapKey and Value do.
            _ => false,
        }
    }
}

// Note: Cannot implement Eq or Hash for Value because f64 is not Eq/Hash.

// Expr can derive PartialEq because Value implements it.
#[derive(Debug, PartialEq, Clone)]
pub enum Expr {
    Literal(Value),
    Variable(String),          // Represents a variable lookup, e.g., `x`
    Define(String, Box<Expr>), // Define a symbol, e.g., `(define x 10)`
    // Set(String, Box<Expr>), // Mutate an existing binding, e.g., `(set! x 20)` - Deferred: Revisit mutation later.
    Let {
        // Local bindings, e.g., `(let ((x 1) (y 2)) (+ x y))`
        bindings: Vec<(String, Expr)>, // The bindings (symbol, value expr)
        body: Box<Expr>,               // The expression to evaluate with the bindings
    },
    If {
        // Conditional expression, e.g., `(if (> x 0) "positive" "non-positive")`
        condition: Box<Expr>,
        then_branch: Box<Expr>,
        else_branch: Box<Expr>, // ACL requires an else branch
    },
    Do {
        // Sequence of expressions, e.g., `(do (print "hello") (+ 1 2))`
        expressions: Vec<Expr>, // Evaluated in order, result is the last one
    },
    Lambda {
        // Anonymous function definition, e.g., `(lambda (x y) (+ x y))`
        params: Vec<String>, // Parameter names
        body: Box<Expr>,     // Function body
    },
    Apply {
        // Function application, e.g., `(f 1 2)` or `((lambda (x) (* x x)) 5)`
        function: Box<Expr>,  // The expression evaluating to the function
        arguments: Vec<Expr>, // The argument expressions
    },
    // TODO: Add Cond, Match, Quote, Quasiquote, Unquote etc. later if needed.
}
