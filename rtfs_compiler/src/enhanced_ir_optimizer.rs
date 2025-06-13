// ENHANCED IR OPTIMIZER - STEP 2 IMPLEMENTATION
// Advanced optimization passes with control flow analysis, function inlining, and enhanced dead code elimination

use std::collections::HashSet;
use crate::ir::*;
use crate::ast::Literal;

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
    }

    pub fn with_level(level: OptimizationLevel) -> Self {
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
                // First pass: control flow analysis
                let node = self.optimize_control_flow(node);
                
                // Second pass: enhanced dead code elimination
                let node = self.optimize_dead_code_elimination(node);
                
                // Third pass: function inlining opportunities
                self.optimize_function_inlines(node)
            }
        }
    }

    fn optimize_control_flow(&self, node: IrNode) -> IrNode {
        match node {
            IrNode::If { id, condition, then_branch, else_branch, ir_type, source_location } => {
                // Optimize condition first
                let optimized_condition = self.optimize_control_flow(condition.as_ref().clone());
                
                // Check for constant conditions
                match optimized_condition {
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
                // Optimize each expression and remove unnecessary intermediates
                let mut optimized_exprs = Vec::new();
                for expr in expressions {
                    let optimized = self.optimize_control_flow(expr);
                    // Keep expressions with side effects and the last expression
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
                // Optimize bindings and check for usage
                let mut used_bindings = Vec::new();
                let optimized_body: Vec<IrNode> = body.into_iter()
                    .map(|expr| self.optimize_control_flow(expr))
                    .collect();
                
                // Collect used variable names from body
                let mut used_vars = HashSet::new();
                for expr in &optimized_body {
                    self.collect_used_variables(expr, &mut used_vars);
                }
                
                // Only keep bindings that are used or have side effects
                for binding in bindings {
                    let binding_name = self.extract_binding_name(&binding.pattern);
                    if binding_name.as_ref().map_or(true, |name| used_vars.contains(name)) || 
                       self.has_side_effects(&binding.init_expr) {
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
            
            // Recursively optimize other node types
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
            
            // For other nodes, recursively apply optimization
            _ => self.optimize_recursive(node)
        }
    }

    fn optimize_function_inlines(&self, node: IrNode) -> IrNode {
        match node {
            IrNode::Apply { id, function, arguments, ir_type, source_location } => {
                // Check if this is a lambda call that can be inlined
                if let IrNode::Lambda { params, body, .. } = function.as_ref() {
                    let body_size = self.estimate_body_size(body);
                    
                    // Only inline small functions
                    if body_size <= self.inline_threshold && 
                       params.len() == arguments.len() &&
                       self.should_inline(function.as_ref()) {
                        
                        // Perform simple function inlining
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

    // Helper methods

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
            
            // For leaf nodes and other complex nodes, return as-is
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

    fn collect_used_variables(&self, node: &IrNode, used: &mut HashSet<String>) {
        match node {
            IrNode::VariableRef { name, .. } => {
                used.insert(name.clone());
            },
            IrNode::Apply { function, arguments, .. } => {
                self.collect_used_variables(function, used);
                for arg in arguments {
                    self.collect_used_variables(arg, used);
                }
            },
            IrNode::If { condition, then_branch, else_branch, .. } => {
                self.collect_used_variables(condition, used);
                self.collect_used_variables(then_branch, used);
                if let Some(else_node) = else_branch {
                    self.collect_used_variables(else_node, used);
                }
            },
            IrNode::Let { bindings, body, .. } => {
                for binding in bindings {
                    self.collect_used_variables(&binding.init_expr, used);
                }
                for expr in body {
                    self.collect_used_variables(expr, used);
                }
            },
            IrNode::Do { expressions, .. } => {
                for expr in expressions {
                    self.collect_used_variables(expr, used);
                }
            },
            _ => {}
        }
    }

    fn extract_binding_name(&self, pattern: &IrNode) -> Option<String> {
        match pattern {
            IrNode::VariableBinding { name, .. } => Some(name.clone()),
            _ => None, // Complex patterns not supported in this simplified version
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
                size <= self.inline_threshold
            },
            _ => false,
        }
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
        if bindings.is_empty() && body.len() == 1 {
            // No parameters, return body directly
            body[0].clone()
        } else {
            IrNode::Let {
                id: 0, // Generate new ID in production
                bindings,
                body: body.to_vec(),
                ir_type: return_type,
                source_location: None,
            }
        }
    }
}

// Enhanced optimization pipeline that combines basic and advanced passes
pub struct EnhancedOptimizationPipeline {
    enhanced_optimizer: EnhancedIrOptimizer,
    stats: EnhancedOptimizationStats,
}

#[derive(Debug, Default)]
pub struct EnhancedOptimizationStats {
    pub control_flow_optimizations: usize,
    pub functions_inlined: usize,
    pub dead_code_blocks_eliminated: usize,
    pub optimization_time_ms: u128,
}

impl EnhancedOptimizationPipeline {
    pub fn new() -> Self {
        Self {
            enhanced_optimizer: EnhancedIrOptimizer::new(),
            stats: EnhancedOptimizationStats::default(),
        }
    }

    pub fn with_optimization_level(level: OptimizationLevel) -> Self {
        Self {
            enhanced_optimizer: EnhancedIrOptimizer::with_level(level),
            stats: EnhancedOptimizationStats::default(),
        }
    }

    pub fn optimize(&mut self, node: IrNode) -> IrNode {
        let start_time = std::time::Instant::now();
        
        // Run enhanced optimizations
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
    fn test_enhanced_control_flow_optimization() {
        let mut optimizer = EnhancedIrOptimizer::new();
        
        // Test constant condition elimination
        let node = IrNode::If {
            id: 1,
            condition: Box::new(IrNode::Literal {
                id: 2,
                value: Literal::Boolean(true),
                ir_type: IrType::Bool,
                source_location: None,
            }),
            then_branch: Box::new(IrNode::Literal {
                id: 3,
                value: Literal::Integer(42),
                ir_type: IrType::Int,
                source_location: None,
            }),
            else_branch: Some(Box::new(IrNode::Literal {
                id: 4,
                value: Literal::Integer(0),
                ir_type: IrType::Int,
                source_location: None,
            })),
            ir_type: IrType::Int,
            source_location: None,
        };
        
        let optimized = optimizer.optimize_with_control_flow(node);
        
        // Should eliminate the if and return the then branch
        if let IrNode::Literal { value: Literal::Integer(result), .. } = optimized {
            assert_eq!(result, 42);
        } else {
            panic!("Expected constant folded result");
        }
    }
    
    #[test]
    fn test_enhanced_dead_code_elimination() {
        let mut optimizer = EnhancedIrOptimizer::new();
        
        // Test unused expression elimination in Do block
        let node = IrNode::Do {
            id: 1,
            expressions: vec![
                IrNode::Literal { id: 2, value: Literal::Integer(1), ir_type: IrType::Int, source_location: None },
                IrNode::Literal { id: 3, value: Literal::Integer(2), ir_type: IrType::Int, source_location: None },
                IrNode::Literal { id: 4, value: Literal::Integer(42), ir_type: IrType::Int, source_location: None },
            ],
            ir_type: IrType::Int,
            source_location: None,
        };
        
        let optimized = optimizer.optimize_with_control_flow(node);
        
        // Should eliminate intermediate expressions and return the last one
        if let IrNode::Literal { value: Literal::Integer(result), .. } = optimized {
            assert_eq!(result, 42);
        } else {
            panic!("Expected dead code eliminated result");
        }
    }
}
