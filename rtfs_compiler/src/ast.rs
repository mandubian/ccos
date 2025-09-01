impl MapKey {
    /// Public constructor for string map keys
    pub fn string(s: &str) -> Self {
        MapKey::String(s.to_string())
    }
}
use crate::runtime::values::Value;
use crate::runtime::error::RuntimeError;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use validator::Validate;

// --- Literal, Symbol, Keyword ---

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Literal {
    Integer(i64),
    Float(f64),
    String(String),
    Boolean(bool),
    Keyword(Keyword),
    Timestamp(String),      // Added
    Uuid(String),           // Added
    ResourceHandle(String), // Added
    Nil,
}

#[derive(Debug, PartialEq, Clone, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[schemars(transparent)]
pub struct Symbol(pub String);

impl Symbol {
    pub fn new(s: &str) -> Self {
        Symbol(s.to_string())
    }
}

#[derive(Debug, PartialEq, Clone, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[schemars(transparent)]
pub struct Keyword(pub String);

impl Keyword {
    pub fn new(s: &str) -> Self {
        Keyword(s.to_string())
    }
}

// --- Map Key ---
#[derive(Debug, PartialEq, Clone, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum MapKey {
    Keyword(Keyword),
    String(String),
    Integer(i64),
}

impl std::fmt::Display for MapKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MapKey::Keyword(k) => write!(f, ":{}", k.0),
            MapKey::String(s) => write!(f, "\"{}\"", s),
            MapKey::Integer(i) => write!(f, "{}", i),
        }
    }
}

// --- Patterns for Destructuring (let, fn params) ---
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Pattern {
    Symbol(Symbol),
    Wildcard, // _
    VectorDestructuring {
        // Renamed from VectorPattern
        elements: Vec<Pattern>,
        rest: Option<Symbol>,      // For ..rest or &rest
        as_symbol: Option<Symbol>, // For :as binding
    },
    MapDestructuring {
        // Renamed from MapPattern
        entries: Vec<MapDestructuringEntry>,
        rest: Option<Symbol>,      // For ..rest or &rest
        as_symbol: Option<Symbol>, // For :as binding
    },
    // Literal(Literal), // Literals are not typically part of binding patterns directly, but MatchPattern
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum MapDestructuringEntry {
    KeyBinding { key: MapKey, pattern: Box<Pattern> },
    Keys(Vec<Symbol>), // For :keys [s1 s2]
                       // TODO: Consider :or { default-val literal } if needed for destructuring
}

// --- Patterns for Matching (match clauses) ---
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum MatchPattern {
    Literal(Literal),
    Symbol(Symbol),                 // Binds the matched value to the symbol
    Keyword(Keyword),               // Matches a specific keyword
    Wildcard,                       // _
    Type(TypeExpr, Option<Symbol>), // Matches type, optionally binds the value
    Vector {
        // Changed from VectorMatchPattern
        elements: Vec<MatchPattern>,
        rest: Option<Symbol>, // For ..rest or &rest
    },
    Map {
        // Changed from MapMatchPattern
        entries: Vec<MapMatchEntry>,
        rest: Option<Symbol>, // For ..rest or &rest
    },
    As(Symbol, Box<MatchPattern>), // :as pattern
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize, JsonSchema)]
#[schemars(rename_all = "camelCase")]
pub struct MapMatchEntry {
    pub key: MapKey,
    pub pattern: Box<MatchPattern>,
}

// --- Type Expressions ---

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum PrimitiveType {
    Int,
    Float,
    String,
    Bool,
    Nil,
    Keyword, // Represents the type of keywords themselves
    Symbol,  // Represents the type of symbols themselves
    // Any, // Moved to TypeExpr::Any
    // Never, // Moved to TypeExpr::Never
    Custom(Keyword), // For other primitive-like types specified by a keyword e.g. :my-custom-primitive
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize, JsonSchema)]
#[schemars(rename_all = "camelCase")]
pub struct MapTypeEntry {
    pub key: Keyword, // Keys in map types are keywords
    pub value_type: Box<TypeExpr>,
    pub optional: bool,
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ParamType {
    Simple(Box<TypeExpr>),
    // Represents a standard parameter with a type
    // Variadic(Box<TypeExpr>), // Represented by FnExpr.variadic_param_type now
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ArrayDimension {
    Fixed(usize),  // Fixed size dimension like 3 in [3 4]
    Variable,      // Variable dimension represented by ?
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum TypePredicate {
    // Numeric predicates
    GreaterThan(Literal),
    GreaterEqual(Literal),
    LessThan(Literal),
    LessEqual(Literal),
    Equal(Literal),
    NotEqual(Literal),
    InRange(Literal, Literal),
    
    // String predicates
    MinLength(usize),
    MaxLength(usize),
    Length(usize),
    MatchesRegex(String),
    IsUrl,
    IsEmail,
    
    // Collection predicates
    MinCount(usize),
    MaxCount(usize),
    Count(usize),
    NonEmpty,
    
    // Map predicates
    HasKey(Keyword),
    RequiredKeys(Vec<Keyword>),
    
    // Custom predicate for extensibility
    Custom(Keyword, Vec<Literal>),
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum TypeExpr {
    Primitive(PrimitiveType),
    Alias(Symbol),         // Type alias like MyType or my.namespace/MyType
    Vector(Box<TypeExpr>), // Vector type, e.g., [:vector :int]
    Tuple(Vec<TypeExpr>),  // Tuple type, e.g., [:tuple :int :string :bool]
    Map {
        entries: Vec<MapTypeEntry>,
        wildcard: Option<Box<TypeExpr>>, // For [:* AnyType]
    },
    Function {
        param_types: Vec<ParamType>,                // Changed from params
        variadic_param_type: Option<Box<TypeExpr>>, // Changed from variadic
        return_type: Box<TypeExpr>,
    },
    Resource(Symbol),            // E.g., [:resource my.pkg/Handle]
    Union(Vec<TypeExpr>),        // E.g., [:union :int :string] (changed from :or)
    Intersection(Vec<TypeExpr>), // E.g., [:and HasName HasId]
    Literal(Literal),            // E.g., [:val 123] or [:val "hello"]
    Any,                         // :any type
    Never,                       // :never type
    
    // New RTFS 2.0 type features
    Array {
        element_type: Box<TypeExpr>,
        shape: Vec<ArrayDimension>,
    },
    Refined {
        base_type: Box<TypeExpr>,
        predicates: Vec<TypePredicate>,
    },
    Enum(Vec<Literal>),          // E.g., [:enum :red :green :blue]
    Optional(Box<TypeExpr>),     // Sugar for [:union T :nil]
}

impl TypeExpr {
    /// Parse a TypeExpr from a string using the RTFS parser
    pub fn from_str(s: &str) -> Result<Self, String> {
        // Try to use the full parser first
        match crate::parser::parse_type_expression(s) {
            Ok(type_expr) => Ok(type_expr),
            Err(_) => {
                // Fallback to simple parsing for basic types
                match s.trim() {
                    ":int" => Ok(TypeExpr::Primitive(PrimitiveType::Int)),
                    ":float" => Ok(TypeExpr::Primitive(PrimitiveType::Float)),
                    ":string" => Ok(TypeExpr::Primitive(PrimitiveType::String)),
                    ":bool" => Ok(TypeExpr::Primitive(PrimitiveType::Bool)),
                    ":nil" => Ok(TypeExpr::Primitive(PrimitiveType::Nil)),
                    ":keyword" => Ok(TypeExpr::Primitive(PrimitiveType::Keyword)),
                    ":symbol" => Ok(TypeExpr::Primitive(PrimitiveType::Symbol)),
                    ":any" => Ok(TypeExpr::Any),
                    ":never" => Ok(TypeExpr::Never),
                    _ => {
                        // Handle optional types (T?)
                        if s.ends_with("?") {
                            let base_type_str = &s[..s.len()-1];
                            let base_type = Self::from_str(base_type_str)?;
                            return Ok(TypeExpr::Optional(Box::new(base_type)));
                        }
                        
                        // For other types, treat as alias
                        Ok(TypeExpr::Alias(Symbol(s.to_string())))
                    }
                }
            }
        }
    }

    /// Convert TypeExpr to JSON Schema for validation
    pub fn to_json(&self) -> Result<serde_json::Value, String> {
        use serde_json::json;
        
        match self {
            TypeExpr::Primitive(ptype) => match ptype {
                PrimitiveType::Int => Ok(json!({"type": "integer"})),
                PrimitiveType::Float => Ok(json!({"type": "number"})),
                PrimitiveType::String => Ok(json!({"type": "string"})),
                PrimitiveType::Bool => Ok(json!({"type": "boolean"})),
                PrimitiveType::Nil => Ok(json!({"type": "null"})),
                PrimitiveType::Keyword => Ok(json!({"type": "string", "pattern": "^:.+"})),
                PrimitiveType::Symbol => Ok(json!({"type": "string"})),
                PrimitiveType::Custom(k) => Ok(json!({"type": "object", "description": format!("Custom type: {}", k.0)})),
            },
            TypeExpr::Vector(inner) => Ok(json!({
                "type": "array",
                "items": inner.to_json()?
            })),
            TypeExpr::Array { element_type, shape } => {
                let mut schema = json!({
                    "type": "array",
                    "items": element_type.to_json()?
                });
                
                // Add shape constraints if present
                if !shape.is_empty() {
                    if let Some(fixed_size) = shape.iter()
                        .filter_map(|d| if let ArrayDimension::Fixed(n) = d { Some(*n) } else { None })
                        .next() {
                        schema["minItems"] = json!(fixed_size);
                        schema["maxItems"] = json!(fixed_size);
                    }
                }
                Ok(schema)
            },
            TypeExpr::Tuple(types) => {
                let schemas: Result<Vec<_>, _> = types.iter().map(|t| t.to_json()).collect();
                Ok(json!({
                    "type": "array",
                    "items": schemas?,
                    "minItems": types.len(),
                    "maxItems": types.len()
                }))
            },
            TypeExpr::Union(types) => {
                let schemas: Result<Vec<_>, _> = types.iter().map(|t| t.to_json()).collect();
                Ok(json!({
                    "anyOf": schemas?
                }))
            },
            TypeExpr::Optional(inner) => {
                Ok(json!({
                    "anyOf": [inner.to_json()?, json!({"type": "null"})]
                }))
            },
            TypeExpr::Enum(values) => {
                let enum_values: Vec<serde_json::Value> = values.iter().map(|lit| {
                    match lit {
                        Literal::Integer(i) => json!(i),
                        Literal::Float(f) => json!(f),
                        Literal::String(s) => json!(s),
                        Literal::Boolean(b) => json!(b),
                        Literal::Keyword(k) => json!(k.0),
                        _ => json!(format!("{:?}", lit)),
                    }
                }).collect();
                Ok(json!({
                    "enum": enum_values
                }))
            },
            TypeExpr::Refined { base_type, predicates } => {
                let mut schema = base_type.to_json()?;
                
                // Apply predicates as JSON Schema constraints
                for predicate in predicates {
                    match predicate {
                        TypePredicate::MinLength(len) => {
                            schema["minLength"] = json!(len);
                        },
                        TypePredicate::MaxLength(len) => {
                            schema["maxLength"] = json!(len);
                        },
                        TypePredicate::MatchesRegex(pattern) => {
                            schema["pattern"] = json!(pattern);
                        },
                        TypePredicate::GreaterThan(Literal::Integer(n)) => {
                            schema["minimum"] = json!(n + 1);
                        },
                        TypePredicate::GreaterEqual(Literal::Integer(n)) => {
                            schema["minimum"] = json!(n);
                        },
                        TypePredicate::LessThan(Literal::Integer(n)) => {
                            schema["maximum"] = json!(n - 1);
                        },
                        TypePredicate::LessEqual(Literal::Integer(n)) => {
                            schema["maximum"] = json!(n);
                        },
                        _ => {} // Other predicates not directly expressible in JSON Schema
                    }
                }
                Ok(schema)
            },
            TypeExpr::Any => Ok(json!({})), // Accept anything
            TypeExpr::Never => Ok(json!({"not": {}})), // Accept nothing
            _ => {
                // For other complex types, provide a basic schema
                // This is a simplified implementation
                Ok(json!({"type": "object"}))
            }
        }
    }
}

impl std::fmt::Display for TypeExpr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TypeExpr::Primitive(ptype) => match ptype {
                PrimitiveType::Int => write!(f, ":int"),
                PrimitiveType::Float => write!(f, ":float"),
                PrimitiveType::String => write!(f, ":string"),
                PrimitiveType::Bool => write!(f, ":bool"),
                PrimitiveType::Nil => write!(f, ":nil"),
                PrimitiveType::Keyword => write!(f, ":keyword"),
                PrimitiveType::Symbol => write!(f, ":symbol"),
                PrimitiveType::Custom(k) => write!(f, ":{}", k.0),
            },
            TypeExpr::Vector(inner) => write!(f, "[:vector {}]", inner),
            TypeExpr::Array { element_type, shape } => {
                if shape.is_empty() {
                    write!(f, "[:array {}]", element_type)
                } else {
                    let shape_str: Vec<String> = shape.iter().map(|d| match d {
                        ArrayDimension::Fixed(n) => n.to_string(),
                        ArrayDimension::Variable => "?".to_string(),
                    }).collect();
                    write!(f, "[:array {} [{}]]", element_type, shape_str.join(" "))
                }
            },
            TypeExpr::Tuple(types) => {
                let type_strs: Vec<String> = types.iter().map(|t| t.to_string()).collect();
                write!(f, "[:tuple {}]", type_strs.join(" "))
            },
            TypeExpr::Union(types) => {
                let type_strs: Vec<String> = types.iter().map(|t| t.to_string()).collect();
                write!(f, "[:union {}]", type_strs.join(" "))
            },
            TypeExpr::Optional(inner) => write!(f, "{}?", inner),
            TypeExpr::Enum(values) => {
                let value_strs: Vec<String> = values.iter().map(|v| match v {
                    Literal::Keyword(k) => format!(":{}", k.0),
                    Literal::String(s) => format!("\"{}\"", s),
                    Literal::Integer(i) => i.to_string(),
                    Literal::Float(f) => f.to_string(),
                    Literal::Boolean(b) => b.to_string(),
                    _ => format!("{:?}", v),
                }).collect();
                write!(f, "[:enum {}]", value_strs.join(" "))
            },
            TypeExpr::Refined { base_type, predicates } => {
                if predicates.is_empty() {
                    write!(f, "{}", base_type)
                } else {
                    let pred_strs: Vec<String> = predicates.iter().map(|p| format!("{:?}", p)).collect();
                    write!(f, "[:and {} {}]", base_type, pred_strs.join(" "))
                }
            },
            TypeExpr::Any => write!(f, ":any"),
            TypeExpr::Never => write!(f, ":never"),
            TypeExpr::Alias(symbol) => write!(f, "{}", symbol.0),
            TypeExpr::Map { entries, wildcard } => {
                let mut parts = Vec::new();
                for entry in entries {
                    let optional = if entry.optional { "?" } else { "" };
                    parts.push(format!("[:{} {}{}]", entry.key.0, entry.value_type, optional));
                }
                if let Some(w) = wildcard {
                    parts.push(format!("[:* {}]", w));
                }
                write!(f, "[:map {}]", parts.join(" "))
            },
            TypeExpr::Function { param_types, variadic_param_type, return_type } => {
                let mut param_strs: Vec<String> = param_types.iter().map(|p| match p {
                    ParamType::Simple(t) => t.to_string(),
                }).collect();
                if let Some(variadic) = variadic_param_type {
                    param_strs.push(format!("& {}", variadic));
                }
                write!(f, "[:fn [{}] {}]", param_strs.join(" "), return_type)
            },
            TypeExpr::Resource(symbol) => write!(f, "[:resource {}]", symbol.0),
            TypeExpr::Intersection(types) => {
                let type_strs: Vec<String> = types.iter().map(|t| t.to_string()).collect();
                write!(f, "[:and {}]", type_strs.join(" "))
            },
            TypeExpr::Literal(lit) => write!(f, "[:val {:?}]", lit),
        }
    }
}

// --- Core Expression Structure ---

// Represents a single binding in a `let` expression
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize, JsonSchema, Validate)]
#[schemars(rename_all = "camelCase")]
pub struct LetBinding {
    pub pattern: Pattern, // Changed from symbol: Symbol
    pub type_annotation: Option<TypeExpr>,
    #[validate(nested)]
    pub value: Box<Expression>,
}

// Represents the main expression types
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Expression {
    Literal(Literal),
    Symbol(Symbol),
    // Keyword(Keyword), // Keywords are literals: Literal::Keyword
    List(#[validate(nested)] Vec<Expression>),
    Vector(#[validate(nested)] Vec<Expression>),
    Map(HashMap<MapKey, Expression>),
    FunctionCall {
        #[validate(nested)]
        callee: Box<Expression>, // Added this field
        #[validate(nested)]
        arguments: Vec<Expression>,
    },
    If(#[validate] IfExpr),
    Let(#[validate] LetExpr),
    Do(#[validate] DoExpr),
    Fn(#[validate] FnExpr),
    Def(#[validate] Box<DefExpr>),   // Added for def as an expression
    Defn(#[validate] Box<DefnExpr>), // Added for defn as an expression
    Defstruct(#[validate] Box<DefstructExpr>), // Added for defstruct as an expression
    DiscoverAgents(#[validate] DiscoverAgentsExpr),
    LogStep(#[validate] Box<LogStepExpr>),
    TryCatch(#[validate] TryCatchExpr),
    Parallel(#[validate] ParallelExpr),
    WithResource(#[validate] WithResourceExpr),
    Match(#[validate] MatchExpr),
    For(#[validate] Box<ForExpr>),           // Added for for comprehension
    Deref(#[validate] Box<Expression>),      // Added for @atom deref sugar
    ResourceRef(String),                      // Added

}

impl Validate for Expression {
    fn validate(&self) -> Result<(), validator::ValidationErrors> {
        match self {
            Expression::List(items) | Expression::Vector(items) => {
                for item in items {
                    item.validate()?;
                }
                Ok(())
            }
            Expression::Map(map) => {
                for value in map.values() {
                    value.validate()?;
                }
                Ok(())
            }
            Expression::FunctionCall { callee, arguments } => {
                callee.validate()?;
                for arg in arguments {
                    arg.validate()?;
                }
                Ok(())
            }
            Expression::If(expr) => expr.validate(),
            Expression::Let(expr) => expr.validate(),
            Expression::Do(expr) => expr.validate(),
            Expression::Fn(expr) => expr.validate(),
            Expression::Def(expr) => expr.validate(),
            Expression::Defn(expr) => expr.validate(),
            Expression::Defstruct(expr) => expr.validate(),
            Expression::DiscoverAgents(expr) => expr.validate(),
            Expression::LogStep(expr) => expr.validate(),
            Expression::TryCatch(expr) => expr.validate(),
            Expression::Parallel(expr) => expr.validate(),
            Expression::WithResource(expr) => expr.validate(),
            Expression::Match(expr) => expr.validate(),
            Expression::For(expr) => expr.validate(),
            Expression::Deref(expr) => expr.validate(),
            _ => Ok(()), // Literals, Symbols, etc. do not need validation
        }
    }
}

// Struct for Match Expression
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize, JsonSchema, Validate)]
#[schemars(rename_all = "camelCase")]
pub struct MatchExpr {
    #[validate(nested)]
    pub expression: Box<Expression>,
    #[validate(nested)]
    pub clauses: Vec<MatchClause>,
}

// Struct for LogStep Expression
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize, JsonSchema, Validate)]
#[schemars(rename_all = "camelCase")]
pub struct LogStepExpr {
    pub level: Option<Keyword>, // e.g., :info, :debug, :error
    #[validate(nested)]
    pub values: Vec<Expression>, // The expressions to log
    pub location: Option<String>, // Optional string literal for source location hint
}

// Structs for Special Forms
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Validate)]
#[schemars(rename_all = "camelCase")]
pub struct LetExpr {
    #[validate(nested)]
    pub bindings: Vec<LetBinding>,
    #[validate(nested)]
    pub body: Vec<Expression>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Validate)]
#[schemars(rename_all = "camelCase")]
pub struct IfExpr {
    #[validate(nested)]
    pub condition: Box<Expression>,
    #[validate(nested)]
    pub then_branch: Box<Expression>,
    #[validate(nested)]
    pub else_branch: Option<Box<Expression>>, // Else is optional in grammar
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Validate)]
#[schemars(rename_all = "camelCase")]
pub struct DoExpr {
    #[validate(nested)]
    pub expressions: Vec<Expression>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Validate)]
#[schemars(rename_all = "camelCase")]
pub struct FnExpr {
    pub params: Vec<ParamDef>,
    pub variadic_param: Option<ParamDef>, // Changed from Option<Symbol>
    pub return_type: Option<TypeExpr>,
    #[validate(nested)]
    pub body: Vec<Expression>,
    pub delegation_hint: Option<DelegationHint>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[schemars(rename_all = "camelCase")]
pub struct ParamDef {
    pub pattern: Pattern, // Changed from name: Symbol to allow destructuring
    pub type_annotation: Option<TypeExpr>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Validate)]
#[schemars(rename_all = "camelCase")]
pub struct DefExpr {
    pub symbol: Symbol,
    pub type_annotation: Option<TypeExpr>,
    #[validate(nested)]
    pub value: Box<Expression>,
}

// Defn is essentially syntax sugar for (def name (fn ...))
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Validate)]
#[schemars(rename_all = "camelCase")]
pub struct DefnExpr {
    pub name: Symbol,
    pub params: Vec<ParamDef>,
    pub variadic_param: Option<ParamDef>, // Changed from Option<Symbol>
    pub return_type: Option<TypeExpr>,
    #[validate(nested)]
    pub body: Vec<Expression>,
    pub delegation_hint: Option<DelegationHint>,
}

// Defstruct is syntactic sugar for (def name refined-map-type)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Validate)]
#[schemars(rename_all = "camelCase")]
pub struct DefstructExpr {
    pub name: Symbol,
    pub fields: Vec<DefstructField>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Validate)]
#[schemars(rename_all = "camelCase")]
pub struct DefstructField {
    pub key: Keyword,
    pub field_type: TypeExpr,
}

// --- New Special Form Structs ---

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Validate)]
#[schemars(rename_all = "camelCase")]
pub struct ParallelExpr {
    // parallel_binding = { "[" ~ symbol ~ (":" ~ type_expr)? ~ expression ~ "]" }
    #[validate(nested)]
    pub bindings: Vec<ParallelBinding>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Validate)]
#[schemars(rename_all = "camelCase")]
pub struct ParallelBinding {
    pub symbol: Symbol,
    pub type_annotation: Option<TypeExpr>,
    #[validate(nested)]
    pub expression: Box<Expression>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Validate)]
#[schemars(rename_all = "camelCase")]
pub struct WithResourceExpr {
    // "[" ~ symbol ~ type_expr ~ expression ~ "]"
    pub resource_symbol: Symbol,
    pub resource_type: TypeExpr, // Type is mandatory in grammar
    #[validate(nested)]
    pub resource_init: Box<Expression>,
    #[validate(nested)]
    pub body: Vec<Expression>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Validate)]
#[schemars(rename_all = "camelCase")]
pub struct TryCatchExpr {
    #[validate(nested)]
    pub try_body: Vec<Expression>,
    #[validate(nested)]
    pub catch_clauses: Vec<CatchClause>,
    #[validate(nested)]
    pub finally_body: Option<Vec<Expression>>, // Optional in grammar
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize, JsonSchema, Validate)]
#[schemars(rename_all = "camelCase")]
pub struct CatchClause {
    pub pattern: CatchPattern, // This seems to be a separate enum
    pub binding: Symbol,
    #[validate(nested)]
    pub body: Vec<Expression>,
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum CatchPattern {
    Keyword(Keyword), // e.g. :Error
    Type(TypeExpr),   // e.g. :my.pkg/CustomErrorType
    Symbol(Symbol),   // e.g. AnyError - acts as a catch-all with binding
    Wildcard,         // e.g. _ - matches any error
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize, JsonSchema, Validate)]
#[schemars(rename_all = "camelCase")]
pub struct MatchClause {
    pub pattern: MatchPattern, // Changed from Pattern
    #[validate(nested)]
    pub guard: Option<Box<Expression>>,
    #[validate(nested)]
    pub body: Box<Expression>, // Changed from Vec<Expression>
}

// Represents top-level definitions in a file
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum TopLevel {
    Intent(#[validate] IntentDefinition),
    Plan(#[validate] PlanDefinition),
    Action(#[validate] ActionDefinition),
    Capability(#[validate] CapabilityDefinition),
    Resource(#[validate] ResourceDefinition),
    Module(#[validate] ModuleDefinition),
    Expression(#[validate] Expression),
}

impl Validate for TopLevel {
    fn validate(&self) -> Result<(), validator::ValidationErrors> {
        match self {
            TopLevel::Intent(def) => def.validate(),
            TopLevel::Plan(def) => def.validate(),
            TopLevel::Action(def) => def.validate(),
            TopLevel::Capability(def) => def.validate(),
            TopLevel::Resource(def) => def.validate(),
            TopLevel::Module(def) => def.validate(),
            TopLevel::Expression(expr) => expr.validate(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Validate, Serialize, Deserialize, JsonSchema)]
#[schemars(rename_all = "camelCase")]
pub struct Property {
    pub key: Keyword,
    #[validate(nested)]
    pub value: Expression,
}

#[derive(Debug, Clone, PartialEq, Validate, Serialize, Deserialize, JsonSchema)]
#[schemars(rename_all = "camelCase")]
pub struct IntentDefinition {
    pub name: Symbol, // Using Symbol to hold the versioned type identifier
    #[validate(nested)]
    pub properties: Vec<Property>,
}

#[derive(Debug, Clone, PartialEq, Validate, Serialize, Deserialize, JsonSchema)]
#[schemars(rename_all = "camelCase")]
pub struct PlanDefinition {
    pub name: Symbol,
    #[validate(nested)]
    pub properties: Vec<Property>,
}

#[derive(Debug, Clone, PartialEq, Validate, Serialize, Deserialize, JsonSchema)]
#[schemars(rename_all = "camelCase")]
pub struct ActionDefinition {
    pub name: Symbol,
    #[validate(nested)]
    pub properties: Vec<Property>,
}

#[derive(Debug, Clone, PartialEq, Validate, Serialize, Deserialize, JsonSchema)]
#[schemars(rename_all = "camelCase")]
pub struct CapabilityDefinition {
    pub name: Symbol,
    #[validate(nested)]
    pub properties: Vec<Property>,
}

#[derive(Debug, Clone, PartialEq, Validate, Serialize, Deserialize, JsonSchema)]
#[schemars(rename_all = "camelCase")]
pub struct ResourceDefinition {
    pub name: Symbol,
    #[validate(nested)]
    pub properties: Vec<Property>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Validate)]
#[schemars(rename_all = "camelCase")]
pub struct ModuleDefinition {
    pub name: Symbol,              // Namespaced identifier
    pub docstring: Option<String>, // Optional documentation string
    pub exports: Option<Vec<Symbol>>,
    #[validate(nested)]
    pub definitions: Vec<ModuleLevelDefinition>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ModuleLevelDefinition {
    Def(#[validate] DefExpr),
    Defn(#[validate] DefnExpr),
    Import(ImportDefinition),
}

impl Validate for ModuleLevelDefinition {
    fn validate(&self) -> Result<(), validator::ValidationErrors> {
        match self {
            ModuleLevelDefinition::Def(def) => def.validate(),
            ModuleLevelDefinition::Defn(def) => def.validate(),
            ModuleLevelDefinition::Import(_) => Ok(()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[schemars(rename_all = "camelCase")]
pub struct ImportDefinition {
    pub module_name: Symbol,       // Namespaced identifier
    pub alias: Option<Symbol>,     // :as alias
    pub only: Option<Vec<Symbol>>, // :only [sym1 sym2]
}

/// For Expression - for (for ...) special form
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Validate)]
#[serde(rename_all = "camelCase")]
pub struct ForExpr {
    /// Vector of binding expressions [sym1 coll1 sym2 coll2 ...]
    #[validate(nested)]
    pub bindings: Vec<Expression>,
    /// Body expression to evaluate for each combination
    #[validate(nested)]
    pub body: Box<Expression>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Validate)]
#[serde(rename_all = "camelCase")]
pub struct DiscoverAgentsExpr {
    /// Discovery criteria map (required)
    #[validate(nested)]
    pub criteria: Box<Expression>, // Must be a Map expression

    /// Options map (optional)
    #[validate(nested)]
    pub options: Option<Box<Expression>>, // Optional Map expression
}

// Removed PlanExpr from RTFS core AST. Plan is a CCOS object extracted from
// standard RTFS expressions (FunctionCall or Map) at the CCOS layer.



// --- Delegation Hint ---
/// Optional compile-time hint that instructs the runtime where a function
/// prefers to execute.  Mirrors (but is independent from) `ExecTarget` in the
/// CCOS Delegation Engine to avoid circular dependencies.
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum DelegationHint {
    LocalPure,
    LocalModel(String),
    RemoteModel(String),
}

impl DelegationHint {
    /// Convert this delegation hint to the corresponding ExecTarget.
    /// This bridges the AST layer with the runtime delegation engine.
    pub fn to_exec_target(&self) -> crate::ccos::delegation::ExecTarget {
        use crate::ccos::delegation::ExecTarget;
        match self {
            DelegationHint::LocalPure => ExecTarget::LocalPure,
            DelegationHint::LocalModel(id) => ExecTarget::LocalModel(id.to_string()),
            DelegationHint::RemoteModel(id) => ExecTarget::RemoteModel(id.to_string()),
        }
    }
}

impl TryFrom<Value> for Expression {
    type Error = RuntimeError;

    fn try_from(value: Value) -> Result<Self, Self::Error> {
        match value {
            Value::Symbol(s) => Ok(Expression::Symbol(s)),
            Value::Keyword(k) => Ok(Expression::Literal(Literal::Keyword(k))),
            Value::Integer(i) => Ok(Expression::Literal(Literal::Integer(i))),
            Value::Float(f) => Ok(Expression::Literal(Literal::Float(f))),
            Value::String(s) => Ok(Expression::Literal(Literal::String(s))),
            Value::Boolean(b) => Ok(Expression::Literal(Literal::Boolean(b))),
            Value::Nil => Ok(Expression::Literal(Literal::Nil)),
            Value::Timestamp(t) => Ok(Expression::Literal(Literal::Timestamp(t))),
            Value::Uuid(u) => Ok(Expression::Literal(Literal::Uuid(u))),
            Value::ResourceHandle(r) => Ok(Expression::Literal(Literal::ResourceHandle(r))),
            Value::Vector(v) => {
                let mut exprs = Vec::new();
                for item in v {
                    exprs.push(Expression::try_from(item)?);
                }
                Ok(Expression::Vector(exprs))
            }
            Value::List(l) => {
                let mut exprs = Vec::new();
                for item in l {
                    exprs.push(Expression::try_from(item)?);
                }
                Ok(Expression::List(exprs))
            }
            Value::Map(m) => {
                let mut map = HashMap::new();
                for (k, v) in m {
                    map.insert(k, Expression::try_from(v)?);
                }
                Ok(Expression::Map(map))
            }
            _ => Err(RuntimeError::new(&format!("Cannot convert {} to an expression", value.type_name()))),
        }
    }
}
