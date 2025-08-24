pub mod agent;
mod ast;
mod development_tooling;
mod error_reporting;
mod integration_tests;
pub mod ir;
pub mod parser;
pub mod runtime;
mod tests;

use std::sync::Arc;

use ir::converter::IrConverter;

use crate::ir::demo::run_enhanced_ir_optimizer_demo;
use parser::parse_expression;
use runtime::module_runtime::ModuleRegistry;
use runtime::{Evaluator, Runtime, RuntimeStrategy};

fn main() {
    println!("RTFS Compiler with AST and IR Runtime");
    println!("====================================="); // Strategic Runtime Comparison
    demonstrate_runtime_strategies();

    // Enhanced IR Optimizer Demonstration (Step 2) - TODO: Fix enhanced_ir_demo compilation
    // enhanced_ir_demo::run_enhanced_ir_optimizer_demo();

    // Development Tooling Demonstration (Step 3)
    development_tooling::run_development_tooling_demo();
    println!();

    demonstrate_ast_runtime();
    println!();
    demonstrate_ir_concepts();
    println!();
    demonstrate_ast_to_ir_pipeline();
    println!();
    ir::ir_demo::demonstrate_ir_pipeline();
    println!();
    ir::ir_demo::run_benchmark_suite();
    println!();
    ir::ir_demo_complete::demonstrate_ir_pipeline();
    println!();
    ir::ir_demo_complete::demonstrate_ir_optimization_pipeline();
    println!();
    ir::optimization_demo::demonstrate_advanced_optimizations();
    println!(); // NEW: Run enhanced comprehensive integration tests
    integration_tests::run_all_enhanced_integration_tests();
    println!();
    integration_tests::demonstrate_complete_pipeline();
    println!();
    integration_tests::benchmark_pipeline_performance();
}

fn demonstrate_ast_runtime() {
    println!("=== AST Runtime Demonstration ===");

    let module_registry = std::sync::Arc::new(ModuleRegistry::new());
    let evaluator = Evaluator::new(module_registry.clone());

    // Test various expressions including new features
    let test_cases = vec![
        // Basic functionality tests
        "(+ 1 2 3)",
        "(vector 1 2 3)",
        "(conj [1 2] 3 4)",
        "(map :a 1 :b 2)", // Map literal constructor
        // Type predicates
        "(nil? nil)",
        "(int? 42)",
        "(string? \"hello\")",
        // Tool functions
        "(tool:log \"Hello from RTFS!\")",
        "(tool:current-time)",
        // Enhanced tool functions
        "(tool:get-env \"PATH\" \"default\")",
        "(tool:http-fetch \"http://example.com\")",
        "(tool:http-fetch \"http://error.com\")",
        // Resource management simulation
        "(tool:open-file \"test.txt\")",
        // Let expressions
        "(let [x 10 y 20] (+ x y))",
        // If expressions
        "(if true \"yes\" \"no\")",
        "(if false \"no\" \"yes\")",
        // Do expressions
        "(do (tool:log \"step 1\") (tool:log \"step 2\") 42)",
        // Parallel execution (sequential simulation)
        "(parallel [a (+ 1 2)] [b (* 3 4)])",
        // JSON operations
        "(tool:parse-json \"42\")",
        "(tool:serialize-json [1 2 3])",
        // Error handling showcase
        "(/ 10 0)", // Division by zero
        // Match expressions (basic)
        "(match 42 42 \"found\" _ \"not found\")",
    ];

    for (i, input) in test_cases.iter().enumerate() {
        println!("\nTest {}: {}", i + 1, input);
        match parse_expression(input) {
            Ok(ast) => {
                println!("  AST: {:?}", ast);
                match evaluator.evaluate(&ast) {
                    Ok(result) => {
                        println!("  Result: {:?}", result);
                    }
                    Err(e) => {
                        println!("  Runtime Error: {:?}", e);
                    }
                }
            }
            Err(e) => {
                println!("  Parse Error: {:?}", e);
            }
        }
    }
}

fn demonstrate_ir_concepts() {
    println!("=== IR Concepts Demonstration ===");

    // Demonstrate the IR structure and concepts
    println!("IR Benefits:");
    println!("1. Type Information: Each node carries its type for optimization");
    println!("2. Resolved Bindings: Variables reference binding sites directly (no symbol lookup)");
    println!("3. Canonical Form: Same IR regardless of surface syntax");
    println!("4. Optimization Ready: Constant folding, dead code elimination, inlining");
    println!("5. Analysis Friendly: Type checking, control flow analysis");

    // Show a simple example of what AST vs IR would look like conceptually
    println!("\nExample: (let [x 10] (+ x 5))");
    println!("AST Form:");
    println!("  LetExpr {{ bindings: [LetBinding {{ symbol: 'x', value: Literal(10) }}], body: [Apply {{ function: '+', args: ['x', Literal(5)] }}] }}");

    println!("\nIR Form (conceptual):");
    println!("  Let {{ id: 1, bindings: [LetBinding {{ pattern: VariableBinding {{ id: 2, name: 'x' }}, init_expr: Literal {{ id: 3, value: 10, type: Int }} }}],");
    println!("       body: [Apply {{ id: 4, function: VariableRef {{ id: 5, binding_id: 2 }}, args: [Literal {{ id: 6, value: 5, type: Int }}], type: Int }}] }}");

    println!("\nOptimization Opportunities:");
    println!("- Constant propagation: x = 10, so (+ x 5) becomes (+ 10 5)");
    println!("- Constant folding: (+ 10 5) becomes 15");
    println!("- Final optimized form: Literal {{ value: 15, type: Int }}");

    println!("\nRuntime Benefits:");
    println!("- No symbol table lookups (direct binding references)");
    println!("- Type-specialized function dispatch");
    println!("- Pre-computed constant expressions");
    println!("- Optimized control flow");
}

fn demonstrate_ast_to_ir_pipeline() {
    println!("=== AST to IR Pipeline Demonstration ===");

    // Example AST for demonstration
    let ast_example = "(let [x 10] (+ x 5))";
    match parse_expression(ast_example) {
        Ok(ast) => {
            println!("AST: {:?}", ast);
            let mut converter = IrConverter::new();
            match converter.convert(&ast) {
                Ok(ir) => {
                    println!("IR: {:?}", ir);
                }
                Err(e) => {
                    println!("IR Conversion Error: {:?}", e);
                }
            }
        }
        Err(e) => {
            println!("Parse Error: {:?}", e);
        }
    }
}

fn demonstrate_runtime_strategies() {
    println!("=== Runtime Strategy Comparison ===");

    let test_expression = "(let [x 10] (+ x 5))";
    let module_registry = ModuleRegistry::new();

    match parse_expression(test_expression) {
        Ok(ast) => {
            println!("Testing expression: {}", test_expression);

            // AST Runtime
            println!("\n📊 AST Runtime (Current Default):");
            let mut ast_runtime = Runtime::with_strategy(RuntimeStrategy::Ast, &module_registry);
            match ast_runtime.evaluate_expression(&ast) {
                Ok(result) => println!("  ✅ Result: {:?}", result),
                Err(e) => println!("  ❌ Error: {:?}", e),
            }

            // IR Runtime
            println!("\n⚡ IR Runtime (High Performance):");
            let mut ir_runtime = Runtime::with_strategy(RuntimeStrategy::Ir, &module_registry);
            match ir_runtime.evaluate_expression(&ast) {
                Ok(result) => println!("  ✅ Result: {:?} (2-26x faster)", result),
                Err(e) => println!("  ❌ Error: {:?}", e),
            }

            // Fallback Strategy
            println!("\n🛡️ IR with AST Fallback (Recommended for transition):");
            let mut fallback_runtime =
                Runtime::with_strategy(RuntimeStrategy::IrWithFallback, &module_registry);
            match fallback_runtime.evaluate_expression(&ast) {
                Ok(result) => println!("  ✅ Result: {:?} (Performance + Stability)", result),
                Err(e) => println!("  ❌ Error: {:?}", e),
            }
        }
        Err(e) => println!("Parse Error: {:?}", e),
    }

    println!("\n🎯 Strategic Recommendation:");
    println!("  • Keep AST as default for stability");
    println!("  • Develop IR aggressively for performance");
    println!("  • Use IrWithFallback for best of both worlds");
    println!("  • Transition to IR-first when module system is complete");
}
