use std::collections::HashMap;

// --- Literal, Symbol, Keyword ---

#[derive(Debug, PartialEq, Clone)]
pub enum Literal {
    Integer(i64),
    Float(f64),
    String(String),
    Boolean(bool),
    Keyword(Keyword), // Added Keyword variant
    Nil,
}

#[derive(Debug, PartialEq, Clone, Eq, Hash)]
pub struct Symbol(pub String);

#[derive(Debug, PartialEq, Clone, Eq, Hash)]
pub struct Keyword(pub String);

// --- Map Key ---
#[derive(Debug, PartialEq, Clone, Eq, Hash)]
pub enum MapKey {
    Keyword(Keyword),
    String(String),
    Integer(i64),
}

// --- Patterns for Destructuring (let, fn params) ---
#[derive(Debug, PartialEq, Clone)]
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

#[derive(Debug, PartialEq, Clone)]
pub enum MapDestructuringEntry {
    KeyBinding { key: MapKey, pattern: Box<Pattern> },
    Keys(Vec<Symbol>), // For :keys [s1 s2]
                       // TODO: Consider :or { default-val literal } if needed for destructuring
}

// --- Patterns for Matching (match clauses) ---
#[derive(Debug, PartialEq, Clone)]
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

#[derive(Debug, PartialEq, Clone)]
pub struct MapMatchEntry {
    pub key: MapKey,
    pub pattern: Box<MatchPattern>,
}

// --- Type Expressions ---

#[derive(Debug, PartialEq, Clone)]
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

#[derive(Debug, PartialEq, Clone)]
pub struct MapTypeEntry {
    pub key: Keyword, // Keys in map types are keywords
    pub value_type: Box<TypeExpr>,
    pub optional: bool,
}

#[derive(Debug, PartialEq, Clone)]
pub enum ParamType {
    Simple(Box<TypeExpr>),
    // Represents a standard parameter with a type
    // Variadic(Box<TypeExpr>), // Represented by FnExpr.variadic_param_type now
}

#[derive(Debug, PartialEq, Clone)]
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
#[derive(Debug, PartialEq, Clone)]
pub struct LetBinding {
    pub pattern: Pattern, // Changed from symbol: Symbol
    pub type_annotation: Option<TypeExpr>,
    pub value: Box<Expression>,
}

// Represents the main expression types
#[derive(Debug, PartialEq, Clone)]
pub enum Expression {
    Literal(Literal),
    Symbol(Symbol),
    // Keyword(Keyword), // Keywords are literals: Literal::Keyword
    List(Vec<Expression>), // Added for generic lists like (1 2 3) or ()
    Vector(Vec<Expression>),
    Map(HashMap<MapKey, Expression>),
    FunctionCall {
        function: Box<Expression>,
        arguments: Vec<Expression>,
    },
    If(IfExpr),
    Let(LetExpr),
    Do(DoExpr),
    Match(Box<MatchExpr>),     // Changed to Box<MatchExpr>
    LogStep(Box<LogStepExpr>), // Changed to Box<LogStepExpr>
    TryCatch(TryCatchExpr),
    Fn(FnExpr),
    WithResource(WithResourceExpr),
    Parallel(ParallelExpr),
    Def(Box<DefExpr>),   // Added for def as an expression
    Defn(Box<DefnExpr>), // Added for defn as an expression
}

// Struct for Match Expression
#[derive(Debug, PartialEq, Clone)]
pub struct MatchExpr {
    pub expression: Box<Expression>,
    pub clauses: Vec<MatchClause>,
}

// Struct for LogStep Expression
#[derive(Debug, PartialEq, Clone)]
pub struct LogStepExpr {
    pub level: Option<Keyword>,   // e.g., :info, :debug, :error
    pub values: Vec<Expression>,  // The expressions to log
    pub location: Option<String>, // Optional string literal for source location hint
}

// Structs for Special Forms
#[derive(Debug, Clone, PartialEq)]
pub struct LetExpr {
    pub bindings: Vec<LetBinding>,
    pub body: Vec<Expression>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IfExpr {
    pub condition: Box<Expression>,
    pub then_branch: Box<Expression>,
    pub else_branch: Option<Box<Expression>>, // Else is optional in grammar
}

#[derive(Debug, Clone, PartialEq)]
pub struct DoExpr {
    pub expressions: Vec<Expression>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FnExpr {
    pub params: Vec<ParamDef>,
    pub variadic_param: Option<ParamDef>, // Changed from Option<Symbol>
    pub return_type: Option<TypeExpr>,
    pub body: Vec<Expression>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParamDef {
    pub pattern: Pattern, // Changed from name: Symbol to allow destructuring
    pub type_annotation: Option<TypeExpr>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DefExpr {
    pub symbol: Symbol,
    pub type_annotation: Option<TypeExpr>,
    pub value: Box<Expression>,
}

// Defn is essentially syntax sugar for (def name (fn ...))
#[derive(Debug, Clone, PartialEq)]
pub struct DefnExpr {
    pub name: Symbol,
    pub params: Vec<ParamDef>,
    pub variadic_param: Option<ParamDef>, // Changed from Option<Symbol>
    pub return_type: Option<TypeExpr>,
    pub body: Vec<Expression>,
}

// --- New Special Form Structs ---

#[derive(Debug, Clone, PartialEq)]
pub struct ParallelExpr {
    // parallel_binding = { "[" ~ symbol ~ (":" ~ type_expr)? ~ expression ~ "]" }
    pub bindings: Vec<ParallelBinding>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParallelBinding {
    pub symbol: Symbol,
    pub type_annotation: Option<TypeExpr>,
    pub expression: Box<Expression>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WithResourceExpr {
    // "[" ~ symbol ~ type_expr ~ expression ~ "]"
    pub resource_symbol: Symbol,
    pub resource_type: TypeExpr, // Type is mandatory in grammar
    pub resource_init: Box<Expression>,
    pub body: Vec<Expression>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TryCatchExpr {
    pub try_body: Vec<Expression>,
    pub catch_clauses: Vec<CatchClause>,
    pub finally_body: Option<Vec<Expression>>, // Optional in grammar
}

#[derive(Debug, PartialEq, Clone)]
pub struct CatchClause {
    pub pattern: CatchPattern, // This seems to be a separate enum
    pub binding: Symbol,
    pub body: Vec<Expression>,
}

#[derive(Debug, PartialEq, Clone)]
pub enum CatchPattern {
    Keyword(Keyword), // e.g. :Error
    Type(TypeExpr),   // e.g. :my.pkg/CustomErrorType
    Symbol(Symbol),   // e.g. AnyError - acts as a catch-all with binding
}

#[derive(Debug, PartialEq, Clone)]
pub struct MatchClause {
    pub pattern: MatchPattern, // Changed from Pattern
    pub guard: Option<Box<Expression>>,
    pub body: Vec<Expression>,
}

// Represents top-level definitions in a file
#[derive(Debug, Clone, PartialEq)]
pub enum TopLevel {
    Task(TaskDefinition),
    Module(ModuleDefinition),
    Expression(Expression), // Allow standalone expressions at top level?
                            // TODO: Add Import definition if needed at top level outside modules
}

// Placeholder structs for top-level items
#[derive(Debug, Clone, PartialEq)]
pub struct TaskDefinition {
    pub id: Option<String>, // Assuming ID is string literal
    pub source: Option<String>,
    pub timestamp: Option<String>,
    pub intent: Option<Expression>,
    pub contracts: Option<Expression>, // Likely a Map expression
    pub plan: Option<Expression>,
    pub execution_trace: Option<Expression>, // Likely a Vector expression
    pub metadata: Option<Expression>,        // Added metadata field
}

#[derive(Debug, Clone, PartialEq)]
pub struct ModuleDefinition {
    pub name: Symbol, // Namespaced identifier
    pub exports: Option<Vec<Symbol>>,
    pub definitions: Vec<ModuleLevelDefinition>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ModuleLevelDefinition {
    Def(DefExpr),
    Defn(DefnExpr),
    Import(ImportDefinition),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ImportDefinition {
    pub module_name: Symbol,       // Namespaced identifier
    pub alias: Option<Symbol>,     // :as alias
    pub only: Option<Vec<Symbol>>, // :only [sym1 sym2]
}
