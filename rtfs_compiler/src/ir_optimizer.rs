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
