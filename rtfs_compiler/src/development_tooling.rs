// RTFS Development Tooling - Step 3 Implementation
// REPL interface, testing framework, and development utilities

use std::collections::HashMap;
use std::io::{self, Write};
use crate::parser::parse_expression;
use crate::runtime::{Runtime, RuntimeStrategy};
use crate::ir_converter::IrConverter;
use crate::enhanced_ir_optimizer::EnhancedOptimizationPipeline;

/// RTFS Read-Eval-Print Loop (REPL) interface
pub struct RtfsRepl {
    runtime: Runtime,
    context: ReplContext,
    history: Vec<String>,
    optimizer: Option<EnhancedOptimizationPipeline>,
}

#[derive(Debug)]
pub struct ReplContext {
    pub variables: HashMap<String, String>,
    pub functions: HashMap<String, String>,
    pub show_ast: bool,
    pub show_ir: bool,
    pub show_optimizations: bool,
    pub runtime_strategy: RuntimeStrategy,
}

impl Default for ReplContext {
    fn default() -> Self {
        Self {
            variables: HashMap::new(),
            functions: HashMap::new(),
            show_ast: false,
            show_ir: false,
            show_optimizations: false,
            runtime_strategy: RuntimeStrategy::Ast,
        }
    }
}

impl RtfsRepl {
    pub fn new() -> Self {
        Self {
            runtime: Runtime::with_strategy(RuntimeStrategy::Ast),
            context: ReplContext::default(),
            history: Vec::new(),
            optimizer: Some(EnhancedOptimizationPipeline::new()),
        }
    }    pub fn with_runtime_strategy(strategy: RuntimeStrategy) -> Self {
        Self {
            runtime: Runtime::with_strategy(strategy.clone()),
            context: ReplContext {
                runtime_strategy: strategy,
                ..Default::default()
            },
            history: Vec::new(),
            optimizer: Some(EnhancedOptimizationPipeline::new()),
        }
    }

    /// Run the REPL interface
    pub fn run(&mut self) -> io::Result<()> {
        println!("🚀 RTFS Development REPL v0.1.0");
        println!("Type :help for commands, :quit to exit");
        println!("Current runtime: {:?}", self.context.runtime_strategy);
        println!();

        loop {
            print!("rtfs> ");
            io::stdout().flush()?;

            let mut input = String::new();
            io::stdin().read_line(&mut input)?;
            let input = input.trim();

            if input.is_empty() {
                continue;
            }

            // Handle REPL commands
            if input.starts_with(':') {
                match self.handle_command(input) {
                    Ok(continue_repl) => {
                        if !continue_repl {
                            break;
                        }
                    }
                    Err(e) => println!("❌ Command error: {}", e),
                }
                continue;
            }

            // Store in history
            self.history.push(input.to_string());

            // Evaluate expression
            self.evaluate_expression(input);
        }

        Ok(())
    }

    fn handle_command(&mut self, command: &str) -> io::Result<bool> {
        match command {
            ":quit" | ":q" => {
                println!("👋 Goodbye!");
                return Ok(false);
            }
            ":help" | ":h" => {
                self.show_help();
            }
            ":history" => {
                self.show_history();
            }
            ":clear" => {
                self.history.clear();
                println!("📝 History cleared");
            }
            ":context" => {
                self.show_context();
            }
            ":ast" => {
                self.context.show_ast = !self.context.show_ast;
                println!("🔍 AST display: {}", if self.context.show_ast { "ON" } else { "OFF" });
            }
            ":ir" => {
                self.context.show_ir = !self.context.show_ir;
                println!("⚡ IR display: {}", if self.context.show_ir { "ON" } else { "OFF" });
            }
            ":opt" => {
                self.context.show_optimizations = !self.context.show_optimizations;
                println!("🚀 Optimization display: {}", if self.context.show_optimizations { "ON" } else { "OFF" });
            }
            ":runtime-ast" => {
                self.context.runtime_strategy = RuntimeStrategy::Ast;
                self.runtime = Runtime::with_strategy(RuntimeStrategy::Ast);
                println!("🔄 Switched to AST runtime");
            }
            ":runtime-ir" => {
                self.context.runtime_strategy = RuntimeStrategy::Ir;
                self.runtime = Runtime::with_strategy(RuntimeStrategy::Ir);
                println!("🔄 Switched to IR runtime");
            }
            ":runtime-fallback" => {
                self.context.runtime_strategy = RuntimeStrategy::IrWithFallback;
                self.runtime = Runtime::with_strategy(RuntimeStrategy::IrWithFallback);
                println!("🔄 Switched to IR with AST fallback runtime");
            }
            ":test" => {
                self.run_test_suite();
            }
            ":bench" => {
                self.run_benchmarks();
            }
            _ => {
                println!("❓ Unknown command: {}", command);
                println!("Type :help for available commands");
            }
        }
        Ok(true)
    }

    fn show_help(&self) {
        println!("📚 RTFS REPL Commands:");
        println!("  :help, :h       - Show this help");
        println!("  :quit, :q       - Exit REPL");
        println!("  :history        - Show command history");
        println!("  :clear          - Clear history");
        println!("  :context        - Show current context");
        println!();
        println!("🔍 Display Options:");
        println!("  :ast            - Toggle AST display");
        println!("  :ir             - Toggle IR display");
        println!("  :opt            - Toggle optimization display");
        println!();
        println!("⚙️ Runtime Options:");
        println!("  :runtime-ast    - Use AST runtime");
        println!("  :runtime-ir     - Use IR runtime");
        println!("  :runtime-fallback - Use IR with AST fallback");
        println!();
        println!("🧪 Testing & Benchmarking:");
        println!("  :test           - Run test suite");
        println!("  :bench          - Run benchmarks");
        println!();
        println!("💡 Examples:");
        println!("  (+ 1 2 3)                  ; Basic arithmetic");
        println!("  (let [x 10] (+ x 5))       ; Let binding");
        println!("  (if true \"yes\" \"no\")      ; Conditional");
        println!("  (vector 1 2 3)             ; Vector creation");
    }

    fn show_history(&self) {
        println!("📜 Command History:");
        for (i, cmd) in self.history.iter().enumerate() {
            println!("  {}: {}", i + 1, cmd);
        }
        if self.history.is_empty() {
            println!("  (empty)");
        }
    }

    fn show_context(&self) {
        println!("🔧 Current Context:");
        println!("  Runtime Strategy: {:?}", self.context.runtime_strategy);
        println!("  Show AST: {}", self.context.show_ast);
        println!("  Show IR: {}", self.context.show_ir);
        println!("  Show Optimizations: {}", self.context.show_optimizations);
        println!("  Variables: {} defined", self.context.variables.len());
        println!("  Functions: {} defined", self.context.functions.len());
        println!("  History entries: {}", self.history.len());
    }

    fn evaluate_expression(&mut self, input: &str) {
        // Parse the expression
        match parse_expression(input) {
            Ok(ast) => {
                if self.context.show_ast {
                    println!("🔍 AST: {:?}", ast);
                }

                // Convert to IR if needed
                if self.context.show_ir || self.context.show_optimizations {
                    let mut converter = IrConverter::new();
                    match converter.convert(&ast) {
                        Ok(ir) => {
                            if self.context.show_ir {
                                println!("⚡ IR: {:?}", ir);
                            }

                            // Apply optimizations if enabled
                            if self.context.show_optimizations {
                                if let Some(optimizer) = &mut self.optimizer {
                                    let optimized = optimizer.optimize(ir.clone());
                                    println!("🚀 Optimized: {:?}", optimized);
                                    println!("📊 Stats: {:?}", optimizer.stats());
                                }
                            }
                        }
                        Err(e) => println!("❌ IR conversion error: {:?}", e),
                    }
                }

                // Evaluate with runtime
                match self.runtime.evaluate_expression(&ast) {
                    Ok(result) => {
                        println!("✅ {:#?}", result);
                    }
                    Err(e) => {
                        println!("❌ Runtime error: {:?}", e);
                    }
                }
            }
            Err(e) => {
                println!("❌ Parse error: {:?}", e);
            }
        }
    }

    fn run_test_suite(&mut self) {
        println!("🧪 Running RTFS Test Suite...");
        
        let test_cases = vec![
            // Basic arithmetic
            ("(+ 1 2 3)", "6"),
            ("(- 10 3)", "7"),
            ("(* 2 3 4)", "24"),
            ("(/ 15 3)", "5"),
            
            // Data structures
            ("(vector 1 2 3)", "[1, 2, 3]"),
            ("(count [1 2 3])", "3"),
            ("(conj [1 2] 3)", "[1, 2, 3]"),
            
            // Conditionals
            ("(if true 1 0)", "1"),
            ("(if false 1 0)", "0"),
            
            // Let bindings
            ("(let [x 5] x)", "5"),
            ("(let [x 5 y 10] (+ x y))", "15"),
            
            // Type predicates
            ("(nil? nil)", "true"),
            ("(int? 42)", "true"),
            ("(string? \"hello\")", "true"),
        ];

        let mut passed = 0;
        let mut failed = 0;

        for (i, (expr, expected)) in test_cases.iter().enumerate() {
            print!("  Test {}: {} ... ", i + 1, expr);
            
            match parse_expression(expr) {
                Ok(ast) => {
                    match self.runtime.evaluate_expression(&ast) {
                        Ok(result) => {
                            let result_str = format!("{:#?}", result);
                            if result_str.contains(expected) {
                                println!("✅ PASS");
                                passed += 1;
                            } else {
                                println!("❌ FAIL (expected: {}, got: {})", expected, result_str);
                                failed += 1;
                            }
                        }
                        Err(e) => {
                            println!("❌ FAIL (runtime error: {:?})", e);
                            failed += 1;
                        }
                    }
                }
                Err(e) => {
                    println!("❌ FAIL (parse error: {:?})", e);
                    failed += 1;
                }
            }
        }

        println!("\n📊 Test Results:");
        println!("  ✅ Passed: {}", passed);
        println!("  ❌ Failed: {}", failed);
        println!("  📈 Success Rate: {:.1}%", (passed as f64 / (passed + failed) as f64) * 100.0);
    }

    fn run_benchmarks(&mut self) {
        println!("⏱️ Running RTFS Benchmarks...");
        
        let benchmark_cases = vec![
            "(+ 1 2)",
            "(let [x 10] (+ x 5))",
            "(if true 42 0)",
            "(vector 1 2 3 4 5)",
            "(count [1 2 3 4 5 6 7 8 9 10])",
        ];

        for (i, expr) in benchmark_cases.iter().enumerate() {
            println!("\n  Benchmark {}: {}", i + 1, expr);
            
            match parse_expression(expr) {
                Ok(ast) => {
                    // Warm up
                    for _ in 0..10 {
                        let _ = self.runtime.evaluate_expression(&ast);
                    }
                    
                    // Benchmark
                    let iterations = 1000;
                    let start = std::time::Instant::now();
                    
                    for _ in 0..iterations {
                        match self.runtime.evaluate_expression(&ast) {
                            Ok(_) => {},
                            Err(e) => {
                                println!("    ❌ Error during benchmark: {:?}", e);
                                break;
                            }
                        }
                    }
                    
                    let duration = start.elapsed();
                    let avg_time = duration / iterations;
                    
                    println!("    ⏱️ {} iterations in {:?}", iterations, duration);
                    println!("    📊 Average: {:?} per evaluation", avg_time);
                    println!("    🚀 Rate: {:.0} evaluations/second", 
                        1_000_000.0 / avg_time.as_micros() as f64 * 1_000_000.0);
                }
                Err(e) => {
                    println!("    ❌ Parse error: {:?}", e);
                }
            }
        }
    }
}

/// Built-in testing framework for RTFS
pub struct RtfsTestFramework {
    tests: Vec<TestCase>,
    runtime: Runtime,
}

#[derive(Debug, Clone)]
pub struct TestCase {
    pub name: String,
    pub description: String,
    pub code: String,
    pub expected: TestExpectation,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone)]
pub enum TestExpectation {
    Success(String),         // Expected successful result
    Error(String),           // Expected error message contains
    ParseError,              // Expected parse error
    RuntimeError,            // Expected runtime error
    Custom(fn(&str) -> bool), // Custom validation function (not serializable)
}

impl RtfsTestFramework {
    pub fn new() -> Self {
        Self {
            tests: Vec::new(),
            runtime: Runtime::with_strategy(RuntimeStrategy::Ast),
        }
    }

    pub fn add_test(&mut self, test: TestCase) {
        self.tests.push(test);
    }

    pub fn add_basic_test(&mut self, name: &str, code: &str, expected: &str) {
        self.tests.push(TestCase {
            name: name.to_string(),
            description: format!("Basic test: {}", code),
            code: code.to_string(),
            expected: TestExpectation::Success(expected.to_string()),
            tags: vec!["basic".to_string()],
        });
    }    pub fn run_all_tests(&mut self) -> TestResults {
        println!("🧪 Running {} tests...", self.tests.len());
        
        let mut results = TestResults {
            total: self.tests.len(),
            passed: 0,
            failed: 0,
            errors: 0,
            failures: Vec::new(),
        };

        // Clone tests to avoid borrowing issues
        let tests = self.tests.clone();
        for (i, test) in tests.iter().enumerate() {
            print!("  [{}] {} ... ", i + 1, test.name);
            
            let result = self.run_single_test(test);
            match result {
                TestResult::Pass => {
                    println!("✅ PASS");
                    results.passed += 1;
                }
                TestResult::Fail(reason) => {
                    println!("❌ FAIL: {}", reason);
                    results.failed += 1;
                    results.failures.push(TestFailure {
                        test_name: test.name.clone(),
                        reason,
                        test_case: test.clone(),
                    });
                }
                TestResult::Error(error) => {
                    println!("💥 ERROR: {}", error);
                    results.errors += 1;
                    results.failures.push(TestFailure {
                        test_name: test.name.clone(),
                        reason: error,
                        test_case: test.clone(),
                    });
                }
            }
        }

        results
    }

    fn run_single_test(&mut self, test: &TestCase) -> TestResult {
        match parse_expression(&test.code) {
            Ok(ast) => {
                match self.runtime.evaluate_expression(&ast) {
                    Ok(result) => {
                        let result_str = format!("{:#?}", result);
                        match &test.expected {
                            TestExpectation::Success(expected) => {
                                if result_str.contains(expected) {
                                    TestResult::Pass
                                } else {
                                    TestResult::Fail(format!("Expected '{}', got '{}'", expected, result_str))
                                }
                            }
                            TestExpectation::Error(_) => {
                                TestResult::Fail("Expected error but got success".to_string())
                            }
                            TestExpectation::ParseError => {
                                TestResult::Fail("Expected parse error but parsing succeeded".to_string())
                            }
                            TestExpectation::RuntimeError => {
                                TestResult::Fail("Expected runtime error but execution succeeded".to_string())
                            }
                            TestExpectation::Custom(_) => {
                                TestResult::Error("Custom expectations not implemented".to_string())
                            }
                        }
                    }
                    Err(e) => {
                        let error_str = format!("{:?}", e);
                        match &test.expected {
                            TestExpectation::Error(expected) => {
                                if error_str.contains(expected) {
                                    TestResult::Pass
                                } else {
                                    TestResult::Fail(format!("Expected error containing '{}', got '{}'", expected, error_str))
                                }
                            }
                            TestExpectation::RuntimeError => {
                                TestResult::Pass
                            }
                            _ => {
                                TestResult::Fail(format!("Unexpected runtime error: {}", error_str))
                            }
                        }
                    }
                }
            }
            Err(e) => {
                match &test.expected {
                    TestExpectation::ParseError => {
                        TestResult::Pass
                    }
                    _ => {
                        TestResult::Error(format!("Parse error: {:?}", e))
                    }
                }
            }
        }
    }

    pub fn run_tests_with_tag(&mut self, tag: &str) -> TestResults {
        let filtered_tests: Vec<_> = self.tests.iter()
            .filter(|test| test.tags.contains(&tag.to_string()))
            .collect();
        
        println!("🧪 Running {} tests with tag '{}'...", filtered_tests.len(), tag);
        
        // Create temporary test framework with filtered tests
        let mut temp_framework = RtfsTestFramework::new();
        for test in filtered_tests {
            temp_framework.add_test(test.clone());
        }
        
        temp_framework.run_all_tests()
    }
}

#[derive(Debug)]
pub enum TestResult {
    Pass,
    Fail(String),
    Error(String),
}

#[derive(Debug)]
pub struct TestResults {
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub errors: usize,
    pub failures: Vec<TestFailure>,
}

#[derive(Debug)]
pub struct TestFailure {
    pub test_name: String,
    pub reason: String,
    pub test_case: TestCase,
}

impl TestResults {
    pub fn success_rate(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            self.passed as f64 / self.total as f64 * 100.0
        }
    }

    pub fn print_summary(&self) {
        println!("\n📊 Test Summary:");
        println!("  📝 Total: {}", self.total);
        println!("  ✅ Passed: {}", self.passed);
        println!("  ❌ Failed: {}", self.failed);
        println!("  💥 Errors: {}", self.errors);
        println!("  📈 Success Rate: {:.1}%", self.success_rate());
        
        if !self.failures.is_empty() {
            println!("\n🔍 Failure Details:");
            for failure in &self.failures {
                println!("  ❌ {}: {}", failure.test_name, failure.reason);
                println!("     Code: {}", failure.test_case.code);
            }
        }
    }
}

/// Run the development tooling demonstration
pub fn run_development_tooling_demo() {
    println!("\n=== RTFS Development Tooling Demo - Step 3 ===");
    println!("Demonstrating REPL interface and testing framework:\n");
    
    // Test Framework Demo
    println!("🧪 Testing Framework Demo:");
    demo_testing_framework();
    
    // REPL Demo (non-interactive)
    println!("\n💻 REPL Interface Demo:");
    demo_repl_interface();
    
    println!("\n✅ Development Tooling Demo (Step 3) Complete!");
    println!("   - REPL interface implemented with commands");
    println!("   - Built-in testing framework functional");
    println!("   - Benchmarking capabilities added");
    println!("   - Interactive development environment ready");
}

fn demo_testing_framework() {
    let mut framework = RtfsTestFramework::new();
    
    // Add comprehensive test suite
    framework.add_basic_test("arithmetic_add", "(+ 1 2 3)", "6");
    framework.add_basic_test("arithmetic_multiply", "(* 2 3 4)", "24");
    framework.add_basic_test("vector_create", "(vector 1 2 3)", "Vector");
    framework.add_basic_test("let_binding", "(let [x 5] x)", "5");
    framework.add_basic_test("conditional_true", "(if true 1 0)", "1");
    framework.add_basic_test("conditional_false", "(if false 1 0)", "0");
    
    // Add error test cases
    framework.add_test(TestCase {
        name: "division_by_zero".to_string(),
        description: "Test division by zero error handling".to_string(),
        code: "(/ 1 0)".to_string(),
        expected: TestExpectation::RuntimeError,
        tags: vec!["error".to_string(), "arithmetic".to_string()],
    });
    
    // Run all tests
    let results = framework.run_all_tests();
    results.print_summary();
    
    // Demo tagged test runs
    println!("\n🏷️ Running tests with 'basic' tag:");
    let basic_results = framework.run_tests_with_tag("basic");
    println!("  Basic tests success rate: {:.1}%", basic_results.success_rate());
}

fn demo_repl_interface() {
    println!("   REPL commands available:");
    println!("   - :help          Show help");
    println!("   - :ast           Toggle AST display");
    println!("   - :ir            Toggle IR display");
    println!("   - :opt           Toggle optimization display");
    println!("   - :runtime-ast   Switch to AST runtime");
    println!("   - :runtime-ir    Switch to IR runtime");
    println!("   - :test          Run test suite");
    println!("   - :bench         Run benchmarks");
    println!("   - :quit          Exit REPL");
    
    println!("\n   Example session:");
    println!("   rtfs> (+ 1 2 3)");
    println!("   ✅ Integer(6)");
    println!("   rtfs> :ast");
    println!("   🔍 AST display: ON");
    println!("   rtfs> (let [x 5] x)");
    println!("   🔍 AST: LetExpr {{ ... }}");
    println!("   ✅ Integer(5)");
    
    println!("\n   To start interactive REPL, use:");
    println!("   RtfsRepl::new().run()");
}
