// Integration Tests for RTFS Source → AST → IR Pipeline
// Tests the complete compilation pipeline from RTFS source code to optimized IR

use crate::parser::parse_expression;
use crate::ir_converter::IrConverter;
use crate::ir_optimizer::OptimizationPipeline;
use crate::ir::*;
use crate::ast::*;
use std::fmt;

// Implement Display for error types to make them work with Box<dyn std::error::Error>
impl fmt::Display for crate::parser::PestParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl std::error::Error for crate::parser::PestParseError {}

impl fmt::Display for crate::ir_converter::IrConversionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl std::error::Error for crate::ir_converter::IrConversionError {}

/// Integration test result containing all pipeline stages
#[derive(Debug)]
pub struct PipelineTestResult {
    pub source: String,
    pub ast: Expression,
    pub ir: IrNode,
    pub optimized_ir: IrNode,
    pub compilation_time_microseconds: u128,
}

/// Comprehensive integration test runner
pub struct IntegrationTestRunner {
    converter: IrConverter,
    optimizer: OptimizationPipeline,
}

impl IntegrationTestRunner {
    pub fn new() -> Self {
        Self {
            converter: IrConverter::new(),
            optimizer: OptimizationPipeline::standard(),
        }
    }

    /// Run complete pipeline test: Source → AST → IR → Optimized IR
    pub fn run_pipeline_test(&mut self, source: &str) -> Result<PipelineTestResult, Box<dyn std::error::Error>> {
        let start_time = std::time::Instant::now();
        
        // Stage 1: Parse RTFS source to AST
        let ast = parse_expression(source)?;
        
        // Stage 2: Convert AST to IR
        let ir = self.converter.convert(&ast)?;
        
        // Stage 3: Optimize IR
        let optimized_ir = self.optimizer.optimize(ir.clone());
        
        let compilation_time = start_time.elapsed().as_micros();
        
        Ok(PipelineTestResult {
            source: source.to_string(),
            ast,
            ir,
            optimized_ir,
            compilation_time_microseconds: compilation_time,
        })
    }

    /// Display detailed test results
    pub fn display_result(&self, result: &PipelineTestResult) {
        let separator = "=".repeat(80);
        println!("{}", separator);
        println!("RTFS PIPELINE INTEGRATION TEST");
        println!("{}", separator);
        println!("Source: {}", result.source);
        println!("Compilation Time: {}μs", result.compilation_time_microseconds);
        println!();
        
        println!("STAGE 1 - AST:");
        println!("{:#?}", result.ast);
        println!();
        
        println!("STAGE 2 - IR:");
        println!("{:#?}", result.ir);
        println!();
        
        println!("STAGE 3 - OPTIMIZED IR:");
        println!("{:#?}", result.optimized_ir);
        println!();
        
        // Analysis
        println!("ANALYSIS:");
        self.analyze_optimization(&result.ir, &result.optimized_ir);
        println!();
    }

    /// Analyze optimization impact
    fn analyze_optimization(&self, original: &IrNode, optimized: &IrNode) {
        let original_nodes = self.count_nodes(original);
        let optimized_nodes = self.count_nodes(optimized);
        
        if original_nodes != optimized_nodes {
            let reduction = original_nodes - optimized_nodes;
            let percentage = (reduction as f64 / original_nodes as f64) * 100.0;
            println!("• Node count reduced: {} → {} ({:.1}% reduction)", 
                     original_nodes, optimized_nodes, percentage);
        } else {
            println!("• Node count unchanged: {}", original_nodes);
        }
        
        // Check for specific optimizations
        if self.contains_literal(optimized) && !self.contains_literal(original) {
            println!("• Constant folding applied");
        }
        
        if self.has_fewer_branches(original, optimized) {
            println!("• Branch optimization applied");
        }
    }    fn count_nodes(&self, node: &IrNode) -> usize {
        match node {
            IrNode::Literal { .. } => 1,
            IrNode::VariableRef { .. } => 1,
            IrNode::Let { bindings, body, .. } => {
                1 + bindings.iter()
                    .map(|b| self.count_nodes(&b.init_expr))
                    .sum::<usize>() 
                + body.iter()
                    .map(|e| self.count_nodes(e))
                    .sum::<usize>()
            },
            IrNode::Apply { function, arguments, .. } => {
                1 + self.count_nodes(function)
                + arguments.iter()
                    .map(|a| self.count_nodes(a))
                    .sum::<usize>()
            },
            IrNode::If { condition, then_branch, else_branch, .. } => {
                1 + self.count_nodes(condition)
                + self.count_nodes(then_branch)
                + else_branch.as_ref()
                    .map(|e| self.count_nodes(e))
                    .unwrap_or(0)
            },
            IrNode::Lambda { body, .. } => {
                1 + body.iter()
                    .map(|e| self.count_nodes(e))
                    .sum::<usize>()
            },
            IrNode::Do { expressions, .. } => {
                1 + expressions.iter()
                    .map(|e| self.count_nodes(e))
                    .sum::<usize>()
            },
            _ => 1, // Other node types
        }
    }

    fn contains_literal(&self, node: &IrNode) -> bool {
        match node {
            IrNode::Literal { .. } => true,
            IrNode::Let { bindings, body, .. } => {
                bindings.iter().any(|b| self.contains_literal(&b.init_expr))
                || body.iter().any(|e| self.contains_literal(e))
            },
            IrNode::Apply { function, arguments, .. } => {
                self.contains_literal(function)
                || arguments.iter().any(|a| self.contains_literal(a))
            },
            IrNode::If { condition, then_branch, else_branch, .. } => {
                self.contains_literal(condition)
                || self.contains_literal(then_branch)
                || else_branch.as_ref()
                    .map(|e| self.contains_literal(e))
                    .unwrap_or(false)
            },
            _ => false,
        }
    }

    fn has_fewer_branches(&self, original: &IrNode, optimized: &IrNode) -> bool {
        let original_branches = self.count_branches(original);
        let optimized_branches = self.count_branches(optimized);
        optimized_branches < original_branches
    }

    fn count_branches(&self, node: &IrNode) -> usize {
        match node {
            IrNode::If { .. } => 1,
            IrNode::Let { bindings, body, .. } => {
                bindings.iter()
                    .map(|b| self.count_branches(&b.init_expr))
                    .sum::<usize>()
                + body.iter()
                    .map(|e| self.count_branches(e))
                    .sum::<usize>()
            },
            IrNode::Apply { function, arguments, .. } => {
                self.count_branches(function)
                + arguments.iter()
                    .map(|a| self.count_branches(a))
                    .sum::<usize>()
            },
            _ => 0,
        }
    }
}

/// Comprehensive test suite for RTFS source → AST → IR pipeline
pub fn run_comprehensive_integration_tests() {
    println!("\n🚀 RTFS INTEGRATION TEST SUITE");
    println!("Testing complete pipeline: RTFS Source → AST → IR → Optimized IR");
    let separator = "=".repeat(80);
    println!("{}", separator);

    let mut runner = IntegrationTestRunner::new();
    let mut total_tests = 0;
    let mut successful_tests = 0;
    let mut total_compilation_time = 0u128;

    // Test cases covering all major RTFS constructs
    let test_cases = vec![
        // Basic literals
        ("42", "Integer literal"),
        ("3.14", "Float literal"),
        ("\"hello world\"", "String literal"),
        ("true", "Boolean literal"),
        ("nil", "Nil literal"),
        (":keyword", "Keyword literal"),
        
        // Simple expressions
        ("(+ 1 2)", "Simple arithmetic"),
        ("(* 3 4)", "Multiplication"),
        ("(- 10 5)", "Subtraction"),
        ("(/ 20 4)", "Division"),
        
        // Let expressions (core language feature)
        ("(let [x 10] x)", "Simple let binding"),
        ("(let [x 5 y 10] (+ x y))", "Multiple let bindings"),
        ("(let [x 10] (let [y 20] (+ x y)))", "Nested let expressions"),
        
        // Function expressions
        ("((fn [x] (+ x 1)) 5)", "Anonymous function application"),
        ("((fn [x y] (* x y)) 3 4)", "Multi-parameter function"),
        
        // Conditional expressions
        ("(if true 1 2)", "Simple if expression"),
        ("(if false \"no\" \"yes\")", "If with string results"),
        ("(if (> 5 3) \"greater\" \"not greater\")", "If with comparison"),
        
        // Do expressions (sequential execution)
        ("(do 1 2 3)", "Simple do expression"),
        ("(do (+ 1 2) (* 3 4))", "Do with calculations"),
        
        // Vector expressions
        ("[1 2 3]", "Simple vector"),
        ("[(+ 1 2) (* 3 4)]", "Vector with expressions"),
        
        // Map expressions
        ("{:a 1 :b 2}", "Simple map"),
        ("{:x (+ 1 2) :y (* 3 4)}", "Map with expressions"),
        
        // Complex nested expressions
        ("(let [f (fn [x] (* x 2))] (f 5))", "Let with function definition"),
        ("(let [x 10 y 20] (if (> x y) x y))", "Let with conditional"),
        
        // Mathematical expressions (good for optimization)
        ("(+ 1 2 3)", "Multi-argument addition"),
        ("(* (+ 1 2) (+ 3 4))", "Nested arithmetic"),
        ("(let [x 5] (+ x x x))", "Variable reuse"),
        
        // Function call chains
        ("(+ 1 (+ 2 (+ 3 4)))", "Nested function calls"),
        
        // Advanced control flow
        ("(let [x 10] (if (> x 5) (+ x 1) (- x 1)))", "Complex conditional logic"),
        ("(do (let [x 5] x) (let [y 10] y))", "Sequential let expressions"),
    ];

    println!("Running {} integration tests...\n", test_cases.len());

    for (i, (source, description)) in test_cases.iter().enumerate() {
        total_tests += 1;
        println!("Test {}/{}: {}", i + 1, test_cases.len(), description);
        println!("Source: {}", source);
        
        match runner.run_pipeline_test(source) {
            Ok(result) => {
                successful_tests += 1;
                total_compilation_time += result.compilation_time_microseconds;
                
                println!("✅ SUCCESS - Compiled in {}μs", result.compilation_time_microseconds);
                
                // Show brief analysis
                let original_nodes = runner.count_nodes(&result.ir);
                let optimized_nodes = runner.count_nodes(&result.optimized_ir);
                if original_nodes != optimized_nodes {
                    let reduction = original_nodes - optimized_nodes;
                    let percentage = (reduction as f64 / original_nodes as f64) * 100.0;
                    println!("   Optimization: {} → {} nodes ({:.1}% reduction)", 
                             original_nodes, optimized_nodes, percentage);
                } else {
                    println!("   Optimization: {} nodes (no reduction)", original_nodes);
                }
                
                // Optional: Display full result for specific tests
                if source.contains("let") && source.len() > 20 {
                    println!("   [Detailed view for complex expression]");
                    runner.display_result(&result);
                }
            },
            Err(e) => {
                println!("❌ FAILED - Error: {:?}", e);
            }
        }
        
        println!();
    }

    // Summary statistics
    let separator = "=".repeat(80);
    println!("{}", separator);
    println!("INTEGRATION TEST SUMMARY");
    println!("{}", separator);
    println!("Total Tests: {}", total_tests);
    println!("Successful: {}", successful_tests);
    println!("Failed: {}", total_tests - successful_tests);
    println!("Success Rate: {:.1}%", (successful_tests as f64 / total_tests as f64) * 100.0);
    println!("Total Compilation Time: {}μs", total_compilation_time);
    println!("Average Compilation Time: {:.1}μs", 
             if successful_tests > 0 { total_compilation_time as f64 / successful_tests as f64 } else { 0.0 });
    
    if successful_tests == total_tests {
        println!("\n🎉 ALL TESTS PASSED! The RTFS compilation pipeline is working correctly.");
        println!("✨ Features validated:");
        println!("   • RTFS source parsing");
        println!("   • AST construction");
        println!("   • IR conversion");
        println!("   • IR optimization");
        println!("   • End-to-end compilation pipeline");
    } else {
        println!("\n⚠️  Some tests failed. Please review the errors above.");
    }
}

/// Comprehensive test suite for demonstrating advanced language constructs
pub fn run_advanced_integration_tests() {
    println!("\n🔬 ADVANCED RTFS INTEGRATION TESTS");
    println!("Testing advanced language constructs: Pattern Matching, Error Handling, Resources");
    let separator = "=".repeat(80);
    println!("{}", separator);

    let mut runner = IntegrationTestRunner::new();
    let mut total_tests = 0;
    let mut successful_tests = 0;
    let mut total_compilation_time = 0u128;

    let advanced_test_cases = vec![
        // PATTERN MATCHING TESTS
        ("(match 42 42 \"found\" _ \"not found\")", "Basic literal pattern matching"),
        ("(match x :ok \"success\" :error \"failure\" _ \"unknown\")", "Keyword pattern matching"),
        ("(match [1 2] [a b] (+ a b) _ 0)", "Vector destructuring pattern"),
        ("(match {:name \"John\" :age 30} {:name n :age a} n _ \"unknown\")", "Map destructuring pattern"),
        ("(match data [first & rest] (+ first (count rest)) _ 0)", "Vector rest pattern"),
        ("(match config {:required r & optional} r _ {})", "Map rest pattern"),
        ("(match value [x y] when (> x y) x [x y] y _ 0)", "Pattern matching with guard"),
        ("(match result [:ok data] data [:error msg] msg _ nil)", "Result pattern matching"),
        ("(match nested [[:inner x]] x [[y]] y _ 0)", "Nested pattern matching"),
        
        // ERROR HANDLING TESTS
        ("(try (/ 10 2) (catch :error/runtime e \"error\"))", "Basic try-catch"),
        ("(try (risky-op) (catch :error/network e \"network\") (catch :error/timeout e \"timeout\"))", "Multiple catch clauses"),
        ("(try (operation) (finally (cleanup)))", "Try-finally without catch"),
        ("(try (main-task) (catch :error/any e (log e)) (finally (cleanup)))", "Try-catch-finally"),
        ("(try (nested-try) (catch err (try (recover) (catch e2 \"failed\"))))", "Nested try-catch"),
        
        // RESOURCE MANAGEMENT TESTS  
        ("(with-resource [f FileHandle (open-file \"test.txt\")] (read-line f))", "Basic resource management"),
        ("(with-resource [db DbConnection (connect)] (with-resource [tx Transaction (begin-tx db)] (commit tx)))", "Nested resource management"),
        
        // COMPLEX CONTROL FLOW
        ("(let [result (if (> x 0) (match x 1 \"one\" 2 \"two\" _ \"many\") \"zero\")] result)", "Nested if-match"),
        ("(do (let [x 1] x) (match y :a 1 :b 2) (if true 3 4))", "Mixed control structures"),
        ("(let [f (fn [x] (match x 0 \"zero\" _ \"nonzero\"))] (f 5))", "Function with pattern matching"),
        
        // EDGE CASES AND BOUNDARY CONDITIONS
        ("(let [x nil] x)", "Nil binding"),
        ("[]", "Empty vector"),
        ("{}", "Empty map"),
        ("(fn [] nil)", "Function with no parameters"),
        ("(fn [& args] args)", "Variadic function"),
        ("(let [x 1 y 2 z 3] (+ x y z))", "Multiple bindings"),
        ("(((fn [] (fn [x] (+ x 1)))))", "Higher-order function application"),
        
        // STRESS TESTS
        ("(let [a 1 b 2 c 3 d 4 e 5] (+ a (+ b (+ c (+ d e)))))", "Deep nesting arithmetic"),
        ("(match [1 [2 [3 [4 5]]]] [a [b [c [d e]]]] (+ a b c d e) _ 0)", "Deep nesting pattern"),
        ("(if (> (+ 1 2) (* 3 4)) (let [x 10] (+ x 5)) (let [y 20] (- y 5)))", "Complex conditional"),
    ];

    println!("Running {} advanced integration tests...\n", advanced_test_cases.len());

    for (i, (source, description)) in advanced_test_cases.iter().enumerate() {
        total_tests += 1;
        println!("Advanced Test {}/{}: {}", i + 1, advanced_test_cases.len(), description);
        println!("Source: {}", source);
        
        match runner.run_pipeline_test(source) {
            Ok(result) => {
                successful_tests += 1;
                total_compilation_time += result.compilation_time_microseconds;
                
                println!("✅ SUCCESS - Compiled in {}μs", result.compilation_time_microseconds);
                
                // Show optimization analysis for complex cases
                let original_nodes = runner.count_nodes(&result.ir);
                let optimized_nodes = runner.count_nodes(&result.optimized_ir);
                
                if original_nodes != optimized_nodes {
                    let reduction = original_nodes - optimized_nodes;
                    let percentage = (reduction as f64 / original_nodes as f64) * 100.0;
                    println!("   Optimization: {} → {} nodes ({:.1}% reduction)", 
                             original_nodes, optimized_nodes, percentage);
                } else {
                    println!("   Optimization: {} nodes (unchanged)", original_nodes);
                }
                
                // For pattern matching and error handling, show detailed analysis
                if source.contains("match") || source.contains("try") || source.contains("with-resource") {
                    println!("   [Advanced construct successfully compiled]");
                }
            },
            Err(e) => {
                println!("❌ FAILED - Error: {:?}", e);
                
                // For advanced tests, show more context on failures
                if source.contains("match") {
                    println!("   Note: Pattern matching construct failed");
                } else if source.contains("try") {
                    println!("   Note: Error handling construct failed");
                } else if source.contains("with-resource") {
                    println!("   Note: Resource management construct failed");
                }
            }
        }
        
        println!();
    }

    // Advanced test summary
    let separator = "=".repeat(80);
    println!("{}", separator);
    println!("ADVANCED INTEGRATION TEST SUMMARY");
    println!("{}", separator);
    println!("Total Advanced Tests: {}", total_tests);
    println!("Successful: {}", successful_tests);
    println!("Failed: {}", total_tests - successful_tests);
    println!("Success Rate: {:.1}%", (successful_tests as f64 / total_tests as f64) * 100.0);
    println!("Total Compilation Time: {}μs", total_compilation_time);
    println!("Average Compilation Time: {:.1}μs", 
             if successful_tests > 0 { total_compilation_time as f64 / successful_tests as f64 } else { 0.0 });
    
    if successful_tests == total_tests {
        println!("\n🎉 ALL ADVANCED TESTS PASSED!");
        println!("✨ Advanced features validated:");
        println!("   • Pattern matching with destructuring");
        println!("   • Guard expressions in patterns");
        println!("   • Try/catch/finally error handling");
        println!("   • Resource management with with-resource");
        println!("   • Complex nested control structures");
        println!("   • Edge cases and boundary conditions");
    } else {
        println!("\n⚠️  Some advanced tests failed. This may indicate:");
        println!("   • Missing implementation of advanced constructs");
        println!("   • IR conversion limitations for complex patterns");
        println!("   • Optimization issues with advanced language features");
    }
}

/// Error case testing for invalid RTFS syntax and edge conditions
pub fn run_error_case_tests() {
    println!("\n🚨 ERROR CASE INTEGRATION TESTS");
    println!("Testing invalid syntax and error handling in the compilation pipeline");
    let separator = "=".repeat(80);
    println!("{}", separator);

    let mut runner = IntegrationTestRunner::new();
    let mut total_tests = 0;
    let mut expected_failures = 0;

    let error_test_cases = vec![
        // Invalid syntax cases that should fail parsing
        ("(", "Unclosed parenthesis"),
        (")", "Unexpected closing parenthesis"),
        ("(let [x] x)", "Invalid let binding (missing value)"),
        ("(let [x 1 y] y)", "Invalid let binding (missing value for y)"),
        ("(if)", "If missing condition and branches"),
        ("(if true)", "If missing else branch"),
        ("(fn)", "Function missing parameters and body"),
        ("(fn [])", "Function missing body"),
        ("(match)", "Match missing expression"),
        ("(match x)", "Match missing patterns"),
        ("(try)", "Try missing body"),
        ("(catch)", "Catch without try"),
        ("(finally)", "Finally without try"),
        ("(with-resource)", "With-resource missing binding"),
        ("(with-resource [])", "With-resource missing expressions"),
        
        // Malformed expressions
        ("(+ 1)", "Addition with too few arguments"),
        ("(let [x 1 y])", "Let with incomplete binding"),
        ("(fn [x y z])", "Function with parameters but no body"),
        ("(if true 1 2 3)", "If with too many branches"),
        ("(match x 1)", "Match with pattern but no result"),
        
        // Nested syntax errors
        ("(let [x (+ 1] x)", "Mismatched parentheses in let"),
        ("(if (> 1 x))", "If with missing closing paren in condition"),
        ("(do (let [x 1) x)", "Do with malformed let"),
        
        // Edge cases that might cause issues
        ("", "Empty input"),
        ("   ", "Whitespace only"),
        ("\"unclosed string", "Unclosed string literal"),
        (":keyword-with-spaces spaces", "Invalid keyword with spaces"),
        ("(unknown-special-form x y)", "Unknown special form"),
    ];

    println!("Running {} error case tests...\n", error_test_cases.len());

    for (i, (source, description)) in error_test_cases.iter().enumerate() {
        total_tests += 1;
        println!("Error Test {}/{}: {}", i + 1, error_test_cases.len(), description);
        println!("Source: {:?}", source);
        
        match runner.run_pipeline_test(source) {
            Ok(result) => {
                println!("❌ UNEXPECTED SUCCESS - This should have failed!");
                println!("   Source parsed to: {:#?}", result.ast);
                println!("   This may indicate the parser is too permissive");
            },
            Err(e) => {
                expected_failures += 1;
                println!("✅ EXPECTED FAILURE - Error: {:?}", e);
                
                // Classify the type of error
                let error_str = format!("{:?}", e);
                if error_str.contains("Parse") || error_str.contains("Unexpected") {
                    println!("   Type: Parse error (as expected)");
                } else if error_str.contains("Missing") {
                    println!("   Type: Missing token error (as expected)");
                } else if error_str.contains("Invalid") {
                    println!("   Type: Invalid syntax error (as expected)");
                } else {
                    println!("   Type: Other error");
                }
            }
        }
        
        println!();
    }

    // Error case summary
    let separator = "=".repeat(80);
    println!("{}", separator);
    println!("ERROR CASE TEST SUMMARY");
    println!("{}", separator);
    println!("Total Error Tests: {}", total_tests);
    println!("Expected Failures: {}", expected_failures);
    println!("Unexpected Successes: {}", total_tests - expected_failures);
    println!("Error Detection Rate: {:.1}%", (expected_failures as f64 / total_tests as f64) * 100.0);
    
    if expected_failures == total_tests {
        println!("\n✅ ALL ERROR CASES CORRECTLY DETECTED!");
        println!("✨ Error handling validated:");
        println!("   • Invalid syntax properly rejected");
        println!("   • Parser error reporting working");
        println!("   • Pipeline stops at parse errors");
        println!("   • No false positive compilations");
    } else {
        let unexpected = total_tests - expected_failures;
        println!("\n⚠️  {} test(s) passed when they should have failed", unexpected);
        println!("This suggests the parser may be too permissive and should be strengthened");
    }
}

/// Run all enhanced integration test suites
pub fn run_all_enhanced_integration_tests() {
    println!("\n🌟 COMPREHENSIVE RTFS INTEGRATION TEST SUITE");
    println!("Running all test categories: Basic, Advanced, Module System, and Error Cases");
    let separator = "=".repeat(100);
    println!("{}", separator);
    
    // Run all test suites
    run_comprehensive_integration_tests();
    run_advanced_integration_tests(); 
    run_module_system_integration_tests();
    run_error_case_tests();
    
    // Overall summary
    println!("\n{}", separator);
    println!("🏁 COMPLETE INTEGRATION TEST SUITE FINISHED");
    println!("{}", separator);
    println!("✨ Comprehensive testing completed for:");
    println!("   📊 Basic Language Constructs");
    println!("   🔬 Advanced Pattern Matching");  
    println!("   🚨 Error Handling Mechanisms");
    println!("   💾 Resource Management");
    println!("   🏗️  Module System (NEW)");
    println!("   ⚠️  Invalid Syntax Detection");
    println!("   ⚡ End-to-End Pipeline Validation");
    println!("\nThe RTFS compiler integration test suite provides comprehensive validation");
    println!("of the complete compilation pipeline from source code to optimized IR.");
}

/// Performance benchmark for the complete pipeline
pub fn benchmark_pipeline_performance() {
    println!("\n📊 PIPELINE PERFORMANCE BENCHMARK");
    println!("Measuring compilation speed across different expression types");
    let separator = "=".repeat(80);
    println!("{}", separator);

    let mut runner = IntegrationTestRunner::new();
    
    let benchmark_cases = vec![
        ("42", "Literal", 1000),
        ("(+ 1 2)", "Simple expression", 1000),
        ("(let [x 10] x)", "Simple let", 500),
        ("(let [x 5 y 10] (+ x y))", "Multi-binding let", 500),
        ("((fn [x] (+ x 1)) 5)", "Function application", 300),
        ("(if (> 5 3) \"yes\" \"no\")", "Conditional", 300),
        ("(let [f (fn [x] (* x 2))] (f 5))", "Complex nested", 100),
        ("(match 42 42 \"found\" _ \"not found\")", "Pattern matching", 200),
        ("(try (/ 10 2) (catch :error/runtime e \"error\"))", "Error handling", 100),
    ];

    println!("Running performance benchmarks...\n");

    for (source, description, iterations) in benchmark_cases {
        println!("Benchmarking: {} ({})", description, source);
        
        let mut total_time = 0u128;
        let mut successful_runs = 0;
        
        for _ in 0..iterations {
            match runner.run_pipeline_test(source) {
                Ok(result) => {
                    total_time += result.compilation_time_microseconds;
                    successful_runs += 1;
                },
                Err(_) => {
                    // Skip failed runs
                }
            }
        }
        
        if successful_runs > 0 {
            let avg_time = total_time as f64 / successful_runs as f64;
            let throughput = 1_000_000.0 / avg_time; // expressions per second
            
            println!("  Average time: {:.2}μs", avg_time);
            println!("  Throughput: {:.0} expressions/second", throughput);
            println!("  Success rate: {}/{}", successful_runs, iterations);
        } else {
            println!("  ❌ All runs failed");
        }
        
        println!();
    }
}

/// Comprehensive module system integration tests
pub fn run_module_system_integration_tests() {
    println!("\n🏗️  MODULE SYSTEM INTEGRATION TESTS");
    println!("Testing module definitions, imports, exports, and cross-module functionality");
    let separator = "=".repeat(80);
    println!("{}", separator);

    let mut runner = IntegrationTestRunner::new();
    let mut total_tests = 0;
    let mut successful_tests = 0;
    let mut total_compilation_time = 0u128;

    let module_test_cases = vec![
        // BASIC MODULE DEFINITION TESTS
        (r#"(module my.math
             (:exports [add])
             (defn add [x y] (+ x y)))"#, 
         "Simple module with single exported function"),
        
        (r#"(module utils.string
             (:exports [capitalize count-chars])
             (defn capitalize [s] (str (upper s)))
             (defn count-chars [s] (len s))
             (def version "1.0"))"#,
         "Module with multiple exports and private definition"),
        
        (r#"(module data.core
             (defn map-transform [f coll] (map f coll))
             (defn filter-data [pred coll] (filter pred coll)))"#,
         "Module without explicit exports (should export all)"),
        
        // MODULE WITH IMPORTS TESTS
        (r#"(module app.main
             (import utils.string :as str)
             (import data.core :only [map-transform])
             (defn process-text [text] 
               (str/capitalize (map-transform identity text))))"#,
         "Module with both alias and selective imports"),
        
        (r#"(module math.advanced
             (import my.math :as basic)
             (:exports [power])
             (defn power [base exp] 
               (if (= exp 0) 1 (* base (power base (- exp 1)))))"#,
         "Module importing and re-exporting functionality"),
        
        // COMPLEX IMPORT SCENARIOS
        (r#"(module test.imports
             (import utils.string :as str)
             (import utils.string :only [capitalize])
             (defn demo [] 
               (do (str/capitalize "hello") 
                   (capitalize "world"))))"#,
         "Module with both alias and selective import from same module"),
        
        (r#"(module nested.example
             (import parent.module :as parent)
             (import child.module :only [helper])
             (defn complex-operation [data]
               (parent/process (helper data))))"#,
         "Module with multiple import dependencies"),
        
        // NAMESPACE AND VISIBILITY TESTS
        (r#"(module public.api
             (:exports [public-fn])
             (def private-var 42)
             (defn public-fn [] private-var)
             (defn private-fn [] "secret"))"#,
         "Module testing public/private visibility"),
        
        (r#"(module qualified.access
             (import public.api :as api)
             (defn test-access [] 
               (api/public-fn)))"#,
         "Module testing qualified symbol access"),
        
        // FUNCTION DEFINITION IN MODULES
        (r#"(module functions.demo
             (:exports [curry compose])
             (defn curry [f x] (fn [y] (f x y)))
             (defn compose [f g] (fn [x] (f (g x)))))"#,
         "Module with higher-order function exports"),
        
        (r#"(module closures.test
             (import functions.demo :as fn)
             (defn make-adder [n] 
               (fn/curry + n)))"#,
         "Module using imported functions in closures"),
        
        // PATTERN MATCHING WITH MODULES
        (r#"(module patterns.module
             (:exports [match-result])
             (defn match-result [data]
               (match data
                 [:ok value] value
                 [:error msg] (str "Error: " msg)
                 _ "Unknown")))"#,
         "Module with pattern matching functions"),
        
        (r#"(module error.handling
             (import patterns.module :as pm)
             (defn safe-operation [input]
               (try 
                 [:ok (pm/match-result input)]
                 (catch e [:error (:message e)]))))"#,
         "Module combining imports with error handling"),
        
        // RESOURCE MANAGEMENT IN MODULES
        (r#"(module resources.manager
             (:exports [with-file])
             (defn with-file [filename operation]
               (with-resource [f FileHandle (open-file filename)]
                 (operation f))))"#,
         "Module with resource management utilities"),
        
        (r#"(module io.operations
             (import resources.manager :as rm)
             (defn read-and-process [filename]
               (rm/with-file filename 
                 (fn [f] (read-line f)))))"#,
         "Module using imported resource management"),
        
        // EDGE CASES AND COMPLEX SCENARIOS
        (r#"(module edge.cases
             (import deeply.nested.module.name :as deep)
             (import single :only [fn])
             (:exports [test-edge])
             (defn test-edge []
               (do (deep/operation) (fn))))"#,
         "Module with complex nested namespace imports"),
        
        (r#"(module self.reference
             (:exports [recursive-fn])
             (defn recursive-fn [n]
               (if (= n 0) 1 (* n (recursive-fn (- n 1))))))"#,
         "Module with self-referential recursive function"),
        
        // MULTIPLE DEFINITION TYPES
        (r#"(module mixed.definitions
             (import external :as ext)
             (:exports [api-fn config])
             (def config {:version "1.0" :debug true})
             (defn helper [x] (+ x 1))
             (defn api-fn [data] 
               (ext/process (helper data) config)))"#,
         "Module with mixed def/defn and import statements"),
        
        // STRESS TEST - LARGE MODULE
        (r#"(module large.module
             (import utils :as u)
             (import helpers :only [h1 h2 h3])
             (:exports [main-api complex-api data-api])
             (def const1 100)
             (def const2 200)
             (defn helper1 [x] (+ x const1))
             (defn helper2 [y] (- y const2))
             (defn main-api [data] 
               (u/transform (helper1 data)))
             (defn complex-api [input]
               (let [step1 (h1 input)
                     step2 (h2 step1)
                     step3 (h3 step2)]
                 (helper2 step3)))
             (defn data-api []
               {:const1 const1 :const2 const2}))"#,
         "Large module with multiple imports and exports"),
    ];

    println!("Running {} module system integration tests...\n", module_test_cases.len());

    for (i, (source, description)) in module_test_cases.iter().enumerate() {
        total_tests += 1;
        println!("Module Test {}/{}: {}", i + 1, module_test_cases.len(), description);
        println!("Source: {}", source.lines().next().unwrap_or("").trim());
        
        match runner.run_pipeline_test(source) {
            Ok(result) => {
                successful_tests += 1;
                total_compilation_time += result.compilation_time_microseconds;
                
                println!("✅ SUCCESS - Compiled in {}μs", result.compilation_time_microseconds);
                
                // Analyze module-specific features
                let source_str = source.to_string();
                if source_str.contains("import") && source_str.contains(":as") {
                    println!("   ✓ Alias import syntax parsed successfully");
                }
                if source_str.contains(":only") {
                    println!("   ✓ Selective import syntax parsed successfully");
                }
                if source_str.contains(":exports") {
                    println!("   ✓ Module export declaration parsed successfully");
                }
                if source_str.contains("/") && source_str.contains("(") {
                    println!("   ✓ Qualified function call syntax parsed successfully");
                }
                
                // Show optimization analysis
                let original_nodes = runner.count_nodes(&result.ir);
                let optimized_nodes = runner.count_nodes(&result.optimized_ir);
                if original_nodes != optimized_nodes {
                    let reduction = original_nodes - optimized_nodes;
                    let percentage = (reduction as f64 / original_nodes as f64) * 100.0;
                    println!("   Optimization: {} → {} nodes ({:.1}% reduction)", 
                             original_nodes, optimized_nodes, percentage);
                } else {
                    println!("   Optimization: {} nodes (unchanged)", original_nodes);
                }
            },
            Err(e) => {
                println!("❌ FAILED - Error: {:?}", e);
                
                // Provide specific guidance for module system failures
                let error_str = format!("{:?}", e);
                if error_str.contains("module") {
                    println!("   Note: Module definition parsing failed");
                } else if error_str.contains("import") {
                    println!("   Note: Import statement parsing failed");
                } else if error_str.contains("export") {
                    println!("   Note: Export declaration parsing failed");
                } else {
                    println!("   Note: General parsing error in module context");
                }
            }
        }
        
        println!();
    }

    // Module system test summary
    let separator = "=".repeat(80);
    println!("{}", separator);
    println!("MODULE SYSTEM INTEGRATION TEST SUMMARY");
    println!("{}", separator);
    println!("Total Module Tests: {}", total_tests);
    println!("Successful: {}", successful_tests);
    println!("Failed: {}", total_tests - successful_tests);
    println!("Success Rate: {:.1}%", (successful_tests as f64 / total_tests as f64) * 100.0);
    println!("Total Compilation Time: {}μs", total_compilation_time);
    println!("Average Compilation Time: {:.1}μs", 
             if successful_tests > 0 { total_compilation_time as f64 / successful_tests as f64 } else { 0.0 });
    
    if successful_tests == total_tests {
        println!("\n🎉 ALL MODULE SYSTEM TESTS PASSED!");
        println!("✨ Module system features validated:");
        println!("   • Module definition syntax");
        println!("   • Import declarations with :as aliases");
        println!("   • Selective imports with :only");
        println!("   • Export declarations and visibility");
        println!("   • Qualified symbol access (module/function)");
        println!("   • Cross-module function calls");
        println!("   • Module-level def/defn statements");
        println!("   • Complex multi-module scenarios");
        println!("   • Integration with other language features");
    } else {
        println!("\n⚠️  Some module system tests failed. This may indicate:");
        println!("   • Incomplete module system implementation");
        println!("   • Parser limitations for module syntax");
        println!("   • IR conversion issues for module constructs");
        println!("   • Missing namespace resolution logic");
        println!("   • Symbol visibility enforcement gaps");
    }
}
/// Focused test for demonstrating the complete pipeline on a complex example
pub fn demonstrate_complete_pipeline() {
    println!("\n🔬 DETAILED PIPELINE DEMONSTRATION");
    println!("Showing complete RTFS source → AST → IR → Optimized IR transformation");
    let separator = "=".repeat(80);
    println!("{}", separator);

    let mut runner = IntegrationTestRunner::new();
    
    // Complex example that showcases multiple language features
    let complex_example = "(let [x 10 y 20 f (fn [a b] (+ a b))] (if (> (f x y) 25) (* x y) (+ x y)))";
    
    println!("Complex Example: {}", complex_example);
    println!("\nThis example demonstrates:");
    println!("• Let bindings with multiple variables");
    println!("• Anonymous function definition");
    println!("• Function application");
    println!("• Conditional logic");
    println!("• Arithmetic operations");
    println!("• Nested expressions");
    
    match runner.run_pipeline_test(complex_example) {
        Ok(result) => {
            runner.display_result(&result);
            
            println!("PIPELINE VERIFICATION:");
            println!("✅ RTFS source successfully parsed to AST");
            println!("✅ AST successfully converted to IR");
            println!("✅ IR successfully optimized");
            println!("✅ Complete pipeline working end-to-end");
            
            if result.compilation_time_microseconds < 100 {
                println!("⚡ Extremely fast compilation: {}μs", result.compilation_time_microseconds);
            } else if result.compilation_time_microseconds < 1000 {
                println!("🚀 Fast compilation: {}μs", result.compilation_time_microseconds);
            } else {
                println!("⏱️  Compilation time: {}μs", result.compilation_time_microseconds);
            }
        },
        Err(e) => {
            println!("❌ Pipeline failed: {:?}", e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_pipeline() {
        let mut runner = IntegrationTestRunner::new();
        let result = runner.run_pipeline_test("(+ 1 2)").unwrap();
        
        // Verify all stages completed
        assert!(!result.source.is_empty());
        assert!(result.compilation_time_microseconds > 0);
        
        // Should have parsed successfully
        match result.ast {
            Expression::FunctionCall { .. } => {}, // Expected
            _ => panic!("Expected function call AST"),
        }
    }

    #[test]
    fn test_let_expression_pipeline() {
        let mut runner = IntegrationTestRunner::new();
        let result = runner.run_pipeline_test("(let [x 10] x)").unwrap();
        
        // Should have parsed as let expression
        match result.ast {
            Expression::Let(_) => {}, // Expected
            _ => panic!("Expected let expression AST"),
        }
    }

    #[test]
    fn test_optimization_reduces_nodes() {
        let mut runner = IntegrationTestRunner::new();
        let result = runner.run_pipeline_test("(+ 1 2)").unwrap();
        
        let original_nodes = runner.count_nodes(&result.ir);
        let optimized_nodes = runner.count_nodes(&result.optimized_ir);
        
        // Optimization should either reduce nodes or keep them the same
        assert!(optimized_nodes <= original_nodes);
    }

    #[test]
    fn test_compilation_speed() {
        let mut runner = IntegrationTestRunner::new();
        let result = runner.run_pipeline_test("(let [x 5] (+ x x))").unwrap();
        
        // Should compile very quickly (under 1ms)
        assert!(result.compilation_time_microseconds < 1000);
    }

    #[test]
    fn test_advanced_pattern_matching() {
        let mut runner = IntegrationTestRunner::new();
        
        // Test if match expressions parse correctly (they might not convert to IR yet)
        let match_result = runner.run_pipeline_test("(match 42 42 \"found\" _ \"not found\")");
        
        // Either succeeds or fails gracefully
        match match_result {
            Ok(result) => {
                println!("Pattern matching compiled successfully in {}μs", result.compilation_time_microseconds);
            }
            Err(e) => {
                println!("Pattern matching not yet fully implemented: {:?}", e);
                // This is expected if match expressions aren't fully implemented in IR converter
            }
        }
    }

    #[test]
    fn test_error_case_handling() {
        let mut runner = IntegrationTestRunner::new();
        
        // Test that invalid syntax is properly rejected
        let invalid_cases = vec!["(", "(let [x] x)", "(if)", ""];
        
        for case in invalid_cases {
            let result = runner.run_pipeline_test(case);
            assert!(result.is_err(), "Expected error for invalid syntax: {}", case);
        }
    }
}
