// Advanced Optimization Demonstration
// Shows the power of the RTFS IR optimization pipeline on complex real-world scenarios

use crate::ast::*;
use crate::ir::*;
use crate::ir_converter::IrConverter;
use crate::ir_optimizer::OptimizationPipeline;
use std::time::Instant;

/// Demonstrate advanced optimizations on complex RTFS programs
pub fn demonstrate_advanced_optimizations() {
    println!("🔥 RTFS Advanced Optimization Demonstration");
    println!("===========================================\n");

    // Test different optimization scenarios
    let test_cases = vec![
        ("Mathematical Expression Optimization", create_math_heavy_program()),
        ("Control Flow Optimization", create_control_flow_program()),
        ("Function Inlining Optimization", create_function_heavy_program()),
        ("Dead Code Elimination", create_dead_code_program()),
    ];

    for (name, program) in test_cases {
        println!("📋 Test Case: {}", name);
        println!("{}", "─".repeat(50));
        
        run_optimization_test(&program);
        println!();
    }

    println!("🏆 Overall Optimization Benefits:");
    println!("   • Compile-time constant evaluation");
    println!("   • Elimination of redundant computations");  
    println!("   • Simplified control flow");
    println!("   • Reduced memory footprint");
    println!("   • Faster runtime execution");
}

fn run_optimization_test(program: &Expression) {
    let start_time = Instant::now();
    
    // Convert to IR
    let mut converter = IrConverter::new();
    let ir_result = converter.convert(program);
    
    match ir_result {
        Ok(original_ir) => {
            let conversion_time = start_time.elapsed();
            
            println!("   ✅ IR Conversion: {:?}", conversion_time);
            
            // Measure original IR complexity
            let original_stats = analyze_ir_complexity(&original_ir);
            
            // Apply optimizations
            let opt_start = Instant::now();
            let mut optimizer = OptimizationPipeline::standard();
            let optimized_ir = optimizer.optimize(original_ir.clone());
            let optimization_time = opt_start.elapsed();
            
            println!("   ⚡ Optimization: {:?}", optimization_time);
            
            // Measure optimized IR complexity
            let optimized_stats = analyze_ir_complexity(&optimized_ir);
            
            // Report improvements
            let complexity_improvement = if optimized_stats.total_operations > 0 {
                original_stats.total_operations as f64 / optimized_stats.total_operations as f64
            } else {
                f64::INFINITY
            };
            
            println!("   📊 Results:");
            println!("      Nodes: {} → {} ({:.1}% reduction)", 
                   original_stats.node_count, 
                   optimized_stats.node_count,
                   (1.0 - optimized_stats.node_count as f64 / original_stats.node_count as f64) * 100.0);
            println!("      Operations: {} → {} ({:.2}x faster)",
                   original_stats.total_operations,
                   optimized_stats.total_operations,
                   complexity_improvement);
            println!("      Memory: {} → {} bytes ({:.1}% reduction)",
                   original_stats.estimated_memory,
                   optimized_stats.estimated_memory,
                   (1.0 - optimized_stats.estimated_memory as f64 / original_stats.estimated_memory as f64) * 100.0);
            
            // Show specific optimizations applied
            show_optimizations_applied(&original_ir, &optimized_ir);
        }
        Err(e) => {
            println!("   ❌ Failed: {:?}", e);
        }
    }
}

#[derive(Debug)]
struct IRComplexityStats {
    node_count: usize,
    total_operations: usize,
    estimated_memory: usize,
    function_calls: usize,
    literals: usize,
    conditionals: usize,
}

fn analyze_ir_complexity(ir: &IrNode) -> IRComplexityStats {
    let mut stats = IRComplexityStats {
        node_count: 0,
        total_operations: 0,
        estimated_memory: 0,
        function_calls: 0,
        literals: 0,
        conditionals: 0,
    };
    
    analyze_ir_recursive(ir, &mut stats);
    stats
}

fn analyze_ir_recursive(node: &IrNode, stats: &mut IRComplexityStats) {
    stats.node_count += 1;
    stats.estimated_memory += std::mem::size_of_val(node);
    
    match node {
        IrNode::Literal { .. } => {
            stats.literals += 1;
            stats.total_operations += 1;
        }
        IrNode::VariableRef { .. } => {
            stats.total_operations += 2; // Variable lookup cost
        }
        IrNode::Apply { function, arguments, .. } => {
            stats.function_calls += 1;
            stats.total_operations += 10 + arguments.len() * 2; // Function call overhead
            
            analyze_ir_recursive(function, stats);
            for arg in arguments {
                analyze_ir_recursive(arg, stats);
            }
        }
        IrNode::If { condition, then_branch, else_branch, .. } => {
            stats.conditionals += 1;
            stats.total_operations += 5; // Conditional evaluation cost
            
            analyze_ir_recursive(condition, stats);
            analyze_ir_recursive(then_branch, stats);
            if let Some(else_br) = else_branch {
                analyze_ir_recursive(else_br, stats);
            }
        }
        IrNode::Let { bindings, body, .. } => {
            stats.total_operations += bindings.len() * 3; // Binding overhead
            
            for binding in bindings {
                analyze_ir_recursive(&binding.pattern, stats);
                analyze_ir_recursive(&binding.init_expr, stats);
            }
            for expr in body {
                analyze_ir_recursive(expr, stats);
            }
        }
        IrNode::Do { expressions, .. } => {
            for expr in expressions {
                analyze_ir_recursive(expr, stats);
            }
        }
        IrNode::Lambda { params, body, .. } => {
            stats.total_operations += params.len() + body.len(); // Function definition cost
            
            for param in params {
                analyze_ir_recursive(param, stats);
            }
            for expr in body {
                analyze_ir_recursive(expr, stats);
            }
        }
        IrNode::LogStep { values, .. } => {
            stats.total_operations += 15; // Logging overhead
            for value in values {
                analyze_ir_recursive(value, stats);
            }
        }
        _ => {
            stats.total_operations += 5; // Default operation cost
        }
    }
}

fn show_optimizations_applied(original: &IrNode, optimized: &IrNode) {
    let original_constants = count_constant_expressions(original);
    let optimized_constants = count_constant_expressions(optimized);
    
    let original_branches = count_conditional_branches(original);
    let optimized_branches = count_conditional_branches(optimized);
    
    println!("   🔧 Optimizations Applied:");
    
    if original_constants > optimized_constants {
        println!("      • Constant folding: {} expressions pre-computed", 
               original_constants - optimized_constants);
    }
    
    if original_branches > optimized_branches {
        println!("      • Branch elimination: {} dead branches removed", 
               original_branches - optimized_branches);
    }
    
    let original_nodes = count_nodes(original);
    let optimized_nodes = count_nodes(optimized);
    
    if original_nodes > optimized_nodes {
        println!("      • Dead code elimination: {} nodes removed", 
               original_nodes - optimized_nodes);
    }
}

fn count_constant_expressions(node: &IrNode) -> usize {
    match node {
        IrNode::Apply { function, arguments, .. } => {
            if let IrNode::VariableRef { name, .. } = function.as_ref() {
                if ["+", "-", "*", "/"].contains(&name.as_str()) {
                    let all_literals = arguments.iter().all(|arg| matches!(arg, IrNode::Literal { .. }));
                    if all_literals {
                        return 1 + arguments.iter().map(count_constant_expressions).sum::<usize>();
                    }
                }
            }
            arguments.iter().map(count_constant_expressions).sum::<usize>() +
            count_constant_expressions(function)
        }
        IrNode::If { condition, then_branch, else_branch, .. } => {
            count_constant_expressions(condition) +
            count_constant_expressions(then_branch) +
            else_branch.as_ref().map_or(0, |e| count_constant_expressions(e))
        }
        IrNode::Let { bindings, body, .. } => {
            bindings.iter().map(|b| count_constant_expressions(&b.init_expr)).sum::<usize>() +
            body.iter().map(count_constant_expressions).sum::<usize>()
        }
        IrNode::Do { expressions, .. } => {
            expressions.iter().map(count_constant_expressions).sum::<usize>()
        }
        _ => 0,
    }
}

fn count_conditional_branches(node: &IrNode) -> usize {
    match node {
        IrNode::If { condition, then_branch, else_branch, .. } => {
            1 + count_conditional_branches(condition) +
            count_conditional_branches(then_branch) +
            else_branch.as_ref().map_or(0, |e| count_conditional_branches(e))
        }
        IrNode::Apply { function, arguments, .. } => {
            count_conditional_branches(function) +
            arguments.iter().map(count_conditional_branches).sum::<usize>()
        }
        IrNode::Let { bindings, body, .. } => {
            bindings.iter().map(|b| count_conditional_branches(&b.init_expr)).sum::<usize>() +
            body.iter().map(count_conditional_branches).sum::<usize>()
        }
        IrNode::Do { expressions, .. } => {
            expressions.iter().map(count_conditional_branches).sum::<usize>()
        }
        _ => 0,
    }
}

fn count_nodes(node: &IrNode) -> usize {
    1 + match node {
        IrNode::Apply { function, arguments, .. } => {
            count_nodes(function) + arguments.iter().map(count_nodes).sum::<usize>()
        }
        IrNode::If { condition, then_branch, else_branch, .. } => {
            count_nodes(condition) + count_nodes(then_branch) +
            else_branch.as_ref().map_or(0, |e| count_nodes(e))
        }
        IrNode::Let { bindings, body, .. } => {
            bindings.iter().map(|b| count_nodes(&b.pattern) + count_nodes(&b.init_expr)).sum::<usize>() +
            body.iter().map(count_nodes).sum::<usize>()
        }
        IrNode::Do { expressions, .. } => {
            expressions.iter().map(count_nodes).sum::<usize>()
        }
        IrNode::Lambda { params, body, .. } => {
            params.iter().map(count_nodes).sum::<usize>() +
            body.iter().map(count_nodes).sum::<usize>()
        }
        IrNode::LogStep { values, .. } => {
            values.iter().map(count_nodes).sum::<usize>()
        }
        _ => 0,
    }
}

/// Create a math-heavy program for constant folding optimization
fn create_math_heavy_program() -> Expression {
    // (let [a (+ 5 3)
    //       b (* 2 4)
    //       c (/ 16 2)
    //       result (+ (* a b) c)]
    //   result)
    
    Expression::Let(LetExpr {
        bindings: vec![
            LetBinding {
                pattern: Pattern::Symbol(Symbol("a".to_string())),
                type_annotation: None,
                value: Box::new(Expression::FunctionCall {
                    callee: Box::new(Expression::Symbol(Symbol("+".to_string()))),
                    arguments: vec![
                        Expression::Literal(Literal::Integer(5)),
                        Expression::Literal(Literal::Integer(3)),
                    ],
                }),
            },
            LetBinding {
                pattern: Pattern::Symbol(Symbol("b".to_string())),
                type_annotation: None,
                value: Box::new(Expression::FunctionCall {
                    callee: Box::new(Expression::Symbol(Symbol("*".to_string()))),
                    arguments: vec![
                        Expression::Literal(Literal::Integer(2)),
                        Expression::Literal(Literal::Integer(4)),
                    ],
                }),
            },
            LetBinding {
                pattern: Pattern::Symbol(Symbol("c".to_string())),
                type_annotation: None,
                value: Box::new(Expression::FunctionCall {
                    callee: Box::new(Expression::Symbol(Symbol("/".to_string()))),
                    arguments: vec![
                        Expression::Literal(Literal::Integer(16)),
                        Expression::Literal(Literal::Integer(2)),
                    ],
                }),
            },
            LetBinding {
                pattern: Pattern::Symbol(Symbol("result".to_string())),
                type_annotation: None,
                value: Box::new(Expression::FunctionCall {
                    callee: Box::new(Expression::Symbol(Symbol("+".to_string()))),
                    arguments: vec![
                        Expression::FunctionCall {
                            callee: Box::new(Expression::Symbol(Symbol("*".to_string()))),
                            arguments: vec![
                                Expression::Symbol(Symbol("a".to_string())),
                                Expression::Symbol(Symbol("b".to_string())),
                            ],
                        },
                        Expression::Symbol(Symbol("c".to_string())),
                    ],
                }),
            },
        ],
        body: vec![Expression::Symbol(Symbol("result".to_string()))],
    })
}

/// Create a control-flow heavy program for branch optimization
fn create_control_flow_program() -> Expression {
    // (let [x 10]
    //   (if true
    //     (if false 999 x)
    //     (if true 888 777)))
    
    Expression::Let(LetExpr {
        bindings: vec![
            LetBinding {
                pattern: Pattern::Symbol(Symbol("x".to_string())),
                type_annotation: None,
                value: Box::new(Expression::Literal(Literal::Integer(10))),
            },
        ],
        body: vec![
            Expression::If(IfExpr {
                condition: Box::new(Expression::Literal(Literal::Boolean(true))),
                then_branch: Box::new(Expression::If(IfExpr {
                    condition: Box::new(Expression::Literal(Literal::Boolean(false))),
                    then_branch: Box::new(Expression::Literal(Literal::Integer(999))),
                    else_branch: Some(Box::new(Expression::Symbol(Symbol("x".to_string())))),
                })),
                else_branch: Some(Box::new(Expression::If(IfExpr {
                    condition: Box::new(Expression::Literal(Literal::Boolean(true))),
                    then_branch: Box::new(Expression::Literal(Literal::Integer(888))),
                    else_branch: Some(Box::new(Expression::Literal(Literal::Integer(777)))),
                }))),
            }),
        ],
    })
}

/// Create a function-heavy program for inlining optimization
fn create_function_heavy_program() -> Expression {
    // (let [add (fn [x y] (+ x y))
    //       square (fn [x] (* x x))]
    //   (add (square 3) (square 4)))
    
    Expression::Let(LetExpr {
        bindings: vec![
            LetBinding {
                pattern: Pattern::Symbol(Symbol("add".to_string())),
                type_annotation: None,
                value: Box::new(Expression::Fn(FnExpr {
                    params: vec![
                        ParamDef {
                            pattern: Pattern::Symbol(Symbol("x".to_string())),
                            type_annotation: Some(TypeExpr::Primitive(PrimitiveType::Int)),
                        },
                        ParamDef {
                            pattern: Pattern::Symbol(Symbol("y".to_string())),
                            type_annotation: Some(TypeExpr::Primitive(PrimitiveType::Int)),
                        },
                    ],
                    variadic_param: None,
                    return_type: Some(TypeExpr::Primitive(PrimitiveType::Int)),
                    body: vec![
                        Expression::FunctionCall {
                            callee: Box::new(Expression::Symbol(Symbol("+".to_string()))),
                            arguments: vec![
                                Expression::Symbol(Symbol("x".to_string())),
                                Expression::Symbol(Symbol("y".to_string())),
                            ],
                        }
                    ],
                })),
            },
            LetBinding {
                pattern: Pattern::Symbol(Symbol("square".to_string())),
                type_annotation: None,
                value: Box::new(Expression::Fn(FnExpr {
                    params: vec![
                        ParamDef {
                            pattern: Pattern::Symbol(Symbol("x".to_string())),
                            type_annotation: Some(TypeExpr::Primitive(PrimitiveType::Int)),
                        },
                    ],
                    variadic_param: None,
                    return_type: Some(TypeExpr::Primitive(PrimitiveType::Int)),
                    body: vec![
                        Expression::FunctionCall {
                            callee: Box::new(Expression::Symbol(Symbol("*".to_string()))),
                            arguments: vec![
                                Expression::Symbol(Symbol("x".to_string())),
                                Expression::Symbol(Symbol("x".to_string())),
                            ],
                        }
                    ],
                })),
            },
        ],
        body: vec![
            Expression::FunctionCall {
                callee: Box::new(Expression::Symbol(Symbol("add".to_string()))),
                arguments: vec![
                    Expression::FunctionCall {
                        callee: Box::new(Expression::Symbol(Symbol("square".to_string()))),
                        arguments: vec![Expression::Literal(Literal::Integer(3))],
                    },
                    Expression::FunctionCall {
                        callee: Box::new(Expression::Symbol(Symbol("square".to_string()))),
                        arguments: vec![Expression::Literal(Literal::Integer(4))],
                    },
                ],
            }
        ],
    })
}

/// Create a program with lots of dead code
fn create_dead_code_program() -> Expression {
    // (let [used 42
    //       unused1 (+ 1 2)
    //       unused2 "dead string"
    //       unused3 [1 2 3]]
    //   (do
    //     "unused expression"
    //     (+ 10 20)
    //     used))
    
    Expression::Let(LetExpr {
        bindings: vec![
            LetBinding {
                pattern: Pattern::Symbol(Symbol("used".to_string())),
                type_annotation: None,
                value: Box::new(Expression::Literal(Literal::Integer(42))),
            },
            LetBinding {
                pattern: Pattern::Symbol(Symbol("unused1".to_string())),
                type_annotation: None,
                value: Box::new(Expression::FunctionCall {
                    callee: Box::new(Expression::Symbol(Symbol("+".to_string()))),
                    arguments: vec![
                        Expression::Literal(Literal::Integer(1)),
                        Expression::Literal(Literal::Integer(2)),
                    ],
                }),
            },
            LetBinding {
                pattern: Pattern::Symbol(Symbol("unused2".to_string())),
                type_annotation: None,
                value: Box::new(Expression::Literal(Literal::String("dead string".to_string()))),
            },
            LetBinding {
                pattern: Pattern::Symbol(Symbol("unused3".to_string())),
                type_annotation: None,
                value: Box::new(Expression::Vector(vec![
                    Expression::Literal(Literal::Integer(1)),
                    Expression::Literal(Literal::Integer(2)),
                    Expression::Literal(Literal::Integer(3)),
                ])),
            },
        ],
        body: vec![
            Expression::Do(DoExpr {
                expressions: vec![
                    Expression::Literal(Literal::String("unused expression".to_string())),
                    Expression::FunctionCall {
                        callee: Box::new(Expression::Symbol(Symbol("+".to_string()))),
                        arguments: vec![
                            Expression::Literal(Literal::Integer(10)),
                            Expression::Literal(Literal::Integer(20)),
                        ],
                    },
                    Expression::Symbol(Symbol("used".to_string())),
                ],
            })
        ],
    })
}
