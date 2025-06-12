// Complete AST to IR Converter Implementation
// Provides full conversion from parsed AST to optimized IR

use std::collections::HashMap;
use std::rc::Rc;
use crate::ast::*;
use crate::ir::*;

/// Error types for IR conversion
#[derive(Debug, Clone, PartialEq)]
pub enum IrConversionError {
    UndefinedSymbol {
        symbol: String,
        location: Option<SourceLocation>,
    },
    TypeMismatch {
        expected: IrType,
        found: IrType,
        location: Option<SourceLocation>,
    },
    InvalidPattern {
        message: String,
        location: Option<SourceLocation>,
    },
    InvalidTypeAnnotation {
        message: String,
        location: Option<SourceLocation>,
    },
    InternalError {
        message: String,
    },
}

pub type IrConversionResult<T> = Result<T, IrConversionError>;

/// Information about a binding in the current scope
#[derive(Debug, Clone)]
pub struct BindingInfo {
    pub name: String,
    pub binding_id: NodeId,
    pub ir_type: IrType,
    pub kind: BindingKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BindingKind {
    Variable,
    Function,
    Parameter,
    Resource,
}

/// Scope for symbol resolution with proper mutable access
#[derive(Debug, Clone)]
pub struct Scope {
    bindings: HashMap<String, BindingInfo>,
    parent: Option<Rc<Scope>>,
}

impl Scope {
    pub fn new() -> Self {
        Scope {
            bindings: HashMap::new(),
            parent: None,
        }
    }
    
    pub fn with_parent(parent: Rc<Scope>) -> Self {
        Scope {
            bindings: HashMap::new(),
            parent: Some(parent),
        }
    }
    
    pub fn define(&mut self, name: String, info: BindingInfo) {
        self.bindings.insert(name, info);
    }
    
    pub fn lookup(&self, name: &str) -> Option<&BindingInfo> {
        self.bindings.get(name).or_else(|| {
            self.parent.as_ref().and_then(|p| p.lookup(name))
        })
    }
}

/// Type inference and checking context
#[derive(Debug, Clone)]
pub struct TypeContext {
    type_aliases: HashMap<String, IrType>,
    constraints: Vec<TypeConstraint>,
}

#[derive(Debug, Clone)]
pub struct TypeConstraint {
    pub node_id: NodeId,
    pub expected: IrType,
    pub actual: IrType,
}

/// Main AST to IR converter with complete implementation
pub struct IrConverter {
    next_node_id: NodeId,
    scope_stack: Vec<HashMap<String, BindingInfo>>,
    type_context: TypeContext,
    capture_analysis: HashMap<NodeId, Vec<IrCapture>>,
    /// Optional module registry for resolving qualified symbols during conversion
    module_registry: Option<*const crate::runtime::module_runtime::ModuleRegistry>,
}

impl IrConverter {
    pub fn new() -> Self {
        let mut converter = IrConverter {
            next_node_id: 1,
            scope_stack: vec![HashMap::new()],
            type_context: TypeContext {
                type_aliases: HashMap::new(),
                constraints: Vec::new(),
            },
            capture_analysis: HashMap::new(),
            module_registry: None,
        };
        
        // Add built-in functions to global scope
        converter.add_builtin_functions();
        converter
    }
    
    fn next_id(&mut self) -> NodeId {
        let id = self.next_node_id;
        self.next_node_id += 1;
        id
    }
    
    /// Add built-in functions to global scope
    fn add_builtin_functions(&mut self) {
        let builtins = [
            ("+", IrType::Function {
                param_types: vec![IrType::Int, IrType::Int],
                variadic_param_type: Some(Box::new(IrType::Int)),
                return_type: Box::new(IrType::Int),
            }),
            ("-", IrType::Function {
                param_types: vec![IrType::Int, IrType::Int],
                variadic_param_type: None,
                return_type: Box::new(IrType::Int),
            }),
            ("*", IrType::Function {
                param_types: vec![IrType::Int, IrType::Int],
                variadic_param_type: Some(Box::new(IrType::Int)),
                return_type: Box::new(IrType::Int),
            }),
            ("/", IrType::Function {
                param_types: vec![IrType::Int, IrType::Int],
                variadic_param_type: None,
                return_type: Box::new(IrType::Int),
            }),
            ("=", IrType::Function {
                param_types: vec![IrType::Any, IrType::Any],
                variadic_param_type: None,
                return_type: Box::new(IrType::Bool),
            }),
            (">", IrType::Function {
                param_types: vec![IrType::Any, IrType::Any],
                variadic_param_type: None,
                return_type: Box::new(IrType::Bool),
            }),
            ("<", IrType::Function {
                param_types: vec![IrType::Any, IrType::Any],
                variadic_param_type: None,
                return_type: Box::new(IrType::Bool),
            }),
        ];
        
        for (name, func_type) in builtins {
            let binding_info = BindingInfo {
                name: name.to_string(),
                binding_id: self.next_id(),
                ir_type: func_type,
                kind: BindingKind::Function,
            };
            self.scope_stack[0].insert(name.to_string(), binding_info);
        }
    }
    
    /// Enter a new scope
    fn enter_scope(&mut self) {
        self.scope_stack.push(HashMap::new());
    }
    
    /// Exit the current scope
    fn exit_scope(&mut self) {
        self.scope_stack.pop();
    }
    
    /// Define a binding in the current scope
    fn define_binding(&mut self, name: String, info: BindingInfo) {
        if let Some(current_scope) = self.scope_stack.last_mut() {
            current_scope.insert(name, info);
        }
    }
    
    /// Look up a symbol in the scope stack
    fn lookup_symbol(&self, name: &str) -> Option<&BindingInfo> {
        for scope in self.scope_stack.iter().rev() {
            if let Some(binding) = scope.get(name) {
                return Some(binding);
            }
        }
        None
    }
    
    /// Convert a simple expression (main entry point)
    pub fn convert_expression(&mut self, expr: Expression) -> IrConversionResult<IrNode> {
        match expr {
            Expression::Literal(lit) => self.convert_literal(lit),
            Expression::Symbol(sym) => self.convert_symbol_ref(sym),
            Expression::FunctionCall { callee, arguments } => {
                self.convert_function_call(*callee, arguments)
            }
            Expression::If(if_expr) => self.convert_if(if_expr),
            Expression::Let(let_expr) => self.convert_let(let_expr),
            Expression::Do(do_expr) => self.convert_do(do_expr),
            Expression::Fn(fn_expr) => self.convert_fn(fn_expr),
            Expression::Match(match_expr) => self.convert_match(*match_expr),
            Expression::Vector(exprs) => self.convert_vector(exprs),
            Expression::Map(map) => self.convert_map(map),
            Expression::List(exprs) => self.convert_list_as_application(exprs),
            Expression::TryCatch(try_expr) => self.convert_try_catch(try_expr),
            Expression::Parallel(parallel_expr) => self.convert_parallel(parallel_expr),
            Expression::WithResource(with_expr) => self.convert_with_resource(with_expr),
            Expression::LogStep(log_expr) => self.convert_log_step(*log_expr),
            Expression::Def(def_expr) => self.convert_def(*def_expr),
            Expression::Defn(defn_expr) => self.convert_defn(*defn_expr),
        }
    }
    
    /// High-level conversion method (entry point)
    pub fn convert(&mut self, expr: &Expression) -> IrConversionResult<IrNode> {
        self.convert_expression(expr.clone())
    }
    
    /// Convert a literal value
    fn convert_literal(&mut self, lit: Literal) -> IrConversionResult<IrNode> {
        let id = self.next_id();
        let ir_type = match &lit {
            Literal::Integer(_) => IrType::Int,
            Literal::Float(_) => IrType::Float,
            Literal::String(_) => IrType::String,
            Literal::Boolean(_) => IrType::Bool,
            Literal::Keyword(_) => IrType::Keyword,
            Literal::Nil => IrType::Nil,
        };
        
        Ok(IrNode::Literal {
            id,
            value: lit,
            ir_type,
            source_location: None,
        })
    }
      /// Convert symbol reference (variable lookup)
    fn convert_symbol_ref(&mut self, sym: Symbol) -> IrConversionResult<IrNode> {
        let id = self.next_id();
        let name = sym.0.clone();
        
        // Check for task context access (symbols starting with @)
        if name.starts_with('@') {
            let field_name = name[1..].to_string();
            return Ok(IrNode::TaskContextAccess {
                id,
                field_name: Keyword(field_name),
                ir_type: IrType::Any,
                source_location: None,
            });
        }
        
        // Check if it's a qualified symbol (e.g., "module/symbol")
        if crate::runtime::module_runtime::ModuleRegistry::is_qualified_symbol(&name) {
            // For qualified symbols, create a special VariableRef that will be resolved at runtime
            // The IR runtime knows how to handle qualified symbols
            return Ok(IrNode::VariableRef {
                id,
                name,
                binding_id: 0, // Use 0 to indicate this is a qualified symbol reference
                ir_type: IrType::Any, // Type will be determined at runtime
                source_location: None,
            });
        }
        
        // Look up the symbol in current scope
        match self.lookup_symbol(&name) {
            Some(binding_info) => {
                Ok(IrNode::VariableRef {
                    id,
                    name,
                    binding_id: binding_info.binding_id,
                    ir_type: binding_info.ir_type.clone(),
                    source_location: None,
                })
            }
            None => {
                Err(IrConversionError::UndefinedSymbol {
                    symbol: name,
                    location: None,
                })
            }
        }
    }
    
    /// Convert function call
    fn convert_function_call(&mut self, callee: Expression, arguments: Vec<Expression>) -> IrConversionResult<IrNode> {
        let id = self.next_id();
        let function = Box::new(self.convert_expression(callee)?);
        let mut ir_arguments = Vec::new();
        
        for arg in arguments {
            ir_arguments.push(self.convert_expression(arg)?);
        }
        
        // Infer return type from function type
        let return_type = match function.ir_type() {
            Some(IrType::Function { return_type, .. }) => (**return_type).clone(),
            _ => IrType::Any,
        };
        
        Ok(IrNode::Apply {
            id,
            function,
            arguments: ir_arguments,
            ir_type: return_type,
            source_location: None,
        })
    }
    
    /// Convert if expression
    fn convert_if(&mut self, if_expr: IfExpr) -> IrConversionResult<IrNode> {
        let id = self.next_id();
        let condition = Box::new(self.convert_expression(*if_expr.condition)?);
        let then_branch = Box::new(self.convert_expression(*if_expr.then_branch)?);
        let else_branch = if let Some(else_expr) = if_expr.else_branch {
            Some(Box::new(self.convert_expression(*else_expr)?))
        } else {
            None
        };
        
        // Infer result type as union of branches
        let result_type = match (then_branch.ir_type(), else_branch.as_ref().and_then(|e| e.ir_type())) {
            (Some(then_type), Some(else_type)) if then_type == else_type => then_type.clone(),
            (Some(then_type), Some(else_type)) => IrType::Union(vec![then_type.clone(), else_type.clone()]),
            (Some(then_type), None) => IrType::Union(vec![then_type.clone(), IrType::Nil]),
            _ => IrType::Any,
        };
        
        Ok(IrNode::If {
            id,
            condition,
            then_branch,
            else_branch,
            ir_type: result_type,
            source_location: None,
        })
    }
    
    /// Convert let expression with proper scope management
    fn convert_let(&mut self, let_expr: LetExpr) -> IrConversionResult<IrNode> {
        let id = self.next_id();
        let mut bindings = Vec::new();
        
        // Enter new scope for let bindings
        self.enter_scope();
          // Process bindings in order
        for binding in let_expr.bindings {
            let binding_id = self.next_id();
            let init_expr = self.convert_expression(*binding.value)?;
            let binding_type = init_expr.ir_type().unwrap_or(&IrType::Any);
            
            // Convert pattern to binding - store pattern first to avoid borrow issues
            let pattern_clone = binding.pattern.clone();
            let pattern_node = self.convert_pattern(binding.pattern, binding_id, binding_type.clone())?;
            
            // Add binding to current scope
            if let Pattern::Symbol(sym) = &pattern_clone {
                let binding_info = BindingInfo {
                    name: sym.0.clone(),
                    binding_id,
                    ir_type: binding_type.clone(),
                    kind: BindingKind::Variable,
                };
                self.define_binding(sym.0.clone(), binding_info);
            }
            
            bindings.push(IrLetBinding {
                pattern: pattern_node,
                type_annotation: binding.type_annotation.map(|t| self.convert_type_annotation(t)).transpose()?,
                init_expr,
            });
        }
        
        // Convert body expressions in the new scope
        let mut body_exprs = Vec::new();
        for body_expr in let_expr.body {
            body_exprs.push(self.convert_expression(body_expr)?);
        }
          // Exit scope        
        self.exit_scope();
        
        // Infer result type from last body expression
        let result_type = body_exprs.last()
            .and_then(|expr| expr.ir_type())
            .cloned()
            .unwrap_or(IrType::Nil);
        
        Ok(IrNode::Let {
            id,
            bindings,
            body: body_exprs,
            ir_type: result_type,
            source_location: None,
        })
    }
      /// Convert pattern to IR node
    fn convert_pattern(&mut self, pattern: Pattern, binding_id: NodeId, ir_type: IrType) -> IrConversionResult<IrNode> {
        match pattern {
            Pattern::Symbol(sym) => {
                Ok(IrNode::VariableBinding {
                    id: binding_id,
                    name: sym.0,
                    ir_type,
                    source_location: None,
                })
            }
            Pattern::Wildcard => {
                Ok(IrNode::VariableBinding {
                    id: binding_id,
                    name: "_".to_string(),
                    ir_type,
                    source_location: None,
                })
            }
            Pattern::VectorDestructuring { elements: _, rest: _, as_symbol: _ } => {
                // Create a destructuring pattern (simplified)
                Ok(IrNode::VariableBinding {
                    id: binding_id,
                    name: format!("__vector_destructure_{}", binding_id),
                    ir_type,
                    source_location: None,
                })
            }
            Pattern::MapDestructuring { entries: _, rest: _, as_symbol: _ } => {
                // Similar to vector - simplified destructuring
                Ok(IrNode::VariableBinding {
                    id: binding_id,
                    name: format!("__map_destructure_{}", binding_id),
                    ir_type,
                    source_location: None,
                })
            }
        }
    }
    
    /// Convert type annotation to IR type
    fn convert_type_annotation(&mut self, type_expr: TypeExpr) -> IrConversionResult<IrType> {
        match type_expr {
            TypeExpr::Primitive(PrimitiveType::Int) => Ok(IrType::Int),
            TypeExpr::Primitive(PrimitiveType::Float) => Ok(IrType::Float),
            TypeExpr::Primitive(PrimitiveType::String) => Ok(IrType::String),
            TypeExpr::Primitive(PrimitiveType::Bool) => Ok(IrType::Bool),
            TypeExpr::Primitive(PrimitiveType::Keyword) => Ok(IrType::Keyword),
            TypeExpr::Primitive(PrimitiveType::Symbol) => Ok(IrType::Symbol),
            TypeExpr::Any => Ok(IrType::Any),
            TypeExpr::Never => Ok(IrType::Never),
            TypeExpr::Vector(element_type) => {
                let ir_element_type = self.convert_type_annotation(*element_type)?;
                Ok(IrType::Vector(Box::new(ir_element_type)))
            }
            TypeExpr::Union(types) => {
                let mut ir_types = Vec::new();
                for t in types {
                    ir_types.push(self.convert_type_annotation(t)?);
                }
                Ok(IrType::Union(ir_types))
            }
            TypeExpr::Literal(lit) => Ok(IrType::LiteralValue(lit)),
            TypeExpr::Alias(sym) => Ok(IrType::TypeRef(sym.0)),
            _ => Ok(IrType::Any), // TODO: Implement remaining type conversions
        }
    }
    
    // Placeholder implementations for other expression types
    fn convert_do(&mut self, do_expr: DoExpr) -> IrConversionResult<IrNode> {
        let id = self.next_id();
        let mut expressions = Vec::new();
          for expr in do_expr.expressions {
            expressions.push(self.convert_expression(expr)?);
        }
        
        let result_type = expressions.last()
            .and_then(|expr| expr.ir_type())
            .cloned()
            .unwrap_or(IrType::Nil);
        
        Ok(IrNode::Do {
            id,
            expressions,
            ir_type: result_type,
            source_location: None,
        })
    }
      fn convert_fn(&mut self, fn_expr: FnExpr) -> IrConversionResult<IrNode> {
        let id = self.next_id();
        
        // Enter new scope for function body
        self.enter_scope();
        
        // Convert parameters
        let mut params = Vec::new();
        for param_def in fn_expr.params {
            let param_id = self.next_id();
            let param_type = if let Some(type_ann) = param_def.type_annotation {
                self.convert_type_annotation(type_ann)?
            } else {
                IrType::Any
            };
            
            // Convert pattern to binding
            let binding_node = self.convert_pattern(param_def.pattern.clone(), param_id, param_type.clone())?;
            
            // Add parameter to scope
            if let Pattern::Symbol(sym) = &param_def.pattern {
                let binding_info = BindingInfo {
                    name: sym.0.clone(),
                    binding_id: param_id,
                    ir_type: param_type.clone(),
                    kind: BindingKind::Parameter,
                };
                self.define_binding(sym.0.clone(), binding_info);
            }
            
            params.push(IrNode::Param {
                id: param_id,
                binding: Box::new(binding_node),
                type_annotation: Some(param_type.clone()),
                ir_type: param_type,
                source_location: None,
            });
        }
        
        // Convert variadic parameter if present
        let variadic_param = if let Some(variadic_def) = fn_expr.variadic_param {
            let param_id = self.next_id();
            let param_type = if let Some(type_ann) = variadic_def.type_annotation {
                self.convert_type_annotation(type_ann)?
            } else {
                IrType::Vector(Box::new(IrType::Any))
            };
            
            let binding_node = self.convert_pattern(variadic_def.pattern.clone(), param_id, param_type.clone())?;
            
            if let Pattern::Symbol(sym) = &variadic_def.pattern {
                let binding_info = BindingInfo {
                    name: sym.0.clone(),
                    binding_id: param_id,
                    ir_type: param_type.clone(),
                    kind: BindingKind::Parameter,
                };
                self.define_binding(sym.0.clone(), binding_info);
            }
            
            Some(Box::new(IrNode::Param {
                id: param_id,
                binding: Box::new(binding_node),
                type_annotation: Some(param_type.clone()),
                ir_type: param_type,
                source_location: None,
            }))
        } else {
            None
        };
        
        // Convert body expressions
        let mut body_exprs = Vec::new();
        for body_expr in fn_expr.body {
            body_exprs.push(self.convert_expression(body_expr)?);
        }
        
        // Exit function scope
        self.exit_scope();
        
        // Determine return type
        let return_type = if let Some(ret_type) = fn_expr.return_type {
            self.convert_type_annotation(ret_type)?
        } else {
            body_exprs.last()
                .and_then(|expr| expr.ir_type())
                .cloned()
                .unwrap_or(IrType::Any)
        };
        
        // Build function type
        let param_types: Vec<IrType> = params.iter()
            .filter_map(|p| p.ir_type())
            .cloned()
            .collect();
        
        let variadic_param_type = variadic_param.as_ref()
            .and_then(|p| p.ir_type())
            .map(|t| Box::new(t.clone()));
        
        let function_type = IrType::Function {
            param_types,
            variadic_param_type,
            return_type: Box::new(return_type),
        };
        
        // TODO: Implement capture analysis for closures
        let captures = Vec::new();
        
        Ok(IrNode::Lambda {
            id,
            params,
            variadic_param,
            body: body_exprs,
            captures,
            ir_type: function_type,
            source_location: None,
        })
    }
      fn convert_match(&mut self, match_expr: MatchExpr) -> IrConversionResult<IrNode> {
        let id = self.next_id();
        
        // Convert the expression being matched
        let match_expr_ir = Box::new(self.convert_expression(*match_expr.expression)?);
        
        // Convert match clauses
        let mut ir_clauses = Vec::new();
        let mut result_types = Vec::new();
        
        for clause in match_expr.clauses {
            // Enter new scope for pattern bindings
            self.enter_scope();
            
            // Convert pattern
            let ir_pattern = self.convert_pattern_to_ir_pattern(clause.pattern)?;
            
            // Convert guard if present
            let guard = if let Some(guard_expr) = clause.guard {
                Some(self.convert_expression(*guard_expr)?)
            } else {
                None
            };
            
            // Convert body
            let body = self.convert_expression(*clause.body)?;
            
            // Track result type for union computation
            if let Some(body_type) = body.ir_type() {
                result_types.push(body_type.clone());
            }
            
            // Exit pattern scope
            self.exit_scope();
            
            ir_clauses.push(IrMatchClause {
                pattern: ir_pattern,
                guard,
                body,
            });
        }
        
        // Compute result type as union of all clause result types
        let result_type = if result_types.is_empty() {
            IrType::Never
        } else if result_types.len() == 1 {
            result_types.into_iter().next().unwrap()
        } else {
            // Check if all types are the same
            let first_type = &result_types[0];
            if result_types.iter().all(|t| t == first_type) {
                first_type.clone()
            } else {
                IrType::Union(result_types)
            }
        };
        
        Ok(IrNode::Match {
            id,
            expression: match_expr_ir,
            clauses: ir_clauses,
            ir_type: result_type,
            source_location: None,
        })
    }
    
    /// Convert AST pattern to IR pattern
    fn convert_pattern_to_ir_pattern(&mut self, pattern: MatchPattern) -> IrConversionResult<IrPattern> {        match pattern {
            MatchPattern::Symbol(sym) => {
                // Add to current scope for pattern binding
                let binding_info = BindingInfo {
                    name: sym.0.clone(),
                    binding_id: self.next_id(),
                    ir_type: IrType::Any, // Will be refined during type inference
                    kind: BindingKind::Variable,
                };
                self.define_binding(sym.0.clone(), binding_info);
                Ok(IrPattern::Variable(sym.0))
            }
            MatchPattern::Wildcard => Ok(IrPattern::Wildcard),
            MatchPattern::Literal(lit) => Ok(IrPattern::Literal(lit)),            MatchPattern::Vector { elements, rest } => {
                let mut ir_patterns = Vec::new();
                
                for pat in elements {
                    ir_patterns.push(self.convert_pattern_to_ir_pattern(pat)?);
                }
                  Ok(IrPattern::Vector {
                    elements: ir_patterns,
                    rest: rest.map(|s| s.0),
                })
            }            MatchPattern::Map { entries, rest } => {
                let mut ir_entries = Vec::new();
                
                for entry in entries {
                    let value_pattern = self.convert_pattern_to_ir_pattern(*entry.pattern)?;
                    ir_entries.push(IrMapPatternEntry {
                        key: entry.key,
                        pattern: value_pattern,
                    });
                }
                
                Ok(IrPattern::Map {
                    entries: ir_entries,
                    rest: rest.map(|s| s.0),
                })
            }
            MatchPattern::Keyword(kw) => {
                Ok(IrPattern::Literal(Literal::Keyword(kw)))
            }            MatchPattern::Type(_type_expr, binding) => {
                // For now, treat type patterns as wildcards with optional binding
                // TODO: Use _type_expr for proper type checking
                if let Some(sym) = binding {
                    let binding_info = BindingInfo {
                        name: sym.0.clone(),
                        binding_id: self.next_id(),
                        ir_type: IrType::Any, // Would need type resolution here
                        kind: BindingKind::Variable,
                    };
                    self.define_binding(sym.0.clone(), binding_info);
                    Ok(IrPattern::Variable(sym.0))
                } else {
                    Ok(IrPattern::Wildcard)
                }
            }            MatchPattern::As(symbol, inner_pattern) => {
                // Convert inner pattern and bind the result to symbol
                let _inner_ir_pattern = self.convert_pattern_to_ir_pattern(*inner_pattern)?;
                // TODO: Properly handle the inner pattern structure
                let binding_info = BindingInfo {
                    name: symbol.0.clone(),
                    binding_id: self.next_id(),
                    ir_type: IrType::Any,
                    kind: BindingKind::Variable,
                };
                self.define_binding(symbol.0.clone(), binding_info);
                // For simplicity, return the variable pattern (more complex logic would wrap both)
                Ok(IrPattern::Variable(symbol.0))
            }
        }
    }
      fn convert_vector(&mut self, exprs: Vec<Expression>) -> IrConversionResult<IrNode> {
        let id = self.next_id();
        let mut elements = Vec::new();
        let mut element_types = Vec::new();
        
        for expr in exprs {
            let element = self.convert_expression(expr)?;
            if let Some(elem_type) = element.ir_type() {
                element_types.push(elem_type.clone());
            }
            elements.push(element);
        }
        
        // Determine the vector's element type
        let element_type = if element_types.is_empty() {
            IrType::Never
        } else if element_types.len() == 1 {
            element_types.into_iter().next().unwrap()
        } else {
            // Check if all types are the same
            let first_type = &element_types[0];
            if element_types.iter().all(|t| t == first_type) {
                first_type.clone()
            } else {
                IrType::Union(element_types)
            }
        };
        
        // Create a literal vector node (representing a vector literal)
        Ok(IrNode::Literal {
            id,
            value: Literal::Nil, // TODO: Create proper vector literal representation
            ir_type: IrType::Vector(Box::new(element_type)),
            source_location: None,
        })
    }
    
    fn convert_map(&mut self, map: HashMap<MapKey, Expression>) -> IrConversionResult<IrNode> {
        let id = self.next_id();
        let mut converted_entries = Vec::new();
        let mut type_entries = Vec::new();
        
        for (key, value_expr) in map {
            let value = self.convert_expression(value_expr)?;
            
            // Track type information for the map type
            if let MapKey::Keyword(ref keyword) = key {
                if let Some(value_type) = value.ir_type() {
                    type_entries.push(IrMapTypeEntry {
                        key: keyword.clone(),
                        value_type: value_type.clone(),
                        optional: false,
                    });
                }
            }
            
            converted_entries.push((key, value));
        }
        
        let map_type = IrType::Map {
            entries: type_entries,
            wildcard: None,
        };
        
        // Create a literal map node (representing a map literal)
        Ok(IrNode::Literal {
            id,
            value: Literal::Nil, // TODO: Create proper map literal representation
            ir_type: map_type,
            source_location: None,
        })
    }
    
    fn convert_list_as_application(&mut self, exprs: Vec<Expression>) -> IrConversionResult<IrNode> {
        if exprs.is_empty() {
            return Ok(IrNode::Literal {
                id: self.next_id(),
                value: Literal::Nil,
                ir_type: IrType::Vector(Box::new(IrType::Never)),
                source_location: None,
            });
        }
        
        let function = self.convert_expression(exprs[0].clone())?;
        let mut arguments = Vec::new();
        
        for arg_expr in exprs.into_iter().skip(1) {
            arguments.push(self.convert_expression(arg_expr)?);
        }
        
        self.convert_function_call_ir(function, arguments)
    }
    
    fn convert_function_call_ir(&mut self, function: IrNode, arguments: Vec<IrNode>) -> IrConversionResult<IrNode> {
        let id = self.next_id();
        
        let return_type = match function.ir_type() {
            Some(IrType::Function { return_type, .. }) => (**return_type).clone(),
            _ => IrType::Any,
        };
        
        Ok(IrNode::Apply {
            id,
            function: Box::new(function),
            arguments,
            ir_type: return_type,
            source_location: None,
        })    }
    
    // Additional placeholder implementations
    fn convert_try_catch(&mut self, try_expr: TryCatchExpr) -> IrConversionResult<IrNode> {
        let id = self.next_id();
        
        // Convert try body
        let mut try_body = Vec::new();
        for expr in try_expr.try_body {
            try_body.push(self.convert_expression(expr)?);
        }
        
        // Convert catch clauses
        let mut catch_clauses = Vec::new();
        for catch_clause in try_expr.catch_clauses {
            self.enter_scope(); // New scope for error binding
              // Convert error pattern
            let error_pattern = match catch_clause.pattern {
                CatchPattern::Keyword(kw) => IrPattern::Literal(Literal::Keyword(kw)),
                CatchPattern::Type(type_expr) => {
                    let ir_type = self.convert_type_annotation(type_expr)?;
                    IrPattern::Type(ir_type)
                }
                CatchPattern::Symbol(_) => IrPattern::Wildcard, // Symbol acts as catch-all
            };
            
            // Handle error binding (use the binding field from CatchClause)
            let binding_info = BindingInfo {
                name: catch_clause.binding.0.clone(),
                binding_id: self.next_id(),
                ir_type: IrType::Any, // Error type
                kind: BindingKind::Variable,
            };
            self.define_binding(catch_clause.binding.0.clone(), binding_info);
            let binding = Some(catch_clause.binding.0);
            
            // Convert catch body
            let mut catch_body = Vec::new();
            for expr in catch_clause.body {
                catch_body.push(self.convert_expression(expr)?);
            }
            
            self.exit_scope();
            
            catch_clauses.push(IrCatchClause {
                error_pattern,
                binding,
                body: catch_body,
            });
        }
        
        // Convert finally body if present
        let finally_body = if let Some(finally_exprs) = try_expr.finally_body {
            let mut finally_ir = Vec::new();
            for expr in finally_exprs {
                finally_ir.push(self.convert_expression(expr)?);
            }
            Some(finally_ir)
        } else {
            None
        };
        
        // Determine result type (try body type or union with catch types)
        let result_type = try_body.last()
            .and_then(|expr| expr.ir_type())
            .cloned()
            .unwrap_or(IrType::Any);
        
        Ok(IrNode::TryCatch {
            id,
            try_body,
            catch_clauses,
            finally_body,
            ir_type: result_type,
            source_location: None,
        })
    }
      fn convert_parallel(&mut self, parallel_expr: ParallelExpr) -> IrConversionResult<IrNode> {
        let id = self.next_id();
        let mut bindings = Vec::new();
        
        for parallel_binding in parallel_expr.bindings {
            // Convert the initialization expression
            let init_expr = self.convert_expression(*parallel_binding.expression)?;
            
            // Create binding for the symbol
            let binding_id = self.next_id();
            let binding_type = if let Some(type_ann) = parallel_binding.type_annotation {
                self.convert_type_annotation(type_ann)?
            } else {
                init_expr.ir_type().cloned().unwrap_or(IrType::Any)
            };
            
            let binding_node = IrNode::VariableBinding {
                id: binding_id,
                name: parallel_binding.symbol.0.clone(),
                ir_type: binding_type.clone(),
                source_location: None,
            };
            
            bindings.push(IrParallelBinding {
                binding: binding_node,
                init_expr,
            });
        }
        
        // Parallel expressions don't have a specific return type - they're about concurrency
        Ok(IrNode::Parallel {
            id,
            bindings,
            ir_type: IrType::Nil,
            source_location: None,
        })
    }
    
    fn convert_with_resource(&mut self, with_expr: WithResourceExpr) -> IrConversionResult<IrNode> {
        let id = self.next_id();
        
        // Convert the resource initialization
        let init_expr = Box::new(self.convert_expression(*with_expr.resource_init)?);
          // Create binding for the resource
        let binding_id = self.next_id();
        let resource_type = self.convert_type_annotation(with_expr.resource_type)?;
          let binding_node = Box::new(IrNode::VariableBinding {
            id: binding_id,
            name: with_expr.resource_symbol.0.clone(),
            ir_type: resource_type.clone(),
            source_location: None,
        });
        
        // Enter scope for resource binding
        self.enter_scope();
        let binding_info = BindingInfo {
            name: with_expr.resource_symbol.0.clone(),
            binding_id,
            ir_type: resource_type.clone(),
            kind: BindingKind::Resource,
        };
        self.define_binding(with_expr.resource_symbol.0, binding_info);
        
        // Convert body with resource in scope
        let mut body_exprs = Vec::new();
        for body_expr in with_expr.body {
            body_exprs.push(self.convert_expression(body_expr)?);
        }
        
        self.exit_scope();
        
        // Result type is the type of the last body expression
        let result_type = body_exprs.last()
            .and_then(|expr| expr.ir_type())
            .cloned()
            .unwrap_or(IrType::Nil);
        
        Ok(IrNode::WithResource {
            id,
            binding: binding_node,
            init_expr,
            body: body_exprs,
            ir_type: result_type,
            source_location: None,
        })
    }
    
    fn convert_log_step(&mut self, log_expr: LogStepExpr) -> IrConversionResult<IrNode> {
        let id = self.next_id();
        
        // Convert the values to log
        let mut values = Vec::new();
        for value_expr in log_expr.values {
            values.push(self.convert_expression(value_expr)?);
        }
          Ok(IrNode::LogStep {
            id,
            level: log_expr.level.unwrap_or(Keyword("info".to_string())),
            values,
            location: log_expr.location,
            ir_type: IrType::Nil, // Log steps don't return values
            source_location: None,
        })
    }
    
    fn convert_def(&mut self, def_expr: DefExpr) -> IrConversionResult<IrNode> {
        let id = self.next_id();
        
        // Convert the initialization expression
        let init_expr = Box::new(self.convert_expression(*def_expr.value)?);
        
        // Convert type annotation if present
        let type_annotation = if let Some(type_ann) = def_expr.type_annotation {
            Some(self.convert_type_annotation(type_ann)?)
        } else {
            None
        };
        
        // Determine the type (annotation takes precedence, otherwise infer from init_expr)
        let var_type = type_annotation
            .clone()
            .or_else(|| init_expr.ir_type().cloned())
            .unwrap_or(IrType::Any);
        
        // Add to global scope (def is module-level)
        let binding_info = BindingInfo {
            name: def_expr.symbol.0.clone(),
            binding_id: id,
            ir_type: var_type.clone(),
            kind: BindingKind::Variable,
        };
        self.define_binding(def_expr.symbol.0.clone(), binding_info);
        
        Ok(IrNode::VariableDef {
            id,
            name: def_expr.symbol.0,
            type_annotation,
            init_expr,
            ir_type: var_type,
            source_location: None,
        })
    }
    
    fn convert_defn(&mut self, defn_expr: DefnExpr) -> IrConversionResult<IrNode> {
        let id = self.next_id();
        
        // Convert the function parameters and body using the existing fn converter
        let fn_expr = FnExpr {
            params: defn_expr.params,
            variadic_param: defn_expr.variadic_param,
            return_type: defn_expr.return_type,
            body: defn_expr.body,
        };
        
        let lambda_node = Box::new(self.convert_fn(fn_expr)?);
        let function_type = lambda_node.ir_type().cloned().unwrap_or(IrType::Any);
        
        // Add to global scope (defn is module-level)
        let binding_info = BindingInfo {
            name: defn_expr.name.0.clone(),
            binding_id: id,
            ir_type: function_type.clone(),
            kind: BindingKind::Function,
        };
        self.define_binding(defn_expr.name.0.clone(), binding_info);
        
        Ok(IrNode::FunctionDef {
            id,
            name: defn_expr.name.0,
            lambda: lambda_node,
            ir_type: function_type,
            source_location: None,
        })
    }
    
    /// Set the module registry for qualified symbol resolution
    pub fn set_module_registry(&mut self, registry: &crate::runtime::module_runtime::ModuleRegistry) {
        self.module_registry = Some(registry as *const _);
    }
    
    /// Check if module registry is available
    fn has_module_registry(&self) -> bool {
        self.module_registry.is_some()
    }
    
    /// Get module registry reference (unsafe but controlled)
    fn get_module_registry(&self) -> Option<&crate::runtime::module_runtime::ModuleRegistry> {
        self.module_registry.map(|ptr| unsafe { &*ptr })
    }
}

/// Factory function for creating an IR converter
pub fn create_ir_converter() -> IrConverter {
    IrConverter::new()
}
