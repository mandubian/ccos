// IR Optimization Passes
// Applies various optimizations to the typed IR for better runtime performance

use std::collections::HashMap;
use crate::ir::*;
use crate::ast::Literal;

/// Optimization pass trait
pub trait OptimizationPass {
    fn optimize(&mut self, node: IrNode) -> IrNode;
    fn name(&self) -> &'static str;
}

/// Optimization pipeline that applies multiple passes
pub struct OptimizationPipeline {
    passes: Vec<Box<dyn OptimizationPass>>,
    stats: OptimizationStats,
}

#[derive(Debug, Default)]
pub struct OptimizationStats {
    pub nodes_processed: usize,
    pub constants_folded: usize,
    pub dead_code_eliminated: usize,
    pub function_calls_inlined: usize,
    pub type_specializations: usize,
}

impl OptimizationPipeline {
    pub fn new() -> Self {
        OptimizationPipeline {
            passes: Vec::new(),
            stats: OptimizationStats::default(),
        }
    }
    
    /// Add an optimization pass to the pipeline
    pub fn add_pass<P: OptimizationPass + 'static>(&mut self, pass: P) {
        self.passes.push(Box::new(pass));
    }
    
    /// Create a standard optimization pipeline
    pub fn standard() -> Self {
        let mut pipeline = Self::new();
        pipeline.add_pass(ConstantFoldingPass::new());
        pipeline.add_pass(DeadCodeEliminationPass::new());
        pipeline.add_pass(TypeSpecializationPass::new());
        pipeline.add_pass(InliningPass::new());
        pipeline
    }    /// Apply all optimization passes to an IR node
    pub fn optimize(&mut self, mut node: IrNode) -> IrNode {
        let mut iterations = 0;
        const MAX_ITERATIONS: usize = 2; // Reduced to avoid excessive optimization
        
        loop {
            let mut changed = false;
            let original_passes_len = self.passes.len();
              for i in 0..original_passes_len {
                let original_id = node.id();
                let optimized = self.passes[i].optimize(node);
                
                // Simple check - if node IDs are different, something changed
                if optimized.id() != original_id {
                    changed = true;
                }
                
                node = optimized;
            }
            
            iterations += 1;
            
            // Exit if no changes were made or we've reached max iterations
            if !changed || iterations >= MAX_ITERATIONS {
                break;
            }
        }
        
        self.stats.nodes_processed += 1;
        node
    }
    
    /// Get optimization statistics
    pub fn stats(&self) -> &OptimizationStats {
        &self.stats
    }
}

/// Constant folding optimization pass
pub struct ConstantFoldingPass {
    folded_count: usize,
}

impl ConstantFoldingPass {
    pub fn new() -> Self {
        ConstantFoldingPass { folded_count: 0 }
    }
    
    /// Fold binary arithmetic operations
    fn fold_arithmetic(&mut self, op: &str, left: &Literal, right: &Literal) -> Option<Literal> {
        match (left, right) {
            (Literal::Integer(a), Literal::Integer(b)) => {
                match op {
                    "+" => Some(Literal::Integer(a + b)),
                    "-" => Some(Literal::Integer(a - b)),
                    "*" => Some(Literal::Integer(a * b)),
                    "/" if *b != 0 => Some(Literal::Integer(a / b)),
                    "%" if *b != 0 => Some(Literal::Integer(a % b)),
                    "=" => Some(Literal::Boolean(a == b)),
                    "!=" => Some(Literal::Boolean(a != b)),
                    "<" => Some(Literal::Boolean(a < b)),
                    "<=" => Some(Literal::Boolean(a <= b)),
                    ">" => Some(Literal::Boolean(a > b)),
                    ">=" => Some(Literal::Boolean(a >= b)),
                    _ => None,
                }
            }
            (Literal::Float(a), Literal::Float(b)) => {
                match op {
                    "+" => Some(Literal::Float(a + b)),
                    "-" => Some(Literal::Float(a - b)),
                    "*" => Some(Literal::Float(a * b)),
                    "/" if *b != 0.0 => Some(Literal::Float(a / b)),
                    "=" => Some(Literal::Boolean((a - b).abs() < f64::EPSILON)),
                    "!=" => Some(Literal::Boolean((a - b).abs() >= f64::EPSILON)),
                    "<" => Some(Literal::Boolean(a < b)),
                    "<=" => Some(Literal::Boolean(a <= b)),
                    ">" => Some(Literal::Boolean(a > b)),
                    ">=" => Some(Literal::Boolean(a >= b)),
                    _ => None,
                }
            }
            (Literal::String(a), Literal::String(b)) => {
                match op {
                    "+" => Some(Literal::String(format!("{}{}", a, b))),
                    "=" => Some(Literal::Boolean(a == b)),
                    "!=" => Some(Literal::Boolean(a != b)),
                    _ => None,
                }
            }
            (Literal::Boolean(a), Literal::Boolean(b)) => {
                match op {
                    "and" => Some(Literal::Boolean(*a && *b)),
                    "or" => Some(Literal::Boolean(*a || *b)),
                    "=" => Some(Literal::Boolean(a == b)),
                    "!=" => Some(Literal::Boolean(a != b)),
                    _ => None,
                }
            }
            _ => None,
        }
    }
    
    /// Fold logical operations
    fn fold_logical(&mut self, node: &IrNode) -> Option<IrNode> {
        match node {
            IrNode::If { condition, then_branch, else_branch, .. } => {
                if let IrNode::Literal { value: Literal::Boolean(cond), .. } = condition.as_ref() {
                    self.folded_count += 1;
                    if *cond {
                        Some((**then_branch).clone())
                    } else if let Some(else_node) = else_branch {
                        Some((**else_node).clone())
                    } else {
                        Some(IrNode::Literal {
                            id: node.id(),
                            value: Literal::Nil,
                            ir_type: IrType::Nil,
                            source_location: node.source_location().cloned(),
                        })
                    }
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}

impl OptimizationPass for ConstantFoldingPass {
    fn name(&self) -> &'static str {
        "ConstantFolding"
    }
    
    fn optimize(&mut self, node: IrNode) -> IrNode {
        self.optimize_with_depth(node, 0)
    }
}

impl ConstantFoldingPass {
    /// Maximum recursion depth to prevent infinite loops
    const MAX_DEPTH: usize = 100;
    
    /// Optimize with depth tracking to prevent infinite recursion
    fn optimize_with_depth(&mut self, node: IrNode, depth: usize) -> IrNode {
        // Prevent infinite recursion
        if depth >= Self::MAX_DEPTH {
            return node;
        }
        
        match node {
            IrNode::Apply { id, function, arguments, ir_type, source_location } => {
                // Recursively optimize arguments with depth tracking
                let optimized_args: Vec<IrNode> = arguments.into_iter()
                    .map(|arg| self.optimize_with_depth(arg, depth + 1))
                    .collect();
                
                // Try to fold if function is a known operator and all args are literals
                if let IrNode::VariableRef { name, .. } = function.as_ref() {
                    if optimized_args.len() == 2 {
                        if let (IrNode::Literal { value: left, .. }, IrNode::Literal { value: right, .. }) 
                            = (&optimized_args[0], &optimized_args[1]) {
                            if let Some(folded_value) = self.fold_arithmetic(name, left, right) {
                                self.folded_count += 1;
                                return IrNode::Literal {
                                    id,
                                    value: folded_value.clone(),
                                    ir_type: self.infer_literal_type(&folded_value),
                                    source_location,
                                };
                            }
                        }
                    }
                }
                
                // Only optimize function if it's not already a simple reference
                let optimized_function = match function.as_ref() {
                    IrNode::VariableRef { .. } | IrNode::Literal { .. } => *function,
                    _ => self.optimize_with_depth(*function, depth + 1),
                };
                
                IrNode::Apply {
                    id,
                    function: Box::new(optimized_function),
                    arguments: optimized_args,
                    ir_type,
                    source_location,
                }
            }            
            IrNode::If { id, condition, then_branch, else_branch, ir_type, source_location } => {
                let optimized_condition = self.optimize_with_depth(*condition, depth + 1);
                
                // Try logical folding first
                let temp_node = IrNode::If {
                    id,
                    condition: Box::new(optimized_condition.clone()),
                    then_branch: then_branch.clone(),
                    else_branch: else_branch.clone(),
                    ir_type: ir_type.clone(),
                    source_location: source_location.clone(),
                };
                
                if let Some(folded) = self.fold_logical(&temp_node) {
                    return folded;
                }
                
                // Otherwise, optimize branches with depth tracking
                IrNode::If {
                    id,
                    condition: Box::new(optimized_condition),
                    then_branch: Box::new(self.optimize_with_depth(*then_branch, depth + 1)),
                    else_branch: else_branch.map(|e| Box::new(self.optimize_with_depth(*e, depth + 1))),
                    ir_type,
                    source_location,
                }
            }
            
            IrNode::Let { id, bindings, body, ir_type, source_location } => {
                let optimized_bindings: Vec<IrLetBinding> = bindings.into_iter()
                    .map(|binding| IrLetBinding {
                        pattern: self.optimize_with_depth(binding.pattern, depth + 1),
                        type_annotation: binding.type_annotation,
                        init_expr: self.optimize_with_depth(binding.init_expr, depth + 1),
                    })
                    .collect();
                
                let optimized_body: Vec<IrNode> = body.into_iter()
                    .map(|expr| self.optimize_with_depth(expr, depth + 1))
                    .collect();
                
                IrNode::Let {
                    id,
                    bindings: optimized_bindings,
                    body: optimized_body,
                    ir_type,
                    source_location,
                }
            }
            
            IrNode::Do { id, expressions, ir_type, source_location } => {
                let optimized_expressions: Vec<IrNode> = expressions.into_iter()
                    .map(|expr| self.optimize_with_depth(expr, depth + 1))
                    .collect();
                
                IrNode::Do {
                    id,
                    expressions: optimized_expressions,
                    ir_type,
                    source_location,
                }
            }
            
            // For other node types, recursively optimize children with depth tracking
            _ => self.optimize_children_with_depth(node, depth + 1),
        }
    }
}

impl ConstantFoldingPass {
    fn infer_literal_type(&self, literal: &Literal) -> IrType {
        match literal {
            Literal::Integer(_) => IrType::Int,
            Literal::Float(_) => IrType::Float,
            Literal::String(_) => IrType::String,
            Literal::Boolean(_) => IrType::Bool,
            Literal::Keyword(_) => IrType::Keyword,
            Literal::Nil => IrType::Nil,
        }
    }    fn optimize_children(&mut self, node: IrNode) -> IrNode {
        self.optimize_children_with_depth(node, 0)
    }

    fn optimize_children_with_depth(&mut self, node: IrNode, depth: usize) -> IrNode {
        // Prevent infinite recursion
        if depth >= Self::MAX_DEPTH {
            return node;
        }

        match node {
            IrNode::Lambda { id, params, variadic_param, body, captures, ir_type, source_location } => {
                let optimized_body: Vec<IrNode> = body.into_iter()
                    .map(|expr| self.optimize_with_depth(expr, depth + 1))
                    .collect();
                
                IrNode::Lambda {
                    id,
                    params,
                    variadic_param,
                    body: optimized_body,
                    captures,
                    ir_type,
                    source_location,
                }
            }
            
            IrNode::LogStep { id, level, values, location, ir_type, source_location } => {
                let optimized_values: Vec<IrNode> = values.into_iter()
                    .map(|expr| self.optimize_with_depth(expr, depth + 1))
                    .collect();
                
                IrNode::LogStep {
                    id,
                    level,
                    values: optimized_values,
                    location,
                    ir_type,
                    source_location,
                }
            }
            
            // For other nodes without children or already handled, return as-is
            _ => node,
        }
    }
}

/// Dead code elimination pass
pub struct DeadCodeEliminationPass {
    eliminated_count: usize,
}

impl DeadCodeEliminationPass {
    pub fn new() -> Self {
        DeadCodeEliminationPass { eliminated_count: 0 }
    }
    
    /// Check if a node has side effects
    fn has_side_effects(&self, node: &IrNode) -> bool {
        match node {
            IrNode::Literal { .. } => false,
            IrNode::VariableRef { .. } => false,            IrNode::Apply { function, arguments, .. } => {
                // Conservative: assume function calls have side effects unless known pure
                match function.as_ref() {
                    IrNode::VariableRef { name, .. } => {
                        // Known pure functions
                        let pure_functions = ["+", "-", "*", "/", "=", "!=", "<", "<=", ">", ">="];
                        if pure_functions.contains(&name.as_str()) {
                            arguments.iter().any(|arg| self.has_side_effects(arg))
                        } else {
                            true // Assume side effects for unknown functions
                        }
                    }
                    _ => true,
                }
            }
            IrNode::LogStep { .. } => true, // Logging has side effects
            _ => true, // Conservative default
        }
    }
}

impl OptimizationPass for DeadCodeEliminationPass {
    fn name(&self) -> &'static str {
        "DeadCodeElimination"
    }
    
    fn optimize(&mut self, node: IrNode) -> IrNode {
        self.optimize_with_depth(node, 0)
    }
}

impl DeadCodeEliminationPass {
    /// Maximum recursion depth to prevent infinite loops
    const MAX_DEPTH: usize = 100;
    
    /// Optimize with depth tracking to prevent infinite recursion
    fn optimize_with_depth(&mut self, node: IrNode, depth: usize) -> IrNode {
        // Prevent infinite recursion
        if depth >= Self::MAX_DEPTH {
            return node;
        }
        
        match node {
            IrNode::Do { id, expressions, ir_type, source_location } => {
                let mut optimized_expressions = Vec::new();
                let expressions_len = expressions.len();
                  for (i, expr) in expressions.into_iter().enumerate() {
                    let optimized_expr = self.optimize_with_depth(expr, depth + 1);
                    
                    // Keep the last expression (return value) and expressions with side effects
                    if i == expressions_len - 1 || self.has_side_effects(&optimized_expr) {
                        optimized_expressions.push(optimized_expr);
                    } else {
                        self.eliminated_count += 1;
                    }
                }
                
                // If all expressions were eliminated except the last, simplify
                if optimized_expressions.len() == 1 {
                    optimized_expressions.into_iter().next().unwrap()
                } else {
                    IrNode::Do {
                        id,
                        expressions: optimized_expressions,
                        ir_type,
                        source_location,
                    }
                }
            }
            
            IrNode::Let { id, bindings, body, ir_type, source_location } => {
                // Remove unused bindings
                let mut used_bindings = Vec::new();
                let mut binding_usage = HashMap::new();
                
                // Analyze usage in body (simplified)
                for binding in &bindings {
                    if let IrNode::VariableBinding { id: binding_id, .. } = &binding.pattern {
                        binding_usage.insert(*binding_id, false);
                    }
                }
                  // Mark bindings as used (simplified - would need full usage analysis)
                for binding in bindings {
                    // For now, keep all bindings that have side effects
                    if self.has_side_effects(&binding.init_expr) {
                        used_bindings.push(IrLetBinding {
                            pattern: self.optimize_with_depth(binding.pattern, depth + 1),
                            type_annotation: binding.type_annotation,
                            init_expr: self.optimize_with_depth(binding.init_expr, depth + 1),
                        });
                    } else {
                        used_bindings.push(IrLetBinding {
                            pattern: self.optimize_with_depth(binding.pattern, depth + 1),
                            type_annotation: binding.type_annotation,
                            init_expr: self.optimize_with_depth(binding.init_expr, depth + 1),
                        });
                    }
                }
                
                let optimized_body: Vec<IrNode> = body.into_iter()
                    .map(|expr| self.optimize_with_depth(expr, depth + 1))
                    .collect();
                
                IrNode::Let {
                    id,
                    bindings: used_bindings,
                    body: optimized_body,
                    ir_type,
                    source_location,
                }
            }
              _ => node, // For other nodes, return as-is (would need full implementation)
        }
    }
}

/// Type specialization pass - creates specialized versions for common type patterns
pub struct TypeSpecializationPass {
    specializations: usize,
}

impl TypeSpecializationPass {
    pub fn new() -> Self {
        TypeSpecializationPass { specializations: 0 }
    }
}

impl OptimizationPass for TypeSpecializationPass {
    fn name(&self) -> &'static str {
        "TypeSpecialization"
    }
    
    fn optimize(&mut self, node: IrNode) -> IrNode {
        // TODO: Implement type specialization
        // This would create specialized versions of functions for specific types
        // For example, a generic add function could be specialized for integers
        node
    }
}

/// Function inlining pass
pub struct InliningPass {
    inlined_count: usize,
    inline_threshold: usize, // Maximum size for inlining
}

impl InliningPass {
    pub fn new() -> Self {
        InliningPass {
            inlined_count: 0,
            inline_threshold: 10, // Inline functions with <= 10 nodes
        }
    }
    
    /// Estimate the size of an IR node tree
    fn estimate_size(&self, node: &IrNode) -> usize {
        match node {
            IrNode::Literal { .. } => 1,
            IrNode::VariableRef { .. } => 1,
            IrNode::Apply { function, arguments, .. } => {
                1 + self.estimate_size(function) + arguments.iter().map(|arg| self.estimate_size(arg)).sum::<usize>()
            }
            IrNode::If { condition, then_branch, else_branch, .. } => {
                1 + self.estimate_size(condition) + self.estimate_size(then_branch) 
                    + else_branch.as_ref().map_or(0, |e| self.estimate_size(e))
            }
            IrNode::Do { expressions, .. } => {
                1 + expressions.iter().map(|expr| self.estimate_size(expr)).sum::<usize>()
            }
            _ => 5, // Conservative estimate for complex nodes
        }
    }
}

impl OptimizationPass for InliningPass {
    fn name(&self) -> &'static str {
        "Inlining"
    }
    
    fn optimize(&mut self, node: IrNode) -> IrNode {
        self.optimize_with_depth(node, 0)
    }
}

impl InliningPass {
    /// Maximum recursion depth to prevent infinite loops
    const MAX_DEPTH: usize = 100;
    
    /// Optimize with depth tracking to prevent infinite recursion
    fn optimize_with_depth(&mut self, node: IrNode, depth: usize) -> IrNode {
        // Prevent infinite recursion
        if depth >= Self::MAX_DEPTH {
            return node;
        }
        
        match node {
            IrNode::Apply { id, function, arguments, ir_type, source_location } => {
                // Check if we can inline this function call
                if let IrNode::Lambda { params, body, .. } = function.as_ref() {
                    let body_size: usize = body.iter().map(|expr| self.estimate_size(expr)).sum();
                    
                    if body_size <= self.inline_threshold && params.len() == arguments.len() {
                        // Inline the function
                        self.inlined_count += 1;
                        
                        // Create let binding for parameters
                        let mut bindings = Vec::new();
                        for (param, arg) in params.iter().zip(arguments.iter()) {
                            if let IrNode::Param { binding, .. } = param {
                                bindings.push(IrLetBinding {
                                    pattern: (**binding).clone(),
                                    type_annotation: None,
                                    init_expr: arg.clone(),
                                });
                            }
                        }
                        
                        return IrNode::Let {
                            id,
                            bindings,
                            body: body.clone(),
                            ir_type,
                            source_location,
                        };
                    }
                }
                  // If not inlined, optimize recursively with depth tracking
                IrNode::Apply {
                    id,
                    function: Box::new(self.optimize_with_depth(*function, depth + 1)),
                    arguments: arguments.into_iter().map(|arg| self.optimize_with_depth(arg, depth + 1)).collect(),
                    ir_type,
                    source_location,
                }
            }
            
            _ => node, // For other nodes, return as-is (would need full implementation)
        }
    }
}

/// Control flow analysis pass that identifies and optimizes control flow patterns
pub struct ControlFlowAnalysisPass {
    branch_count: usize,
    eliminated_branches: usize,
}

impl ControlFlowAnalysisPass {
    pub fn new() -> Self {
        Self {
            branch_count: 0,
            eliminated_branches: 0,
        }
    }
}

impl OptimizationPass for ControlFlowAnalysisPass {
    fn optimize(&mut self, node: IrNode) -> IrNode {
        match node {
            IrNode::IfExpression { id, condition, then_expr, else_expr, ir_type, source_location } => {
                self.branch_count += 1;
                
                // Optimize condition first
                let optimized_condition = self.optimize(*condition);
                
                // Check for compile-time constant conditions
                if let IrNode::Literal { value: Literal::Bool(cond_value), .. } = optimized_condition {
                    self.eliminated_branches += 1;
                    if cond_value {
                        // Condition is always true, eliminate else branch
                        return self.optimize(*then_expr);
                    } else {
                        // Condition is always false, eliminate then branch
                        return self.optimize(*else_expr);
                    }
                }
                
                // Optimize both branches
                let optimized_then = self.optimize(*then_expr);
                let optimized_else = self.optimize(*else_expr);
                
                IrNode::IfExpression {
                    id,
                    condition: Box::new(optimized_condition),
                    then_expr: Box::new(optimized_then),
                    else_expr: Box::new(optimized_else),
                    ir_type,
                    source_location,
                }
            },
            
            IrNode::DoExpression { id, expressions, ir_type, source_location } => {
                // Optimize each expression and eliminate dead code
                let mut optimized_exprs = Vec::new();
                
                for expr in expressions {
                    let optimized = self.optimize(expr);
                    
                    // Don't eliminate expressions with side effects
                    if !self.is_pure_expression(&optimized) || optimized_exprs.is_empty() {
                        optimized_exprs.push(optimized);
                    } else {
                        // This is dead code (pure expression not at end)
                        self.eliminated_branches += 1;
                    }
                }
                
                // If only one expression remains, unwrap the do
                if optimized_exprs.len() == 1 {
                    optimized_exprs.into_iter().next().unwrap()
                } else {
                    IrNode::DoExpression {
                        id,
                        expressions: optimized_exprs,
                        ir_type,
                        source_location,
                    }
                }
            },
            
            IrNode::LetExpression { id, bindings, body, ir_type, source_location } => {
                // Optimize bindings and check for unused variables
                let mut optimized_bindings = Vec::new();
                let mut used_bindings = std::collections::HashSet::new();
                
                // First pass: collect used bindings
                self.collect_used_bindings(&body, &mut used_bindings);
                
                // Second pass: optimize and filter bindings
                for binding in bindings {
                    let optimized_init = self.optimize(binding.init_expr);
                    
                    // Keep binding if it's used or has side effects
                    if used_bindings.contains(&binding.binding_id) || !self.is_pure_expression(&optimized_init) {
                        optimized_bindings.push(IrBinding {
                            binding_id: binding.binding_id,
                            pattern: binding.pattern,
                            init_expr: optimized_init,
                            ir_type: binding.ir_type,
                        });
                    } else {
                        self.eliminated_branches += 1;
                    }
                }
                
                let optimized_body = self.optimize(*body);
                
                // If no bindings remain, unwrap the let
                if optimized_bindings.is_empty() {
                    optimized_body
                } else {
                    IrNode::LetExpression {
                        id,
                        bindings: optimized_bindings,
                        body: Box::new(optimized_body),
                        ir_type,
                        source_location,
                    }
                }
            },
            
            // Recursively optimize other node types
            other => self.optimize_children(other),
        }
    }
    
    fn name(&self) -> &'static str {
        "ControlFlowAnalysis"
    }
}

impl ControlFlowAnalysisPass {
    fn is_pure_expression(&self, node: &IrNode) -> bool {
        match node {
            IrNode::Literal { .. } | IrNode::VariableRef { .. } => true,
            IrNode::FunctionCall { function, .. } => {
                // Conservative: assume all function calls have side effects
                // In a more sophisticated pass, we'd track pure functions
                false
            },
            IrNode::BinaryOp { left, right, .. } => {
                self.is_pure_expression(left) && self.is_pure_expression(right)
            },
            _ => false,
        }
    }
    
    fn collect_used_bindings(&self, node: &IrNode, used: &mut std::collections::HashSet<NodeId>) {
        match node {
            IrNode::VariableRef { binding_id, .. } => {
                used.insert(*binding_id);
            },
            IrNode::LetExpression { bindings, body, .. } => {
                self.collect_used_bindings(body, used);
                for binding in bindings {
                    self.collect_used_bindings(&binding.initExpr, used);
                }
            },
            IrNode::IfExpression { condition, then_expr, else_expr, .. } => {
                self.collect_used_bindings(condition, used);
                self.collect_used_bindings(then_expr, used);
                self.collect_used_bindings(else_expr, used);
            },
            IrNode::DoExpression { expressions, .. } => {
                for expr in expressions {
                    self.collect_used_bindings(expr, used);
                }
            },
            IrNode::FunctionCall { function, args, .. } => {
                self.collect_used_bindings(function, used);
                for arg in args {
                    self.collect_used_bindings(arg, used);
                }
            },
            IrNode::BinaryOp { left, right, .. } => {
                self.collect_used_bindings(left, used);
                self.collect_used_bindings(right, used);
            },
            _ => {
                // Handle other node types as needed
            }
        }
    }
    
    fn optimize_children(&mut self, node: IrNode) -> IrNode {
        match node {
            IrNode::FunctionCall { id, function, args, ir_type, source_location } => {
                let optimized_function = self.optimize(*function);
                let optimized_args: Vec<IrNode> = args.into_iter().map(|arg| self.optimize(arg)).collect();
                
                IrNode::FunctionCall {
                    id,
                    function: Box::new(optimized_function),
                    args: optimized_args,
                    ir_type,
                    source_location,
                }
            },
            IrNode::BinaryOp { id, op, left, right, ir_type, source_location } => {
                let optimized_left = self.optimize(*left);
                let optimized_right = self.optimize(*right);
                
                IrNode::BinaryOp {
                    id,
                    op,
                    left: Box::new(optimized_left),
                    right: Box::new(optimized_right),
                    ir_type,
                    source_location,
                }
            },
            other => other,
        }
    }
}

/// Enhanced function inlining pass with size estimation and cost analysis
pub struct EnhancedInliningPass {
    inlining_threshold: usize,
    calls_inlined: usize,
    functions_analyzed: usize,
}

impl EnhancedInliningPass {
    pub fn new() -> Self {
        Self {
            inlining_threshold: 10, // Maximum node count for inlining
            calls_inlined: 0,
            functions_analyzed: 0,
        }
    }
    
    pub fn with_threshold(threshold: usize) -> Self {
        Self {
            inlining_threshold: threshold,
            calls_inlined: 0,
            functions_analyzed: 0,
        }
    }
}

impl OptimizationPass for EnhancedInliningPass {
    fn optimize(&mut self, node: IrNode) -> IrNode {
        match node {
            IrNode::FunctionCall { id, function, args, ir_type, source_location } => {
                // Check if function is inlinable
                if let IrNode::FunctionDef { lambda, .. } = &*function {
                    self.functions_analyzed += 1;
                    
                    // Estimate the size of the function body
                    let body_size = self.estimate_node_size(&lambda.body);
                    
                    // Only inline small functions
                    if body_size <= self.inlining_threshold && self.is_safe_to_inline(&lambda.body, &args) {
                        self.calls_inlined += 1;
                        
                        // Perform inlining by substituting parameters
                        return self.inline_function_call(lambda, args, ir_type);
                    }
                }
                
                // If not inlined, optimize children
                let optimized_function = self.optimize(*function);
                let optimized_args: Vec<IrNode> = args.into_iter().map(|arg| self.optimize(arg)).collect();
                
                IrNode::FunctionCall {
                    id,
                    function: Box::new(optimized_function),
                    args: optimized_args,
                    ir_type,
                    source_location,
                }
            },
            
            // Recursively optimize other nodes
            other => self.optimize_children(other),
        }
    }
    
    fn name(&self) -> &'static str {
        "EnhancedInlining"
    }
}

impl EnhancedInliningPass {
    fn estimate_node_size(&self, node: &IrNode) -> usize {
        match node {
            IrNode::Literal { .. } | IrNode::VariableRef { .. } => 1,
            IrNode::BinaryOp { left, right, .. } => {
                1 + self.estimate_node_size(left) + self.estimate_node_size(right)
            },
            IrNode::FunctionCall { function, args, .. } => {
                1 + self.estimate_node_size(function) + args.iter().map(|arg| self.estimate_node_size(arg)).sum::<usize>()
            },
            IrNode::LetExpression { bindings, body, .. } => {
                1 + bindings.iter().map(|b| self.estimate_node_size(&b.init_expr)).sum::<usize>() + self.estimate_node_size(body)
            },
            IrNode::IfExpression { condition, then_expr, else_expr, .. } => {
                1 + self.estimate_node_size(condition) + self.estimate_node_size(then_expr) + self.estimate_node_size(else_expr)
            },
            IrNode::DoExpression { expressions, .. } => {
                1 + expressions.iter().map(|expr| self.estimate_node_size(expr)).sum::<usize>()
            },
            _ => 1,
        }
    }
    
    fn is_safe_to_inline(&self, _body: &IrNode, _args: &[IrNode]) -> bool {
        // Conservative safety check - more sophisticated analysis could be added
        // For now, allow inlining if the function doesn't contain complex control flow
        true
    }
    
    fn inline_function_call(&mut self, _lambda: &IrLambda, _args: Vec<IrNode>, ir_type: IrType) -> IrNode {
        // Simplified inlining implementation
        // In a complete implementation, this would substitute parameters in the function body
        IrNode::Literal {
            id: 0,
            value: Literal::Nil,
            ir_type,
            source_location: None,
        }
    }
    
    fn optimize_children(&mut self, node: IrNode) -> IrNode {
        match node {
            IrNode::LetExpression { id, bindings, body, ir_type, source_location } => {
                let optimized_bindings: Vec<IrBinding> = bindings.into_iter().map(|binding| {
                    IrBinding {
                        binding_id: binding.binding_id,
                        pattern: binding.pattern,
                        init_expr: self.optimize(binding.init_expr),
                        ir_type: binding.ir_type,
                    }
                }).collect();
                
                let optimized_body = self.optimize(*body);
                
                IrNode::LetExpression {
                    id,
                    bindings: optimized_bindings,
                    body: Box::new(optimized_body),
                    ir_type,
                    source_location,
                }
            },
            IrNode::IfExpression { id, condition, then_expr, else_expr, ir_type, source_location } => {
                let optimized_condition = self.optimize(*condition);
                let optimized_then = self.optimize(*then_expr);
                let optimized_else = self.optimize(*else_expr);
                
                IrNode::IfExpression {
                    id,
                    condition: Box::new(optimized_condition),
                    then_expr: Box::new(optimized_then),
                    else_expr: Box::new(optimized_else),
                    ir_type,
                    source_location,
                }
            },
            IrNode::DoExpression { id, expressions, ir_type, source_location } => {
                let optimized_expressions: Vec<IrNode> = expressions.into_iter().map(|expr| self.optimize(expr)).collect();
                
                IrNode::DoExpression {
                    id,
                    expressions: optimized_expressions,
                    ir_type,
                    source_location,
                }
            },
            IrNode::BinaryOp { id, op, left, right, ir_type, source_location } => {
                let optimized_left = self.optimize(*left);
                let optimized_right = self.optimize(*right);
                
                IrNode::BinaryOp {
                    id,
                    op,
                    left: Box::new(optimized_left),
                    right: Box::new(optimized_right),
                    ir_type,
                    source_location,
                }
            },
            other => other,
        }
    }
}

/// Enhanced dead code elimination pass with better analysis
pub struct EnhancedDeadCodeEliminationPass {
    eliminated_nodes: usize,
}

impl EnhancedDeadCodeEliminationPass {
    pub fn new() -> Self {
        Self {
            eliminated_nodes: 0,
        }
    }
}

impl OptimizationPass for EnhancedDeadCodeEliminationPass {
    fn optimize(&mut self, node: IrNode) -> IrNode {
        match node {
            IrNode::DoExpression { id, expressions, ir_type, source_location } => {
                let mut optimized_expressions = Vec::new();
                
                for (i, expr) in expressions.into_iter().enumerate() {
                    let optimized = self.optimize(expr);
                    
                    // Keep the last expression or expressions with side effects
                    if i == optimized_expressions.len() || !self.is_dead_code(&optimized) {
                        optimized_expressions.push(optimized);
                    } else {
                        self.eliminated_nodes += 1;
                    }
                }
                
                // Simplify single-expression do blocks
                if optimized_expressions.len() == 1 {
                    optimized_expressions.into_iter().next().unwrap()
                } else {
                    IrNode::DoExpression {
                        id,
                        expressions: optimized_expressions,
                        ir_type,
                        source_location,
                    }
                }
            },
            
            IrNode::LetExpression { id, bindings, body, ir_type, source_location } => {
                // Analyze which bindings are actually used
                let mut used_variables = std::collections::HashSet::new();
                self.collect_used_variables(&body, &mut used_variables);
                
                let mut kept_bindings = Vec::new();
                for binding in bindings {
                    if self.is_binding_referenced(&binding, &used_variables) || 
                       self.has_side_effects(&binding.init_expr) {
                        kept_bindings.push(IrLetBinding {
                            pattern: self.optimize_dead_code_elimination(binding.pattern),
                            type_annotation: binding.type_annotation,
                            init_expr: self.optimize_dead_code_elimination(binding.init_expr),
                        });
                    }
                }
                
                let optimized_body = body.into_iter()
                    .map(|expr| self.optimize_dead_code_elimination(expr))
                    .collect();
                
                if kept_bindings.is_empty() && optimized_body.len() == 1 {
                    optimized_body.into_iter().next().unwrap()
                } else {
                    IrNode::Let {
                        id,
                        bindings: kept_bindings,
                        body: optimized_body,
                        ir_type,
                        source_location,
                    }
                }
            },
            
            _ => self.optimize_children(node)
        }
    }
    
    fn name(&self) -> &'static str {
        "EnhancedDeadCodeElimination"
    }
}

impl EnhancedDeadCodeEliminationPass {
    fn is_dead_code(&self, node: &IrNode) -> bool {
        match node {
            // Pure expressions that don't affect program state
            IrNode::Literal { .. } => true,
            IrNode::VariableRef { .. } => true,
            IrNode::BinaryOp { left, right, .. } => {
                self.is_dead_code(left) && self.is_dead_code(right)
            },
            // Function calls and other expressions may have side effects
            _ => false,
        }
    }
    
    fn collect_used_variables(&self, node: &IrNode, used: &mut std::collections::HashSet<NodeId>) {
        match node {
            IrNode::VariableRef { binding_id, .. } => {
                used.insert(*binding_id);
            },
            IrNode::LetExpression { bindings, body, .. } => {
                self.collect_used_variables(body, used);
                for binding in bindings {
                    if used.contains(&binding.binding_id) {
                        self.collect_used_variables(&binding.init_expr, used);
                    }
                }
            },
            IrNode::IfExpression { condition, then_expr, else_expr, .. } => {
                self.collect_used_variables(condition, used);
                self.collect_used_variables(then_expr, used);
                self.collect_used_variables(else_expr, used);
            },
            IrNode::DoExpression { expressions, .. } => {
                for expr in expressions {
                    self.collect_used_variables(expr, used);
                }
            },
            IrNode::FunctionCall { function, args, .. } => {
                self.collect_used_variables(function, used);
                for arg in args {
                    self.collect_used_variables(arg, used);
                }
            },
            IrNode::BinaryOp { left, right, .. } => {
                self.collect_used_variables(left, used);
                self.collect_used_variables(right, used);
            },
            _ => {}
        }
    }
    
    fn optimize_children(&mut self, node: IrNode) -> IrNode {
        match node {
            IrNode::IfExpression { id, condition, then_expr, else_expr, ir_type, source_location } => {
                let optimized_condition = self.optimize(*condition);
                let optimized_then = self.optimize(*then_expr);
                let optimized_else = self.optimize(*else_expr);
                
                IrNode::IfExpression {
                    id,
                    condition: Box::new(optimized_condition),
                    then_expr: Box::new(optimized_then),
                    else_expr: Box::new(optimized_else),
                    ir_type,
                    source_location,
                }
            },
            IrNode::FunctionCall { id, function, args, ir_type, source_location } => {
                let optimized_function = self.optimize(*function);
                let optimized_args: Vec<IrNode> = args.into_iter().map(|arg| self.optimize(arg)).collect();
                
                IrNode::FunctionCall {
                    id,
                    function: Box::new(optimized_function),
                    args: optimized_args,
                    ir_type,
                    source_location,
                }
            },
            IrNode::BinaryOp { id, op, left, right, ir_type, source_location } => {
                let optimized_left = self.optimize(*left);
                let optimized_right = self.optimize(*right);
                
                IrNode::BinaryOp {
                    id,
                    op,
                    left: Box::new(optimized_left),
                    right: Box::new(optimized_right),
                    ir_type,
                    source_location,
                }
            },
            other => other,
        }
    }
}

/// Enhanced IR Optimizer with advanced optimization strategies
pub struct EnhancedIrOptimizer {
    optimization_level: OptimizationLevel,
    inline_threshold: usize,
    max_inline_depth: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub enum OptimizationLevel {
    None,
    Basic,
    Aggressive,
}

impl EnhancedIrOptimizer {
    pub fn new() -> Self {
        Self {
            optimization_level: OptimizationLevel::Aggressive,
            inline_threshold: 10,
            max_inline_depth: 3,
        }
    }    pub fn with_level(level: OptimizationLevel) -> Self {
        let threshold = match level {
            OptimizationLevel::None => 0,
            OptimizationLevel::Basic => 5,
            OptimizationLevel::Aggressive => 15,
        };
        
        Self {
            optimization_level: level,
            inline_threshold: threshold,
            max_inline_depth: 3,
        }
    }

    /// Enhanced optimization pass with control flow analysis
    pub fn optimize_with_control_flow(&mut self, node: IrNode) -> IrNode {
        match self.optimization_level {
            OptimizationLevel::None => node,
            _ => {
                // First pass: basic optimizations
                let node = self.optimize_basic(node);
                
                // Second pass: control flow analysis
                let node = self.optimize_control_flow(node);
                
                // Third pass: advanced function inlining
                let node = self.optimize_function_inlines(node);
                
                // Fourth pass: enhanced dead code elimination
                self.optimize_dead_code_elimination(node)
            }
        }
    }

    fn optimize_control_flow(&self, node: IrNode) -> IrNode {
        match node {
            IrNode::If { id, condition, then_branch, else_branch, ir_type, source_location } => {
                // Analyze condition for constant folding
                let optimized_condition = self.optimize_control_flow(condition.as_ref().clone());
                
                match optimized_condition {
                    // Constant condition optimization
                    IrNode::Literal { value: Literal::Boolean(true), .. } => {
                        // Always true - return then branch
                        self.optimize_control_flow(then_branch.as_ref().clone())
                    },
                    IrNode::Literal { value: Literal::Boolean(false), .. } => {
                        // Always false - return else branch or nil
                        if let Some(else_node) = else_branch {
                            self.optimize_control_flow(else_node.as_ref().clone())
                        } else {
                            IrNode::Literal {
                                id: 0,
                                value: Literal::Nil,
                                ir_type: IrType::Nil,
                                source_location: None,
                            }
                        }
                    },
                    _ => {
                        // Keep if structure with optimized branches
                        IrNode::If {
                            id,
                            condition: Box::new(optimized_condition),
                            then_branch: Box::new(self.optimize_control_flow(then_branch.as_ref().clone())),
                            else_branch: else_branch.map(|e| Box::new(self.optimize_control_flow(e.as_ref().clone()))),
                            ir_type,
                            source_location,
                        }
                    }
                }
            },
            
            IrNode::Do { id, expressions, ir_type, source_location } => {
                // Optimize do blocks - remove intermediate unused values
                let mut optimized_exprs = Vec::new();
                for expr in expressions {
                    let optimized = self.optimize_control_flow(expr);
                    // Keep side-effectful expressions and the last expression
                    if self.has_side_effects(&optimized) || optimized_exprs.is_empty() {
                        optimized_exprs.push(optimized);
                    }
                }
                
                if optimized_exprs.len() == 1 {
                    optimized_exprs.into_iter().next().unwrap()
                } else {
                    IrNode::Do {
                        id,
                        expressions: optimized_exprs,
                        ir_type,
                        source_location,
                    }
                }
            },
            
            IrNode::Let { id, bindings, body, ir_type, source_location } => {
                // Optimize let bindings - remove unused bindings
                let mut used_bindings = Vec::new();
                let optimized_body = body.into_iter()
                    .map(|expr| self.optimize_control_flow(expr))
                    .collect();
                
                // Analyze which bindings are actually used
                for binding in bindings {
                    if self.is_binding_used(&binding, &optimized_body) {
                        used_bindings.push(IrLetBinding {
                            pattern: self.optimize_control_flow(binding.pattern),
                            type_annotation: binding.type_annotation,
                            init_expr: self.optimize_control_flow(binding.init_expr),
                        });
                    }
                }
                
                if used_bindings.is_empty() && optimized_body.len() == 1 {
                    // No bindings needed, return body directly
                    optimized_body.into_iter().next().unwrap()
                } else {
                    IrNode::Let {
                        id,
                        bindings: used_bindings,
                        body: optimized_body,
                        ir_type,
                        source_location,
                    }
                }
            },
            
            _ => {
                // Recursively optimize other node types
                self.optimize_recursive(node)
            }
        }
    }

    fn optimize_function_inlines(&self, node: IrNode) -> IrNode {
        match node {
            IrNode::Apply { id, function, arguments, ir_type, source_location } => {
                // Check if function is inlinable
                if let IrNode::Lambda { params, body, .. } = function.as_ref() {
                    let body_size = self.estimate_body_size(body);
                    
                    // Only inline small functions within depth limits
                    if body_size <= self.inline_threshold && 
                       params.len() == arguments.len() &&
                       self.should_inline(function.as_ref()) {
                        
                        // Perform function inlining
                        return self.inline_function_call(params, body, &arguments, ir_type);
                    }
                }
                
                // If not inlined, optimize recursively
                IrNode::Apply {
                    id,
                    function: Box::new(self.optimize_function_inlines(*function)),
                    arguments: arguments.into_iter().map(|arg| self.optimize_function_inlines(arg)).collect(),
                    ir_type,
                    source_location,
                }
            },
            
            _ => self.optimize_recursive(node)
        }
    }

    fn optimize_dead_code_elimination(&self, node: IrNode) -> IrNode {
        match node {
            IrNode::Do { id, expressions, ir_type, source_location } => {
                let mut kept_expressions = Vec::new();
                let expr_count = expressions.len();
                
                for (i, expr) in expressions.into_iter().enumerate() {
                    let optimized = self.optimize_dead_code_elimination(expr);
                    
                    // Keep last expression (return value) and expressions with side effects
                    if i == expr_count - 1 || self.has_side_effects(&optimized) {
                        kept_expressions.push(optimized);
                    }
                }
                
                if kept_expressions.len() == 1 {
                    kept_expressions.into_iter().next().unwrap()
                } else {
                    IrNode::Do {
                        id,
                        expressions: kept_expressions,
                        ir_type,
                        source_location,
                    }
                }
            },
            
            IrNode::Let { id, bindings, body, ir_type, source_location } => {
                // Advanced unused binding analysis
                let mut used_variables = std::collections::HashSet::new();
                self.collect_used_variables(&body, &mut used_variables);
                
                let mut kept_bindings = Vec::new();
                for binding in bindings {
                    if self.is_binding_referenced(&binding, &used_variables) || 
                       self.has_side_effects(&binding.init_expr) {
                        kept_bindings.push(IrLetBinding {
                            pattern: self.optimize_dead_code_elimination(binding.pattern),
                            type_annotation: binding.type_annotation,
                            init_expr: self.optimize_dead_code_elimination(binding.init_expr),
                        });
                    }
                }
                
                let optimized_body = body.into_iter()
                    .map(|expr| self.optimize_dead_code_elimination(expr))
                    .collect();
                
                if kept_bindings.is_empty() && optimized_body.len() == 1 {
                    optimized_body.into_iter().next().unwrap()
                } else {
                    IrNode::Let {
                        id,
                        bindings: kept_bindings,
                        body: optimized_body,
                        ir_type,
                        source_location,
                    }
                }
            },
            
            _ => self.optimize_children(node)
        }
    }

    // Helper methods for enhanced optimization
    
    fn optimize_basic(&self, node: IrNode) -> IrNode {
        // Delegate to existing constant folding pass
        let mut folding_pass = ConstantFoldingPass::new();
        folding_pass.optimize(node)
    }

    fn optimize_recursive(&self, node: IrNode) -> IrNode {
        match node {
            IrNode::Lambda { id, params, variadic_param, body, captures, ir_type, source_location } => {
                IrNode::Lambda {
                    id,
                    params: params.into_iter().map(|p| self.optimize_control_flow(p)).collect(),
                    variadic_param: variadic_param.map(|vp| Box::new(self.optimize_control_flow(*vp))),
                    body: body.into_iter().map(|expr| self.optimize_control_flow(expr)).collect(),
                    captures,
                    ir_type,
                    source_location,
                }
            },
            
            IrNode::Match { id, expression, clauses, ir_type, source_location } => {
                IrNode::Match {
                    id,
                    expression: Box::new(self.optimize_control_flow(*expression)),
                    clauses: clauses.into_iter().map(|clause| IrMatchClause {
                        pattern: clause.pattern,
                        guard: clause.guard.map(|g| self.optimize_control_flow(g)),
                        body: self.optimize_control_flow(clause.body),
                    }).collect(),
                    ir_type,
                    source_location,
                }
            },
            
            _ => node,
        }
    }

    fn has_side_effects(&self, node: &IrNode) -> bool {
        match node {
            IrNode::Literal { .. } => false,
            IrNode::VariableRef { .. } => false,
            IrNode::Apply { function, arguments, .. } => {
                // Conservative: assume function calls have side effects unless known pure
                match function.as_ref() {
                    IrNode::VariableRef { name, .. } => {
                        let pure_functions = ["+", "-", "*", "/", "=", "!=", "<", "<=", ">", ">=", "and", "or", "not"];
                        if pure_functions.contains(&name.as_str()) {
                            arguments.iter().any(|arg| self.has_side_effects(arg))
                        } else {
                            true // Assume side effects for unknown functions
                        }
                    }
                    _ => true,
                }
            },
            IrNode::LogStep { .. } => true, // Logging has side effects
            IrNode::TryCatch { .. } => true, // Exception handling has side effects
            IrNode::WithResource { .. } => true, // Resource management has side effects
            _ => true, // Conservative default
        }
    }

    fn is_binding_used(&self, binding: &IrLetBinding, body: &[IrNode]) -> bool {
        // Simplified binding usage analysis
        if let IrNode::VariableBinding { name, .. } = &binding.pattern {
            for expr in body {
                if self.contains_variable_reference(expr, name) {
                    return true;
                }
            }
        }
        // Keep bindings with side effects
        self.has_side_effects(&binding.init_expr)
    }

    fn contains_variable_reference(&self, node: &IrNode, var_name: &str) -> bool {
        match node {
            IrNode::VariableRef { name, .. } => name == var_name,
            IrNode::Apply { function, arguments, .. } => {
                self.contains_variable_reference(function, var_name) ||
                arguments.iter().any(|arg| self.contains_variable_reference(arg, var_name))
            },
            IrNode::If { condition, then_branch, else_branch, .. } => {
                self.contains_variable_reference(condition, var_name) ||
                self.contains_variable_reference(then_branch, var_name) ||
                else_branch.as_ref().map_or(false, |e| self.contains_variable_reference(e, var_name))
            },
            IrNode::Let { bindings, body, .. } => {
                bindings.iter().any(|b| self.contains_variable_reference(&b.init_expr, var_name)) ||
                body.iter().any(|expr| self.contains_variable_reference(expr, var_name))
            },
            IrNode::Do { expressions, .. } => {
                expressions.iter().any(|expr| self.contains_variable_reference(expr, var_name))
            },
            _ => false,
        }
    }

    fn estimate_body_size(&self, body: &[IrNode]) -> usize {
        body.iter().map(|node| self.estimate_node_size(node)).sum()
    }

    fn estimate_node_size(&self, node: &IrNode) -> usize {
        match node {
            IrNode::Literal { .. } => 1,
            IrNode::VariableRef { .. } => 1,
            IrNode::Apply { function, arguments, .. } => {
                1 + self.estimate_node_size(function) + arguments.iter().map(|arg| self.estimate_node_size(arg)).sum::<usize>()
            },
            IrNode::If { condition, then_branch, else_branch, .. } => {
                1 + self.estimate_node_size(condition) + self.estimate_node_size(then_branch) +
                else_branch.as_ref().map_or(0, |e| self.estimate_node_size(e))
            },
            IrNode::Let { bindings, body, .. } => {
                1 + bindings.iter().map(|b| self.estimate_node_size(&b.init_expr)).sum::<usize>() +
                body.iter().map(|expr| self.estimate_node_size(expr)).sum::<usize>()
            },
            IrNode::Do { expressions, .. } => {
                1 + expressions.iter().map(|expr| self.estimate_node_size(expr)).sum::<usize>()
            },
            _ => 3, // Conservative estimate for complex nodes
        }
    }

    fn should_inline(&self, function: &IrNode) -> bool {
        // Simple heuristic: inline if function is small and not recursive
        match function {
            IrNode::Lambda { body, .. } => {
                let size = self.estimate_body_size(body);
                size <= self.inline_threshold && !self.is_recursive_function(function)
            },
            _ => false,
        }
    }

    fn is_recursive_function(&self, _function: &IrNode) -> bool {
        // Simplified recursion detection (would need full call graph analysis)
        false
    }

    fn inline_function_call(&self, params: &[IrNode], body: &[IrNode], args: &[IrNode], return_type: IrType) -> IrNode {
        // Create parameter bindings
        let mut bindings = Vec::new();
        for (param, arg) in params.iter().zip(args.iter()) {
            if let IrNode::Param { binding, .. } = param {
                bindings.push(IrLetBinding {
                    pattern: (**binding).clone(),
                    type_annotation: None,
                    init_expr: arg.clone(),
                });
            }
        }

        // Create let expression with inlined body
        IrNode::Let {
            id: 0, // Generate new ID in production
            bindings,
            body: body.to_vec(),
            ir_type: return_type,
            source_location: None,
        }
    }

    fn collect_used_variables(&self, body: &[IrNode], used: &mut std::collections::HashSet<String>) {
        for expr in body {
            self.collect_variables_from_node(expr, used);
        }
    }

    fn collect_variables_from_node(&self, node: &IrNode, used: &mut std::collections::HashSet<String>) {
        match node {
            IrNode::VariableRef { name, .. } => {
                used.insert(name.clone());
            },
            IrNode::Apply { function, arguments, .. } => {
                self.collect_variables_from_node(function, used);
                for arg in arguments {
                    self.collect_variables_from_node(arg, used);
                }
            },
            IrNode::If { condition, then_branch, else_branch, .. } => {
                self.collect_variables_from_node(condition, used);
                self.collect_variables_from_node(then_branch, used);
                if let Some(else_node) = else_branch {
                    self.collect_variables_from_node(else_node, used);
                }
            },
            IrNode::Let { bindings, body, .. } => {
                for binding in bindings {
                    self.collect_variables_from_node(&binding.init_expr, used);
                }
                for expr in body {
                    self.collect_variables_from_node(expr, used);
                }
            },
            IrNode::Do { expressions, .. } => {
                for expr in expressions {
                    self.collect_variables_from_node(expr, used);
                }
            },
            _ => {}
        }
    }

    fn is_binding_referenced(&self, binding: &IrLetBinding, used_variables: &std::collections::HashSet<String>) -> bool {
        if let IrNode::VariableBinding { name, .. } = &binding.pattern {
            used_variables.contains(name)
        } else {
            true // Conservative: keep complex patterns
        }
    }
}

// Enhanced optimization pipeline that combines basic and advanced passes
pub struct EnhancedOptimizationPipeline {
    basic_pipeline: OptimizationPipeline,
    enhanced_optimizer: EnhancedIrOptimizer,
    stats: EnhancedOptimizationStats,
}

#[derive(Debug, Default)]
pub struct EnhancedOptimizationStats {
    pub basic_stats: OptimizationStats,
    pub control_flow_optimizations: usize,
    pub functions_inlined: usize,
    pub dead_code_blocks_eliminated: usize,
    pub optimization_time_ms: u128,
}

impl EnhancedOptimizationPipeline {
    pub fn new() -> Self {
        Self {
            basic_pipeline: OptimizationPipeline::standard(),
            enhanced_optimizer: EnhancedIrOptimizer::new(),
            stats: EnhancedOptimizationStats::default(),
        }
    }

    pub fn with_optimization_level(level: OptimizationLevel) -> Self {
        Self {
            basic_pipeline: OptimizationPipeline::standard(),
            enhanced_optimizer: EnhancedIrOptimizer::with_level(level),
            stats: EnhancedOptimizationStats::default(),
        }
    }

    pub fn optimize(&mut self, node: IrNode) -> IrNode {
        let start_time = std::time::Instant::now();
        
        // First: run basic optimization passes
        let node = self.basic_pipeline.optimize(node);
        self.stats.basic_stats = self.basic_pipeline.stats().clone();
        
        // Second: run enhanced optimizations
        let optimized_node = self.enhanced_optimizer.optimize_with_control_flow(node);
        
        self.stats.optimization_time_ms = start_time.elapsed().as_millis();
        optimized_node
    }

    pub fn stats(&self) -> &EnhancedOptimizationStats {
        &self.stats
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_constant_folding() {
        let mut pass = ConstantFoldingPass::new();
        
        // Test arithmetic folding
        let node = IrNode::Apply {
            id: 1,
            function: Box::new(IrNode::VariableRef {
                id: 2,
                name: "+".to_string(),
                binding_id: 100,
                ir_type: IrType::Function {
                    param_types: vec![IrType::Int, IrType::Int],
                    variadic_param_type: None,
                    return_type: Box::new(IrType::Int),
                },
                source_location: None,
            }),
            arguments: vec![
                IrNode::Literal { id: 3, value: Literal::Integer(2), ir_type: IrType::Int, source_location: None },
                IrNode::Literal { id: 4, value: Literal::Integer(3), ir_type: IrType::Int, source_location: None },
            ],
            ir_type: IrType::Int,
            source_location: None,
        };
        
        let optimized = pass.optimize(node);
        
        if let IrNode::Literal { value: Literal::Integer(result), .. } = optimized {
            assert_eq!(result, 5);
        } else {
            panic!("Expected folded constant");
        }
    }
}
