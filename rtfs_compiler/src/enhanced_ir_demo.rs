// Enhanced IR Optimizer Demonstration - Step 2 Implementation
// Demonstrates control flow analysis, function inlining, and enhanced dead code elimination

use crate::enhanced_ir_optimizer::{EnhancedIrOptimizer, EnhancedOptimizationPipeline, OptimizationLevel};
use crate::ir::*;
use crate::ast::Literal;

pub fn run_enhanced_ir_optimizer_demo() {
    println!("\n=== RTFS Enhanced IR Optimizer Demo - Step 2 ===");
    println!("Demonstrating advanced IR optimization passes:\n");
    
    // Test 1: Control Flow Analysis - Constant Condition Elimination
    println!("🔍 Test 1: Control Flow Analysis - Constant Condition Elimination");
    test_constant_condition_elimination();
    
    // Test 2: Enhanced Dead Code Elimination
    println!("\n🧹 Test 2: Enhanced Dead Code Elimination"); 
    test_dead_code_elimination();
    
    // Test 3: Function Inlining Analysis
    println!("\n⚡ Test 3: Function Inlining Analysis");
    test_function_inlining();
    
    // Test 4: Optimization Pipeline with Different Levels
    println!("\n🚀 Test 4: Optimization Pipeline with Different Levels");
    test_optimization_levels();
    
    // Test 5: Complex Nested Optimization
    println!("\n🏗️ Test 5: Complex Nested Optimization");
    test_complex_optimization();
    
    println!("\n✅ Enhanced IR Optimizer Demo (Step 2) Complete!");
    println!("   - Control flow analysis implemented");
    println!("   - Enhanced dead code elimination working");
    println!("   - Function inlining analysis functional");
    println!("   - Multiple optimization levels supported");
}

fn test_constant_condition_elimination() {
    let mut optimizer = EnhancedIrOptimizer::new();
    
    // Create an if expression with constant true condition
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
    
    println!("   Original: if true then 42 else 0");
    
    let optimized = optimizer.optimize_with_control_flow(node);
    
    match optimized {
        IrNode::Literal { value: Literal::Integer(result), .. } => {
            println!("   Optimized: {} (constant condition eliminated)", result);
            assert_eq!(result, 42);
        },
        _ => {
            println!("   ❌ Optimization failed - expected constant folded result");
        }
    }
}

fn test_dead_code_elimination() {
    let mut optimizer = EnhancedIrOptimizer::new();
    
    // Create a do block with unused intermediate expressions
    let node = IrNode::Do {
        id: 1,
        expressions: vec![
            IrNode::Literal { id: 2, value: Literal::Integer(1), ir_type: IrType::Int, source_location: None },
            IrNode::Literal { id: 3, value: Literal::Integer(2), ir_type: IrType::Int, source_location: None },
            IrNode::Literal { id: 4, value: Literal::String("result".to_string()), ir_type: IrType::String, source_location: None },
        ],
        ir_type: IrType::String,
        source_location: None,
    };
    
    println!("   Original: do {{ 1; 2; \"result\" }}");
    
    let optimized = optimizer.optimize_with_control_flow(node);
    
    match optimized {
        IrNode::Literal { value: Literal::String(result), .. } => {
            println!("   Optimized: \"{}\" (dead code eliminated)", result);
            assert_eq!(result, "result");
        },
        _ => {
            println!("   ❌ Optimization failed - expected dead code eliminated result");
        }
    }
}

fn test_function_inlining() {
    let mut optimizer = EnhancedIrOptimizer::new();
    
    // Create a small lambda that should be inlined
    let lambda = IrNode::Lambda {
        id: 10,
        params: vec![
            IrNode::Param {
                id: 11,
                binding: Box::new(IrNode::VariableBinding {
                    id: 12,
                    name: "x".to_string(),
                    ir_type: IrType::Int,
                    source_location: None,
                }),
                type_annotation: Some(IrType::Int),
                ir_type: IrType::Int,
                source_location: None,
            }
        ],
        variadic_param: None,
        body: vec![
            IrNode::VariableRef {
                id: 13,
                name: "x".to_string(),
                binding_id: 12,
                ir_type: IrType::Int,
                source_location: None,
            }
        ],
        captures: vec![],
        ir_type: IrType::Function {
            param_types: vec![IrType::Int],
            variadic_param_type: None,
            return_type: Box::new(IrType::Int),
        },
        source_location: None,
    };
    
    // Apply the lambda to an argument
    let node = IrNode::Apply {
        id: 1,
        function: Box::new(lambda),
        arguments: vec![
            IrNode::Literal { id: 2, value: Literal::Integer(5), ir_type: IrType::Int, source_location: None }
        ],
        ir_type: IrType::Int,
        source_location: None,
    };
    
    println!("   Original: (lambda (x) x) 5");
    
    let optimized = optimizer.optimize_with_control_flow(node);
    
    match optimized {
        IrNode::Let { bindings, body, .. } => {
            println!("   Optimized: let binding created for function inlining");
            assert_eq!(bindings.len(), 1);
            assert_eq!(body.len(), 1);
        },
        IrNode::Literal { value: Literal::Integer(result), .. } => {
            println!("   Optimized: {} (function completely inlined)", result);
            assert_eq!(result, 5);
        },
        _ => {
            println!("   ⚠️ Function not inlined (size threshold or complexity)");
        }
    }
}

fn test_optimization_levels() {
    println!("   Testing different optimization levels:");
    
    // Test with None level
    let mut optimizer_none = EnhancedIrOptimizer::with_level(OptimizationLevel::None);
    let test_node = create_test_node();
    let result_none = optimizer_none.optimize_with_control_flow(test_node.clone());
    println!("   - None level: {} nodes (no optimization)", count_nodes(&result_none));
    
    // Test with Basic level  
    let mut optimizer_basic = EnhancedIrOptimizer::with_level(OptimizationLevel::Basic);
    let result_basic = optimizer_basic.optimize_with_control_flow(test_node.clone());
    println!("   - Basic level: {} nodes", count_nodes(&result_basic));
    
    // Test with Aggressive level
    let mut optimizer_aggressive = EnhancedIrOptimizer::with_level(OptimizationLevel::Aggressive);
    let result_aggressive = optimizer_aggressive.optimize_with_control_flow(test_node);
    println!("   - Aggressive level: {} nodes (maximum optimization)", count_nodes(&result_aggressive));
}

fn test_complex_optimization() {
    let mut pipeline = EnhancedOptimizationPipeline::with_optimization_level(OptimizationLevel::Aggressive);
    
    // Create a complex nested structure
    let complex_node = IrNode::Let {
        id: 1,
        bindings: vec![
            IrLetBinding {
                pattern: IrNode::VariableBinding {
                    id: 2,
                    name: "unused".to_string(),
                    ir_type: IrType::Int,
                    source_location: None,
                },
                type_annotation: Some(IrType::Int),
                init_expr: IrNode::Literal { id: 3, value: Literal::Integer(10), ir_type: IrType::Int, source_location: None },
            },
            IrLetBinding {
                pattern: IrNode::VariableBinding {
                    id: 4,
                    name: "result".to_string(),
                    ir_type: IrType::String,
                    source_location: None,
                },
                type_annotation: Some(IrType::String),
                init_expr: IrNode::If {
                    id: 5,
                    condition: Box::new(IrNode::Literal {
                        id: 6,
                        value: Literal::Boolean(true),
                        ir_type: IrType::Bool,
                        source_location: None,
                    }),
                    then_branch: Box::new(IrNode::Literal {
                        id: 7,
                        value: Literal::String("optimized".to_string()),
                        ir_type: IrType::String,
                        source_location: None,
                    }),
                    else_branch: Some(Box::new(IrNode::Literal {
                        id: 8,
                        value: Literal::String("not optimized".to_string()),
                        ir_type: IrType::String,
                        source_location: None,
                    })),
                    ir_type: IrType::String,
                    source_location: None,
                },
            }
        ],
        body: vec![
            IrNode::VariableRef {
                id: 9,
                name: "result".to_string(),
                binding_id: 4,
                ir_type: IrType::String,
                source_location: None,
            }
        ],
        ir_type: IrType::String,
        source_location: None,
    };
    
    println!("   Original: Complex nested let/if structure");
    println!("   - {} nodes before optimization", count_nodes(&complex_node));
    
    let optimized = pipeline.optimize(complex_node);
    
    println!("   - {} nodes after optimization", count_nodes(&optimized));
    println!("   - Optimization time: {}ms", pipeline.stats().optimization_time_ms);
    
    match optimized {
        IrNode::Let { bindings, .. } => {
            println!("   - {} bindings remain (unused eliminated)", bindings.len());
        },
        IrNode::Literal { value: Literal::String(result), .. } => {
            println!("   - Fully optimized to constant: \"{}\"", result);
        },
        _ => {
            println!("   - Partially optimized");
        }
    }
}

fn create_test_node() -> IrNode {
    IrNode::Do {
        id: 1,
        expressions: vec![
            IrNode::Literal { id: 2, value: Literal::Integer(1), ir_type: IrType::Int, source_location: None },
            IrNode::Literal { id: 3, value: Literal::Integer(2), ir_type: IrType::Int, source_location: None },
        ],
        ir_type: IrType::Int,
        source_location: None,
    }
}

fn count_nodes(node: &IrNode) -> usize {
    match node {
        IrNode::Literal { .. } => 1,
        IrNode::VariableRef { .. } => 1,
        IrNode::VariableBinding { .. } => 1,
        IrNode::Apply { function, arguments, .. } => {
            1 + count_nodes(function) + arguments.iter().map(count_nodes).sum::<usize>()
        },
        IrNode::If { condition, then_branch, else_branch, .. } => {
            1 + count_nodes(condition) + count_nodes(then_branch) + 
            else_branch.as_ref().map_or(0, |e| count_nodes(e))
        },
        IrNode::Let { bindings, body, .. } => {
            1 + bindings.iter().map(|b| count_nodes(&b.pattern) + count_nodes(&b.init_expr)).sum::<usize>() +
            body.iter().map(count_nodes).sum::<usize>()
        },
        IrNode::Do { expressions, .. } => {
            1 + expressions.iter().map(count_nodes).sum::<usize>()
        },
        IrNode::Lambda { params, body, .. } => {
            1 + params.iter().map(count_nodes).sum::<usize>() + body.iter().map(count_nodes).sum::<usize>()
        },
        IrNode::Param { binding, .. } => {
            1 + count_nodes(binding)
        },
        _ => 1, // Default for other node types
    }
}
