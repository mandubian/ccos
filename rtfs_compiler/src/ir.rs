// RTFS Intermediate Representation (IR)
// Typed, canonical representation of RTFS programs

use std::collections::HashMap;
use std::rc::Rc;
use crate::ast::{Symbol, Keyword, MapKey, Literal};

/// Unique identifier for IR nodes (for scope resolution and linking)
pub type NodeId = u64;

/// Source location information for error reporting
#[derive(Debug, Clone, PartialEq)]
pub struct SourceLocation {
    pub line: usize,
    pub column: usize,
    pub file: Option<String>,
}

/// IR Type system - represents resolved types
#[derive(Debug, Clone, PartialEq)]
pub enum IrType {
    // Primitive types
    Int,
    Float,
    String,
    Bool,
    Nil,
    Keyword,
    Symbol,
    Any,
    Never,
    
    // Collection types
    Vector(Box<IrType>),
    List(Box<IrType>),
    Tuple(Vec<IrType>),
    Map {
        entries: Vec<IrMapTypeEntry>,
        wildcard: Option<Box<IrType>>,
    },
    
    // Function types
    Function {
        param_types: Vec<IrType>,
        variadic_param_type: Option<Box<IrType>>,
        return_type: Box<IrType>,
    },
    
    // Advanced types
    Union(Vec<IrType>),
    Intersection(Vec<IrType>),
    Resource(String),
    LiteralValue(Literal),
    
    // Type references (for aliases and forward declarations)
    TypeRef(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct IrMapTypeEntry {
    pub key: Keyword,
    pub value_type: IrType,
    pub optional: bool,
}

/// Core IR Node structure
#[derive(Debug, Clone, PartialEq)]
pub enum IrNode {
    // Program structure
    Program {
        id: NodeId,
        version: String,
        forms: Vec<IrNode>,
        source_location: Option<SourceLocation>,
    },
    
    // Literals
    Literal {
        id: NodeId,
        value: Literal,
        ir_type: IrType,
        source_location: Option<SourceLocation>,
    },
    
    // Variable operations
    VariableRef {
        id: NodeId,
        name: String,
        binding_id: NodeId, // Points to the defining binding
        ir_type: IrType,
        source_location: Option<SourceLocation>,
    },
    
    VariableBinding {
        id: NodeId,
        name: String,
        ir_type: IrType,
        source_location: Option<SourceLocation>,
    },
    
    // Function operations
    Apply {
        id: NodeId,
        function: Box<IrNode>,
        arguments: Vec<IrNode>,
        ir_type: IrType,
        source_location: Option<SourceLocation>,
    },
    
    Lambda {
        id: NodeId,
        params: Vec<IrNode>, // IrParam nodes
        variadic_param: Option<Box<IrNode>>,
        body: Vec<IrNode>,
        captures: Vec<IrCapture>,
        ir_type: IrType,
        source_location: Option<SourceLocation>,
    },
    
    Param {
        id: NodeId,
        binding: Box<IrNode>, // Can be VariableBinding or destructuring pattern
        type_annotation: Option<IrType>,
        ir_type: IrType,
        source_location: Option<SourceLocation>,
    },
    
    // Control flow
    If {
        id: NodeId,
        condition: Box<IrNode>,
        then_branch: Box<IrNode>,
        else_branch: Option<Box<IrNode>>,
        ir_type: IrType,
        source_location: Option<SourceLocation>,
    },
    
    Let {
        id: NodeId,
        bindings: Vec<IrLetBinding>,
        body: Vec<IrNode>,
        ir_type: IrType,
        source_location: Option<SourceLocation>,
    },
    
    Do {
        id: NodeId,
        expressions: Vec<IrNode>,
        ir_type: IrType,
        source_location: Option<SourceLocation>,
    },
    
    // Pattern matching
    Match {
        id: NodeId,
        expression: Box<IrNode>,
        clauses: Vec<IrMatchClause>,
        ir_type: IrType,
        source_location: Option<SourceLocation>,
    },
    
    // Advanced constructs
    TryCatch {
        id: NodeId,
        try_body: Vec<IrNode>,
        catch_clauses: Vec<IrCatchClause>,
        finally_body: Option<Vec<IrNode>>,
        ir_type: IrType,
        source_location: Option<SourceLocation>,
    },
    
    Parallel {
        id: NodeId,
        bindings: Vec<IrParallelBinding>,
        ir_type: IrType,
        source_location: Option<SourceLocation>,
    },
    
    WithResource {
        id: NodeId,
        binding: Box<IrNode>,
        init_expr: Box<IrNode>,
        body: Vec<IrNode>,
        ir_type: IrType,
        source_location: Option<SourceLocation>,
    },
    
    LogStep {
        id: NodeId,
        level: Keyword,
        values: Vec<IrNode>,
        location: Option<String>,
        ir_type: IrType,
        source_location: Option<SourceLocation>,
    },
    
    // Module system
    Module {
        id: NodeId,
        name: String,
        exports: Vec<String>,
        definitions: Vec<IrNode>,
        source_location: Option<SourceLocation>,
    },
    
    FunctionDef {
        id: NodeId,
        name: String,
        lambda: Box<IrNode>,
        ir_type: IrType,
        source_location: Option<SourceLocation>,
    },
    
    VariableDef {
        id: NodeId,
        name: String,
        type_annotation: Option<IrType>,
        init_expr: Box<IrNode>,
        ir_type: IrType,
        source_location: Option<SourceLocation>,
    },
    
    Import {
        id: NodeId,
        module_name: String,
        alias: Option<String>,
        imports: Option<Vec<String>>,
        source_location: Option<SourceLocation>,
    },
    
    // Task system
    Task {
        id: NodeId,
        task_id: String,
        metadata: HashMap<String, IrNode>,
        intent: Box<IrNode>,
        contracts: Box<IrNode>,
        plan: Box<IrNode>,
        execution_trace: Vec<IrNode>,
        ir_type: IrType,
        source_location: Option<SourceLocation>,
    },
    
    TaskContextAccess {
        id: NodeId,
        field_name: Keyword,
        ir_type: IrType,
        source_location: Option<SourceLocation>,
    },
}

/// Captured variable information for closures
#[derive(Debug, Clone, PartialEq)]
pub struct IrCapture {
    pub name: String,
    pub binding_id: NodeId,
    pub ir_type: IrType,
}

/// Let binding in IR
#[derive(Debug, Clone, PartialEq)]
pub struct IrLetBinding {
    pub pattern: IrNode, // Can be VariableBinding or destructuring pattern
    pub type_annotation: Option<IrType>,
    pub init_expr: IrNode,
}

/// Match clause in IR
#[derive(Debug, Clone, PartialEq)]
pub struct IrMatchClause {
    pub pattern: IrPattern,
    pub guard: Option<IrNode>,
    pub body: IrNode,
}

/// Pattern matching patterns
#[derive(Debug, Clone, PartialEq)]
pub enum IrPattern {
    Literal(Literal),
    Variable(String),
    Wildcard,
    Vector {
        elements: Vec<IrPattern>,
        rest: Option<String>,
    },
    Map {
        entries: Vec<IrMapPatternEntry>,
        rest: Option<String>,
    },
    Type(IrType),
}

#[derive(Debug, Clone, PartialEq)]
pub struct IrMapPatternEntry {
    pub key: MapKey,
    pub pattern: IrPattern,
}

/// Catch clause in try-catch
#[derive(Debug, Clone, PartialEq)]
pub struct IrCatchClause {
    pub error_pattern: IrPattern,
    pub binding: Option<String>,
    pub body: Vec<IrNode>,
}

/// Parallel binding
#[derive(Debug, Clone, PartialEq)]
pub struct IrParallelBinding {
    pub binding: IrNode,
    pub init_expr: IrNode,
}

impl IrNode {
    /// Get the unique ID of this node
    pub fn id(&self) -> NodeId {
        match self {
            IrNode::Program { id, .. } => *id,
            IrNode::Literal { id, .. } => *id,
            IrNode::VariableRef { id, .. } => *id,
            IrNode::VariableBinding { id, .. } => *id,
            IrNode::Apply { id, .. } => *id,
            IrNode::Lambda { id, .. } => *id,
            IrNode::Param { id, .. } => *id,
            IrNode::If { id, .. } => *id,
            IrNode::Let { id, .. } => *id,
            IrNode::Do { id, .. } => *id,
            IrNode::Match { id, .. } => *id,
            IrNode::TryCatch { id, .. } => *id,
            IrNode::Parallel { id, .. } => *id,
            IrNode::WithResource { id, .. } => *id,
            IrNode::LogStep { id, .. } => *id,
            IrNode::Module { id, .. } => *id,
            IrNode::FunctionDef { id, .. } => *id,
            IrNode::VariableDef { id, .. } => *id,
            IrNode::Import { id, .. } => *id,
            IrNode::Task { id, .. } => *id,
            IrNode::TaskContextAccess { id, .. } => *id,
        }
    }
    
    /// Get the type of this node (if it has one)
    pub fn ir_type(&self) -> Option<&IrType> {
        match self {
            IrNode::Literal { ir_type, .. } => Some(ir_type),
            IrNode::VariableRef { ir_type, .. } => Some(ir_type),
            IrNode::VariableBinding { ir_type, .. } => Some(ir_type),
            IrNode::Apply { ir_type, .. } => Some(ir_type),
            IrNode::Lambda { ir_type, .. } => Some(ir_type),
            IrNode::Param { ir_type, .. } => Some(ir_type),
            IrNode::If { ir_type, .. } => Some(ir_type),
            IrNode::Let { ir_type, .. } => Some(ir_type),
            IrNode::Do { ir_type, .. } => Some(ir_type),
            IrNode::Match { ir_type, .. } => Some(ir_type),
            IrNode::TryCatch { ir_type, .. } => Some(ir_type),
            IrNode::Parallel { ir_type, .. } => Some(ir_type),
            IrNode::WithResource { ir_type, .. } => Some(ir_type),
            IrNode::LogStep { ir_type, .. } => Some(ir_type),
            IrNode::FunctionDef { ir_type, .. } => Some(ir_type),
            IrNode::VariableDef { ir_type, .. } => Some(ir_type),
            IrNode::Task { ir_type, .. } => Some(ir_type),
            IrNode::TaskContextAccess { ir_type, .. } => Some(ir_type),
            _ => None,
        }
    }
    
    /// Get source location if available
    pub fn source_location(&self) -> Option<&SourceLocation> {
        match self {
            IrNode::Program { source_location, .. } => source_location.as_ref(),
            IrNode::Literal { source_location, .. } => source_location.as_ref(),
            IrNode::VariableRef { source_location, .. } => source_location.as_ref(),
            IrNode::VariableBinding { source_location, .. } => source_location.as_ref(),
            IrNode::Apply { source_location, .. } => source_location.as_ref(),
            IrNode::Lambda { source_location, .. } => source_location.as_ref(),
            IrNode::Param { source_location, .. } => source_location.as_ref(),
            IrNode::If { source_location, .. } => source_location.as_ref(),
            IrNode::Let { source_location, .. } => source_location.as_ref(),
            IrNode::Do { source_location, .. } => source_location.as_ref(),
            IrNode::Match { source_location, .. } => source_location.as_ref(),
            IrNode::TryCatch { source_location, .. } => source_location.as_ref(),
            IrNode::Parallel { source_location, .. } => source_location.as_ref(),
            IrNode::WithResource { source_location, .. } => source_location.as_ref(),
            IrNode::LogStep { source_location, .. } => source_location.as_ref(),
            IrNode::Module { source_location, .. } => source_location.as_ref(),
            IrNode::FunctionDef { source_location, .. } => source_location.as_ref(),
            IrNode::VariableDef { source_location, .. } => source_location.as_ref(),
            IrNode::Import { source_location, .. } => source_location.as_ref(),
            IrNode::Task { source_location, .. } => source_location.as_ref(),
            IrNode::TaskContextAccess { source_location, .. } => source_location.as_ref(),
        }
    }
}
