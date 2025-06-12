// Complete IR Pipeline Demonstration
// Shows AST→IR conversion with a real RTFS program

use crate::ast::*;
use crate::ir::*;
use crate::ir_converter::IrConverter;
use crate::ir_optimizer::{OptimizationPipeline, ConstantFoldingPass, DeadCodeEliminationPass};
use std::collections::HashMap;

/// Demonstrate the complete IR conversion pipeline
pub fn demonstrate_ir_pipeline() {
    println!("🚀 RTFS IR Pipeline Demonstration");
    println!("==================================\n");

    // Create a comprehensive example AST representing a real RTFS program
    let sample_ast = create_comprehensive_sample_ast();
    
    println!("📝 Sample RTFS Program (AST representation):");
    print_ast_structure(&sample_ast);
    
    println!("\n🔄 Converting AST to IR...\n");
    
    // Convert AST to IR
    let mut converter = IrConverter::new();
    match converter.convert(&sample_ast) {
        Ok(ir_node) => {
            println!("✅ IR Conversion Successful!");
            println!("\n🔧 Generated IR Structure:");
            print_ir_structure(&ir_node, 0);
            
            println!("\n📊 IR Conversion Statistics:");
            print_conversion_stats(&ir_node);
            
            println!("\n🎯 Type Information:");
            print_type_analysis(&ir_node);
            
            println!("\n🔮 Optimization Opportunities:");
            analyze_optimization_opportunities(&ir_node);
        }
        Err(error) => {
            println!("❌ IR Conversion Failed: {:?}", error);
        }
    }
    
    println!("\n{}", "=".repeat(50));
    println!("🎉 IR Pipeline Demonstration Complete!");
}

/// Demonstrate the complete IR conversion pipeline with optimizations
pub fn demonstrate_ir_optimization_pipeline() {
    println!("🚀 RTFS IR Optimization Pipeline Demonstration");
    println!("===============================================\n");

    // Create a sample AST with optimization opportunities
    let sample_ast = create_optimization_sample_ast();
    
    println!("📝 Sample RTFS Program (with optimization opportunities):");
    print_ast_structure(&sample_ast);
    
    println!("\n🔄 Converting AST to IR...\n");
    
    // Convert AST to IR
    let mut converter = IrConverter::new();
    match converter.convert(&sample_ast) {
        Ok(ir_node) => {
            println!("✅ IR Conversion Successful!");
            println!("\n🔧 Original IR Structure (before optimization):");
            print_ir_structure(&ir_node, 0);
            
            println!("\n🎯 Applying Optimizations...");
            
            // Create and apply optimization pipeline
            let mut optimizer = OptimizationPipeline::new();
            optimizer.add_pass(ConstantFoldingPass::new());
            optimizer.add_pass(DeadCodeEliminationPass::new());
            
            let optimized_ir = optimizer.optimize(ir_node.clone());
            
            println!("\n🚀 Optimized IR Structure:");
            print_ir_structure(&optimized_ir, 0);
            
            println!("\n📊 Optimization Results:");
            compare_ir_nodes(&ir_node, &optimized_ir);
            
            println!("\n🔮 Performance Analysis:");
            analyze_performance_improvements(&ir_node, &optimized_ir);
        }
        Err(error) => {
            println!("❌ IR Conversion Failed: {:?}", error);
        }
    }
    
    println!("\n{}", "=".repeat(60));
    println!("🎉 IR Optimization Pipeline Demonstration Complete!");
}

/// Create a comprehensive sample AST that demonstrates various RTFS features
fn create_comprehensive_sample_ast() -> Expression {
    // Represents this RTFS program:
    // (let [x 42
    //       y (+ x 10)
    //       calculate (fn [a b] (+ a b (* a b)))]
    //   (do
    //     (log-step :info "Starting calculation" x y)
    //     (if (> x 40)
    //       (calculate x y)
    //       0)))
    
    Expression::Let(LetExpr {
        bindings: vec![
            // x = 42
            LetBinding {
                pattern: Pattern::Symbol(Symbol("x".to_string())),
                type_annotation: None,
                value: Box::new(Expression::Literal(Literal::Integer(42))),
            },
            // y = (+ x 10)
            LetBinding {
                pattern: Pattern::Symbol(Symbol("y".to_string())),
                type_annotation: None,
                value: Box::new(Expression::List(vec![
                    Expression::Symbol(Symbol("+".to_string())),
                    Expression::Symbol(Symbol("x".to_string())),
                    Expression::Literal(Literal::Integer(10)),
                ])),
            },
            // calculate = (fn [a b] (+ a b (* a b)))
            LetBinding {
                pattern: Pattern::Symbol(Symbol("calculate".to_string())),
                type_annotation: None,
                value: Box::new(Expression::Fn(FnExpr {
                    params: vec![
                        ParamDef {
                            pattern: Pattern::Symbol(Symbol("a".to_string())),
                            type_annotation: Some(TypeExpr::Primitive(PrimitiveType::Int)),
                        },
                        ParamDef {
                            pattern: Pattern::Symbol(Symbol("b".to_string())),
                            type_annotation: Some(TypeExpr::Primitive(PrimitiveType::Int)),
                        },
                    ],
                    variadic_param: None,
                    return_type: Some(TypeExpr::Primitive(PrimitiveType::Int)),
                    body: vec![
                        Expression::List(vec![
                            Expression::Symbol(Symbol("+".to_string())),
                            Expression::Symbol(Symbol("a".to_string())),
                            Expression::Symbol(Symbol("b".to_string())),
                            Expression::List(vec![
                                Expression::Symbol(Symbol("*".to_string())),
                                Expression::Symbol(Symbol("a".to_string())),
                                Expression::Symbol(Symbol("b".to_string())),
                            ]),
                        ])
                    ],
                })),
            },
        ],
        body: vec![
            Expression::Do(DoExpr {
                expressions: vec![
                    // (log-step :info "Starting calculation" x y)
                    Expression::LogStep(Box::new(LogStepExpr {
                        level: Some(Keyword("info".to_string())),
                        values: vec![
                            Expression::Literal(Literal::String("Starting calculation".to_string())),
                            Expression::Symbol(Symbol("x".to_string())),
                            Expression::Symbol(Symbol("y".to_string())),
                        ],
                        location: None,
                    })),
                    // (if (> x 40) (calculate x y) 0)
                    Expression::If(IfExpr {
                        condition: Box::new(Expression::List(vec![
                            Expression::Symbol(Symbol(">".to_string())),
                            Expression::Symbol(Symbol("x".to_string())),
                            Expression::Literal(Literal::Integer(40)),
                        ])),
                        then_branch: Box::new(Expression::List(vec![
                            Expression::Symbol(Symbol("calculate".to_string())),
                            Expression::Symbol(Symbol("x".to_string())),
                            Expression::Symbol(Symbol("y".to_string())),
                        ])),
                        else_branch: Some(Box::new(Expression::Literal(Literal::Integer(0)))),
                    }),
                ],
            }),
        ],
    })
}

/// Create a sample AST that demonstrates optimization opportunities
fn create_optimization_sample_ast() -> Expression {
    // Represents this RTFS program with obvious optimizations:
    // (let [x 10
    //       y 20
    //       unused 42    ; Dead code - unused variable
    //       z (+ 5 3)]   ; Constant folding opportunity
    //   (do
    //     "unused-string" ; Dead code - unused value
    //     (if true        ; Branch folding opportunity
    //         (+ x z)     ; More constant folding after z is folded
    //         999)))      ; Dead branch
    
    Expression::Let(LetExpr {
        bindings: vec![
            LetBinding {
                pattern: Pattern::Symbol(Symbol("x".to_string())),
                type_annotation: None,
                value: Box::new(Expression::Literal(Literal::Integer(10))),
            },
            LetBinding {
                pattern: Pattern::Symbol(Symbol("y".to_string())),
                type_annotation: None,
                value: Box::new(Expression::Literal(Literal::Integer(20))),
            },
            LetBinding {
                pattern: Pattern::Symbol(Symbol("unused".to_string())),
                type_annotation: None,
                value: Box::new(Expression::Literal(Literal::Integer(42))),
            },
            LetBinding {
                pattern: Pattern::Symbol(Symbol("z".to_string())),
                type_annotation: None,
                value: Box::new(Expression::FunctionCall {
                    callee: Box::new(Expression::Symbol(Symbol("+".to_string()))),
                    arguments: vec![
                        Expression::Literal(Literal::Integer(5)),
                        Expression::Literal(Literal::Integer(3)),
                    ],
                }),
            },
        ],
        body: vec![
            Expression::Do(DoExpr {
                expressions: vec![
                    Expression::Literal(Literal::String("unused-string".to_string())),
                    Expression::If(IfExpr {
                        condition: Box::new(Expression::Literal(Literal::Boolean(true))),
                        then_branch: Box::new(Expression::FunctionCall {
                            callee: Box::new(Expression::Symbol(Symbol("+".to_string()))),
                            arguments: vec![
                                Expression::Symbol(Symbol("x".to_string())),
                                Expression::Symbol(Symbol("z".to_string())),
                            ],
                        }),
                        else_branch: Some(Box::new(Expression::Literal(Literal::Integer(999)))),
                    }),
                ],
            }),
        ],
    })
}

/// Print a readable representation of the AST structure
fn print_ast_structure(expr: &Expression) {
    print_ast_recursive(expr, 0);
}

fn print_ast_recursive(expr: &Expression, indent: usize) {
    let prefix = "  ".repeat(indent);
    match expr {
        Expression::Let(let_expr) => {
            println!("{}📦 Let Expression", prefix);
            println!("{}  Bindings:", prefix);
            for (i, binding) in let_expr.bindings.iter().enumerate() {
                println!("{}    {}. {:?} =", prefix, i + 1, binding.pattern);
                print_ast_recursive(&binding.value, indent + 3);
            }
            println!("{}  Body:", prefix);
            for (i, body_expr) in let_expr.body.iter().enumerate() {
                println!("{}    {}.", prefix, i + 1);
                print_ast_recursive(body_expr, indent + 2);
            }
        }
        Expression::Fn(fn_expr) => {
            println!("{}🔧 Function Expression", prefix);
            println!("{}  Parameters: {:?}", prefix, fn_expr.params.iter().map(|p| &p.pattern).collect::<Vec<_>>());
            println!("{}  Return Type: {:?}", prefix, fn_expr.return_type);
            println!("{}  Body:", prefix);
            for (i, body_expr) in fn_expr.body.iter().enumerate() {
                println!("{}    {}.", prefix, i + 1);
                print_ast_recursive(body_expr, indent + 2);
            }
        }
        Expression::Do(do_expr) => {
            println!("{}📋 Do Expression", prefix);
            for (i, expr) in do_expr.expressions.iter().enumerate() {
                println!("{}  {}.", prefix, i + 1);
                print_ast_recursive(expr, indent + 1);
            }
        }
        Expression::If(if_expr) => {
            println!("{}❓ If Expression", prefix);
            println!("{}  Condition:", prefix);
            print_ast_recursive(&if_expr.condition, indent + 1);
            println!("{}  Then:", prefix);
            print_ast_recursive(&if_expr.then_branch, indent + 1);
            if let Some(else_branch) = &if_expr.else_branch {
                println!("{}  Else:", prefix);
                print_ast_recursive(else_branch, indent + 1);
            }
        }
        Expression::List(exprs) => {
            if !exprs.is_empty() {
                if let Expression::Symbol(sym) = &exprs[0] {
                    println!("{}📞 Function Call: {}", prefix, sym.0);
                    if exprs.len() > 1 {
                        println!("{}  Arguments:", prefix);
                        for (i, arg) in exprs.iter().skip(1).enumerate() {
                            println!("{}    {}.", prefix, i + 1);
                            print_ast_recursive(arg, indent + 2);
                        }
                    }
                } else {
                    println!("{}📋 List Expression", prefix);
                    for (i, expr) in exprs.iter().enumerate() {
                        println!("{}  {}.", prefix, i + 1);
                        print_ast_recursive(expr, indent + 1);
                    }
                }
            } else {
                println!("{}📋 Empty List", prefix);
            }
        }
        Expression::LogStep(log_expr) => {
            println!("{}📝 Log Step: {:?}", prefix, log_expr.level);
            for (i, value) in log_expr.values.iter().enumerate() {
                println!("{}  {}.", prefix, i + 1);
                print_ast_recursive(value, indent + 1);
            }
        }
        Expression::Literal(lit) => {
            println!("{}💎 Literal: {:?}", prefix, lit);
        }
        Expression::Symbol(sym) => {
            println!("{}🔗 Symbol: {}", prefix, sym.0);
        }
        _ => {
            println!("{}❔ Other: {:?}", prefix, expr);
        }
    }
}

/// Print a readable representation of the IR structure
fn print_ir_structure(node: &IrNode, indent: usize) {
    let prefix = "  ".repeat(indent);
    match node {
        IrNode::Let { bindings, body, ir_type, .. } => {
            println!("{}🏗️  IR Let [Type: {:?}]", prefix, ir_type);
            println!("{}   Bindings:", prefix);
            for (i, binding) in bindings.iter().enumerate() {
                println!("{}     {}.", prefix, i + 1);
                print_ir_structure(&binding.pattern, indent + 2);
                println!("{}       Init:", prefix);
                print_ir_structure(&binding.init_expr, indent + 3);
            }
            println!("{}   Body:", prefix);
            for (i, expr) in body.iter().enumerate() {
                println!("{}     {}.", prefix, i + 1);
                print_ir_structure(expr, indent + 2);
            }
        }
        IrNode::Lambda { params, body, ir_type, .. } => {
            println!("{}⚡ IR Lambda [Type: {:?}]", prefix, ir_type);
            println!("{}   Parameters: {}", prefix, params.len());
            for (i, param) in params.iter().enumerate() {
                println!("{}     {}. [Type: {:?}]", prefix, i + 1, param.ir_type());
                print_ir_structure(param, indent + 2);
            }
            println!("{}   Body:", prefix);
            for (i, expr) in body.iter().enumerate() {
                println!("{}     {}.", prefix, i + 1);
                print_ir_structure(expr, indent + 2);
            }
        }
        IrNode::Apply { function, arguments, ir_type, .. } => {
            println!("{}📞 IR Apply [Return Type: {:?}]", prefix, ir_type);
            println!("{}   Function:", prefix);
            print_ir_structure(function, indent + 1);
            println!("{}   Arguments:", prefix);
            for (i, arg) in arguments.iter().enumerate() {
                println!("{}     {}.", prefix, i + 1);
                print_ir_structure(arg, indent + 2);
            }
        }
        IrNode::Do { expressions, ir_type, .. } => {
            println!("{}📋 IR Do [Type: {:?}]", prefix, ir_type);
            for (i, expr) in expressions.iter().enumerate() {
                println!("{}   {}.", prefix, i + 1);
                print_ir_structure(expr, indent + 1);
            }
        }
        IrNode::If { condition, then_branch, else_branch, ir_type, .. } => {
            println!("{}❓ IR If [Type: {:?}]", prefix, ir_type);
            println!("{}   Condition:", prefix);
            print_ir_structure(condition, indent + 1);
            println!("{}   Then:", prefix);
            print_ir_structure(then_branch, indent + 1);
            if let Some(else_br) = else_branch {
                println!("{}   Else:", prefix);
                print_ir_structure(else_br, indent + 1);
            }
        }
        IrNode::VariableRef { name, ir_type, .. } => {
            println!("{}🔗 IR VarRef: {} [Type: {:?}]", prefix, name, ir_type);
        }
        IrNode::VariableBinding { name, ir_type, .. } => {
            println!("{}🏷️  IR VarBinding: {} [Type: {:?}]", prefix, name, ir_type);
        }
        IrNode::Literal { value, ir_type, .. } => {
            println!("{}💎 IR Literal: {:?} [Type: {:?}]", prefix, value, ir_type);
        }
        IrNode::LogStep { level, values, ir_type, .. } => {
            println!("{}📝 IR LogStep: {:?} [Type: {:?}]", prefix, level, ir_type);
            for (i, value) in values.iter().enumerate() {
                println!("{}   {}.", prefix, i + 1);
                print_ir_structure(value, indent + 1);
            }
        }
        _ => {
            println!("{}❔ IR Other: [Type: {:?}]", prefix, node.ir_type());
        }
    }
}

/// Print conversion statistics
fn print_conversion_stats(node: &IrNode) {
    let mut stats = ConversionStats::default();
    collect_stats(node, &mut stats);
    
    println!("   📊 Total IR Nodes: {}", stats.total_nodes);
    println!("   🔗 Variable References: {}", stats.variable_refs);
    println!("   📞 Function Calls: {}", stats.function_calls);
    println!("   ⚡ Lambda Expressions: {}", stats.lambdas);
    println!("   📦 Let Bindings: {}", stats.let_bindings);
    println!("   ❓ Conditionals: {}", stats.conditionals);
    println!("   💎 Literals: {}", stats.literals);
}

#[derive(Default)]
struct ConversionStats {
    total_nodes: usize,
    variable_refs: usize,
    function_calls: usize,
    lambdas: usize,
    let_bindings: usize,
    conditionals: usize,
    literals: usize,
}

fn collect_stats(node: &IrNode, stats: &mut ConversionStats) {
    stats.total_nodes += 1;
    
    match node {
        IrNode::VariableRef { .. } => stats.variable_refs += 1,
        IrNode::Apply { function, arguments, .. } => {
            stats.function_calls += 1;
            collect_stats(function, stats);
            for arg in arguments {
                collect_stats(arg, stats);
            }
        }
        IrNode::Lambda { params, body, .. } => {
            stats.lambdas += 1;
            for param in params {
                collect_stats(param, stats);
            }
            for expr in body {
                collect_stats(expr, stats);
            }
        }
        IrNode::Let { bindings, body, .. } => {
            stats.let_bindings += bindings.len();
            for binding in bindings {
                collect_stats(&binding.pattern, stats);
                collect_stats(&binding.init_expr, stats);
            }
            for expr in body {
                collect_stats(expr, stats);
            }
        }
        IrNode::If { condition, then_branch, else_branch, .. } => {
            stats.conditionals += 1;
            collect_stats(condition, stats);
            collect_stats(then_branch, stats);
            if let Some(else_br) = else_branch {
                collect_stats(else_br, stats);
            }
        }
        IrNode::Do { expressions, .. } => {
            for expr in expressions {
                collect_stats(expr, stats);
            }
        }
        IrNode::Literal { .. } => stats.literals += 1,
        IrNode::LogStep { values, .. } => {
            for value in values {
                collect_stats(value, stats);
            }
        }
        _ => {}
    }
}

/// Analyze type information in the IR
fn print_type_analysis(node: &IrNode) {
    let mut types = std::collections::HashMap::new();
    collect_types(node, &mut types);
    
    println!("   🎯 Type Distribution:");
    for (type_name, count) in types {
        println!("      {} : {} occurrences", type_name, count);
    }
}

fn collect_types(node: &IrNode, types: &mut std::collections::HashMap<String, usize>) {
    if let Some(ir_type) = node.ir_type() {
        let type_name = format!("{:?}", ir_type);
        *types.entry(type_name).or_insert(0) += 1;
    }
    
    // Recursively collect from child nodes
    match node {
        IrNode::Apply { function, arguments, .. } => {
            collect_types(function, types);
            for arg in arguments {
                collect_types(arg, types);
            }
        }
        IrNode::Lambda { params, body, .. } => {
            for param in params {
                collect_types(param, types);
            }
            for expr in body {
                collect_types(expr, types);
            }
        }
        IrNode::Let { bindings, body, .. } => {
            for binding in bindings {
                collect_types(&binding.pattern, types);
                collect_types(&binding.init_expr, types);
            }
            for expr in body {
                collect_types(expr, types);
            }
        }
        IrNode::If { condition, then_branch, else_branch, .. } => {
            collect_types(condition, types);
            collect_types(then_branch, types);
            if let Some(else_br) = else_branch {
                collect_types(else_br, types);
            }
        }
        IrNode::Do { expressions, .. } => {
            for expr in expressions {
                collect_types(expr, types);
            }
        }
        IrNode::LogStep { values, .. } => {
            for value in values {
                collect_types(value, types);
            }
        }
        _ => {}
    }
}

/// Identify optimization opportunities
fn analyze_optimization_opportunities(node: &IrNode) {
    let mut opportunities = Vec::new();
    find_optimization_opportunities(node, &mut opportunities, 0);
    
    if opportunities.is_empty() {
        println!("   ✨ No obvious optimization opportunities found - code looks efficient!");
    } else {
        for opportunity in opportunities {
            println!("   💡 {}", opportunity);
        }
    }
}

fn find_optimization_opportunities(node: &IrNode, opportunities: &mut Vec<String>, depth: usize) {
    match node {
        IrNode::Apply { function, arguments, .. } => {
            // Check for constant folding opportunities
            if let IrNode::VariableRef { name, .. } = function.as_ref() {
                if ["+", "-", "*", "/"].contains(&name.as_str()) {
                    let all_literals = arguments.iter().all(|arg| matches!(arg, IrNode::Literal { .. }));
                    if all_literals {
                        opportunities.push(format!("Constant folding: {} with all literal arguments", name));
                    }
                }
            }
            
            // Recurse into function and arguments
            find_optimization_opportunities(function, opportunities, depth + 1);
            for arg in arguments {
                find_optimization_opportunities(arg, opportunities, depth + 1);
            }
        }
        IrNode::Let { bindings, body, .. } => {
            // Check for unused bindings
            for binding in bindings {
                if let IrNode::VariableBinding { name, .. } = &binding.pattern {
                    if !is_variable_used_in_body(name, body) {
                        opportunities.push(format!("Dead code elimination: unused binding '{}'", name));
                    }
                }
                find_optimization_opportunities(&binding.init_expr, opportunities, depth + 1);
            }
            for expr in body {
                find_optimization_opportunities(expr, opportunities, depth + 1);
            }
        }
        IrNode::Lambda { body, .. } => {
            // Check for tail call optimization opportunities
            if body.len() == 1 {
                if let IrNode::Apply { .. } = &body[0] {
                    opportunities.push("Tail call optimization: single function call in lambda body".to_string());
                }
            }
            for expr in body {
                find_optimization_opportunities(expr, opportunities, depth + 1);
            }
        }
        IrNode::If { condition, then_branch, else_branch, .. } => {
            // Check for constant conditions
            if let IrNode::Literal { .. } = condition.as_ref() {
                opportunities.push("Branch elimination: constant condition in if expression".to_string());
            }
            
            find_optimization_opportunities(condition, opportunities, depth + 1);
            find_optimization_opportunities(then_branch, opportunities, depth + 1);
            if let Some(else_br) = else_branch {
                find_optimization_opportunities(else_br, opportunities, depth + 1);
            }
        }
        _ => {
            // Handle other node types recursively if needed
        }
    }
}

fn is_variable_used_in_body(var_name: &str, body: &[IrNode]) -> bool {
    for node in body {
        if is_variable_used_in_node(var_name, node) {
            return true;
        }
    }
    false
}

fn is_variable_used_in_node(var_name: &str, node: &IrNode) -> bool {
    match node {
        IrNode::VariableRef { name, .. } => name == var_name,
        IrNode::Apply { function, arguments, .. } => {
            is_variable_used_in_node(var_name, function) || 
            arguments.iter().any(|arg| is_variable_used_in_node(var_name, arg))
        }
        IrNode::Let { bindings, body, .. } => {
            bindings.iter().any(|b| is_variable_used_in_node(var_name, &b.init_expr)) ||
            body.iter().any(|expr| is_variable_used_in_node(var_name, expr))
        }
        IrNode::If { condition, then_branch, else_branch, .. } => {
            is_variable_used_in_node(var_name, condition) ||
            is_variable_used_in_node(var_name, then_branch) ||
            else_branch.as_ref().map_or(false, |eb| is_variable_used_in_node(var_name, eb))
        }
        IrNode::Do { expressions, .. } => {
            expressions.iter().any(|expr| is_variable_used_in_node(var_name, expr))
        }
        IrNode::LogStep { values, .. } => {
            values.iter().any(|value| is_variable_used_in_node(var_name, value))
        }
        _ => false,
    }
}

/// Compare original and optimized IR nodes to show improvements
fn compare_ir_nodes(original: &IrNode, optimized: &IrNode) {
    let original_stats = count_ir_node_types(original);
    let optimized_stats = count_ir_node_types(optimized);
    
    println!("   📊 Node Count Comparison:");
    for (node_type, original_count) in &original_stats {
        let optimized_count = optimized_stats.get(node_type).unwrap_or(&0);
        let difference = *original_count as i32 - *optimized_count as i32;
        
        if difference > 0 {
            println!("      {} : {} → {} (reduced by {})", 
                   node_type, original_count, optimized_count, difference);
        } else if difference < 0 {
            println!("      {} : {} → {} (increased by {})", 
                   node_type, original_count, optimized_count, -difference);
        } else {
            println!("      {} : {} (unchanged)", node_type, original_count);
        }
    }
}

/// Count different types of IR nodes for analysis
fn count_ir_node_types(node: &IrNode) -> HashMap<String, usize> {
    let mut counts = HashMap::new();
    count_ir_node_types_recursive(node, &mut counts);
    counts
}

fn count_ir_node_types_recursive(node: &IrNode, counts: &mut HashMap<String, usize>) {
    let node_type = match node {
        IrNode::Literal { .. } => "Literal",
        IrNode::VariableRef { .. } => "VariableRef",
        IrNode::Apply { .. } => "Apply",
        IrNode::If { .. } => "If",
        IrNode::Let { .. } => "Let",
        IrNode::Do { .. } => "Do",
        IrNode::Lambda { .. } => "Lambda",
        IrNode::LogStep { .. } => "LogStep",
        _ => "Other",
    };
    
    *counts.entry(node_type.to_string()).or_insert(0) += 1;
    
    // Recursively count children
    match node {
        IrNode::Apply { function, arguments, .. } => {
            count_ir_node_types_recursive(function, counts);
            for arg in arguments {
                count_ir_node_types_recursive(arg, counts);
            }
        }
        IrNode::If { condition, then_branch, else_branch, .. } => {
            count_ir_node_types_recursive(condition, counts);
            count_ir_node_types_recursive(then_branch, counts);
            if let Some(else_node) = else_branch {
                count_ir_node_types_recursive(else_node, counts);
            }
        }
        IrNode::Let { bindings, body, .. } => {
            for binding in bindings {
                count_ir_node_types_recursive(&binding.pattern, counts);
                count_ir_node_types_recursive(&binding.init_expr, counts);
            }
            for expr in body {
                count_ir_node_types_recursive(expr, counts);
            }
        }
        IrNode::Do { expressions, .. } => {
            for expr in expressions {
                count_ir_node_types_recursive(expr, counts);
            }
        }
        IrNode::Lambda { params, body, .. } => {
            for param in params {
                count_ir_node_types_recursive(param, counts);
            }
            for expr in body {
                count_ir_node_types_recursive(expr, counts);
            }
        }
        IrNode::LogStep { values, .. } => {
            for value in values {
                count_ir_node_types_recursive(value, counts);
            }
        }
        _ => {} // Base cases handled above
    }
}

/// Analyze performance improvements from optimizations
fn analyze_performance_improvements(original: &IrNode, optimized: &IrNode) {
    let original_complexity = estimate_execution_complexity(original);
    let optimized_complexity = estimate_execution_complexity(optimized);
    
    let improvement_ratio = if optimized_complexity > 0 {
        original_complexity as f64 / optimized_complexity as f64
    } else {
        f64::INFINITY
    };
    
    println!("   🏃 Execution Complexity Analysis:");
    println!("      Original complexity:  {} operations", original_complexity);
    println!("      Optimized complexity: {} operations", optimized_complexity);
    
    if improvement_ratio > 1.0 {
        println!("      Performance improvement: {:.2}x faster", improvement_ratio);
    } else if improvement_ratio < 1.0 {
        println!("      Performance regression: {:.2}x slower", 1.0 / improvement_ratio);
    } else {
        println!("      Performance unchanged");
    }
    
    println!("\n   💡 Optimization Benefits:");
    if original_complexity > optimized_complexity {
        println!("      • Reduced computational overhead");
        println!("      • Eliminated dead code paths");
        println!("      • Pre-computed constant expressions");
        println!("      • Simplified control flow");
    } else {
        println!("      • No significant optimizations applied");
        println!("      • Consider more aggressive optimization passes");
    }
}

/// Estimate execution complexity of an IR node tree
fn estimate_execution_complexity(node: &IrNode) -> usize {
    match node {
        IrNode::Literal { .. } => 1,
        IrNode::VariableRef { .. } => 2,
        IrNode::Apply { function, arguments, .. } => {
            10 + estimate_execution_complexity(function) + 
            arguments.iter().map(estimate_execution_complexity).sum::<usize>()
        }
        IrNode::If { condition, then_branch, else_branch, .. } => {
            5 + estimate_execution_complexity(condition) +
            estimate_execution_complexity(then_branch) +
            else_branch.as_ref().map_or(0, |e| estimate_execution_complexity(e))
        }
        IrNode::Let { bindings, body, .. } => {
            bindings.iter().map(|b| estimate_execution_complexity(&b.init_expr)).sum::<usize>() +
            body.iter().map(estimate_execution_complexity).sum::<usize>()
        }
        IrNode::Do { expressions, .. } => {
            expressions.iter().map(estimate_execution_complexity).sum::<usize>()
        }
        IrNode::Lambda { body, .. } => {
            body.iter().map(estimate_execution_complexity).sum::<usize>()
        }
        IrNode::LogStep { values, .. } => {
            15 + values.iter().map(estimate_execution_complexity).sum::<usize>()
        }
        _ => 5, // Default for other node types
    }
}
