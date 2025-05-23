use std::collections::HashMap;

// Using i64 for integers and f64 for floats as common defaults.
#[derive(Debug, Clone, PartialEq)]
pub enum Literal {
    Integer(i64),
    Float(f64),
    String(String),
    Boolean(bool),
    Nil,
}

// Represents identifiers, potentially namespaced (e.g., "my-var", "my.ns/func")
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Symbol(pub String);

// Represents keywords (e.g., ":my-key", ":my.ns/key")
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Keyword(pub String);

// Represents different types of map keys allowed by the grammar
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MapKey {
    Keyword(Keyword),
    String(String),
    Integer(i64),
}

// Represents a binding pattern used in let, fn, match
#[derive(Debug, Clone, PartialEq)]
pub enum Pattern {
    Symbol(Symbol),
    MapPattern(MapPattern),
    VectorPattern(VectorPattern),
    Literal(Literal), // For match patterns
    Wildcard,         // For match patterns: _
                      // TODO: Consider adding TypePattern for match?
}

// Updated MapPattern based on grammar
#[derive(Debug, Clone, PartialEq)]
pub struct MapPattern {
    // :keys [s1 s2 ...]
    pub keys: Vec<Symbol>, // Symbols bound directly from matching map keys
    // :kw pattern, "str" pattern, 123 pattern
    pub entries: Vec<(MapKey, Pattern)>, // Explicit key-pattern pairs
    // :or { default-symbol default-literal ... }
    pub or_defaults: Option<HashMap<Symbol, Literal>>, // Defaults for symbols if key missing
    // & rest-symbol
    pub rest: Option<Symbol>, // Bind remaining key-value pairs to a map symbol
    // :as whole-map-symbol
    pub as_binding: Option<Symbol>, // Bind the entire matched map to a symbol
}

// Updated VectorPattern based on grammar
#[derive(Debug, Clone, PartialEq)]
pub struct VectorPattern {
    // [p1 p2 ...]
    pub elements: Vec<Pattern>, // Patterns for positional elements
    // & rest-symbol
    pub rest: Option<Symbol>, // Bind remaining elements to a vector/list symbol
    // :as whole-vec-symbol
    pub as_binding: Option<Symbol>, // Bind the entire matched vector/list to a symbol
}

// Represents type expressions used in annotations
#[derive(Debug, Clone, PartialEq)]
pub enum TypeExpr {
    Primitive(PrimitiveType),
    Vector(Box<TypeExpr>), // [:vector ElementType]
    Map {
        // Ensure this is a struct variant
        entries: Vec<(Keyword, TypeExpr, bool)>, // [:map [key Type optional?]...]
        wildcard: Option<Box<TypeExpr>>,         // [:* WildcardType]?
    },
    Function {
        // Ensure this is a struct variant
        param_types: Vec<TypeExpr>,                 // Parameter types
        variadic_param_type: Option<Box<TypeExpr>>, // Type after &
        return_type: Box<TypeExpr>,                 // Return type
    },
    Resource(Symbol),            // [:resource Symbol]
    Union(Vec<TypeExpr>),        // [:or Type...]
    Intersection(Vec<TypeExpr>), // [:and Type...]
    Literal(Literal),            // [:val Literal]
    Any,                         // :any
    Never,                       // :never
    Alias(Symbol),               // Added missing Alias variant
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PrimitiveType {
    Int,
    Float,
    String,
    Bool,
    Nil,
    Keyword,
    Symbol,
}

// Represents a single binding in a `let` expression
#[derive(Debug, Clone, PartialEq)]
pub struct LetBinding {
    pub pattern: Pattern,
    pub type_annotation: Option<TypeExpr>,
    pub value: Box<Expression>,
}

// Represents the main expression types
#[derive(Debug, Clone, PartialEq)]
pub enum Expression {
    Literal(Literal),
    Symbol(Symbol),
    Keyword(Keyword),
    List(Vec<Expression>), // General list/potential function call
    Vector(Vec<Expression>),
    Map(HashMap<MapKey, Expression>),

    // Special Forms
    Let(LetExpr),
    If(IfExpr),
    Do(DoExpr),
    Fn(FnExpr),
    Def(DefExpr),
    Defn(DefnExpr),
    Parallel(ParallelExpr),         // Added
    WithResource(WithResourceExpr), // Added
    TryCatch(TryCatchExpr),         // Added
    Match(MatchExpr),               // Added
    LogStep(LogStepExpr),           // Added

    // Explicit Function Call (might be refined during parsing/semantic analysis)
    // For now, List might cover this implicitly based on context.
    FunctionCall {
        // Uncommented this variant
        function: Box<Expression>,
        arguments: Vec<Expression>, // Renamed from args to arguments
    },
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
    pub variadic_param: Option<Symbol>, // Symbol for the '& rest' binding
    pub return_type: Option<TypeExpr>,
    pub body: Vec<Expression>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParamDef {
    pub pattern: Pattern, // Parameter name or destructuring pattern
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
    pub variadic_param: Option<Symbol>,
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

#[derive(Debug, Clone, PartialEq)]
pub enum CatchPattern {
    // catch_pattern = _{ type_expr | keyword | symbol }
    Type(TypeExpr),
    Keyword(Keyword),
    Symbol(Symbol), // Catch-all or specific error symbol
}

#[derive(Debug, Clone, PartialEq)]
pub struct CatchClause {
    // "(" ~ "catch" ~ catch_pattern ~ symbol ~ expression+ ~ ")"
    pub pattern: CatchPattern,
    pub binding: Symbol, // Symbol to bind the caught error/exception object
    pub body: Vec<Expression>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MatchExpr {
    pub expression: Box<Expression>, // Expression to match against
    pub clauses: Vec<MatchClause>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MatchClause {
    // "(" ~ match_pattern ~ ("when" ~ expression)? ~ expression+ ~ ")"
    pub pattern: MatchPattern, // Using a distinct MatchPattern type
    pub guard: Option<Box<Expression>>, // Optional 'when' condition
    pub body: Vec<Expression>,
}

// Represents patterns specifically allowed in `match` clauses
#[derive(Debug, Clone, PartialEq)]
pub enum MatchPattern {
    Literal(Literal),
    Symbol(Symbol), // Can act as a binding or match specific symbol value (context-dependent)
    Keyword(Keyword),
    Wildcard,       // _
    Type(TypeExpr), // Match based on type (runtime check)
    Vector(VectorMatchPattern),
    Map(MapMatchPattern),
    As(Symbol, Box<MatchPattern>), // (:as name pattern)
}

// Vector pattern specific to match (allows nested match patterns)
#[derive(Debug, Clone, PartialEq)]
pub struct VectorMatchPattern {
    // vector_match_pattern = { "[" ~ match_pattern* ~ ("&" ~ symbol)? ~ "]" }
    pub elements: Vec<MatchPattern>,
    pub rest: Option<Symbol>, // Bind rest of elements
}

// Map pattern specific to match (allows nested match patterns)
#[derive(Debug, Clone, PartialEq)]
pub struct MapMatchPattern {
    // map_match_pattern = { "{" ~ map_match_pattern_entry* ~ ("&" ~ symbol)? ~ "}" }
    // map_match_pattern_entry = { map_key ~ match_pattern }
    pub entries: Vec<(MapKey, MatchPattern)>,
    pub rest: Option<Symbol>, // Bind rest of key-value pairs
}

#[derive(Debug, Clone, PartialEq)]
pub struct LogStepExpr {
    // "(" ~ "log-step" ~ ":id" ~ string ~ expression ~ ")"
    pub id: String, // ID is a string literal
    pub expression: Box<Expression>,
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
    pub metadata: Option<Expression>, // Added metadata field
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
