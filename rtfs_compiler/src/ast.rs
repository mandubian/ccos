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
    Union(Vec<TypeExpr>),        // E.g., [:or :int :string]
    Intersection(Vec<TypeExpr>), // E.g., [:and HasName HasId]
    Literal(Literal),            // E.g., [:val 123] or [:val "hello"]
    Any,                         // :any type
    Never,                       // :never type
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
    DiscoverAgents(#[validate] DiscoverAgentsExpr),
    LogStep(#[validate] Box<LogStepExpr>),
    TryCatch(#[validate] TryCatchExpr),
    Parallel(#[validate] ParallelExpr),
    WithResource(#[validate] WithResourceExpr),
    Match(#[validate] MatchExpr),
    ResourceRef(String),                      // Added
    TaskContextAccess(TaskContextAccessExpr), // Added for @context-key syntax
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
            Expression::DiscoverAgents(expr) => expr.validate(),
            Expression::LogStep(expr) => expr.validate(),
            Expression::TryCatch(expr) => expr.validate(),
            Expression::Parallel(expr) => expr.validate(),
            Expression::WithResource(expr) => expr.validate(),
            Expression::Match(expr) => expr.validate(),
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

/// Discover Agents Expression - for (discover-agents ...) special form
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Validate)]
#[schemars(rename_all = "camelCase")]
pub struct DiscoverAgentsExpr {
    /// Discovery criteria map (required)
    #[validate(nested)]
    pub criteria: Box<Expression>, // Must be a Map expression

    /// Options map (optional)
    #[validate(nested)]
    pub options: Option<Box<Expression>>, // Optional Map expression
}

/// Task context access expression (@context-key)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Validate)]
#[schemars(rename_all = "camelCase")]
pub struct TaskContextAccessExpr {
    pub field: Keyword, // The field name to access from task context
}

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
