// RTFS Runtime Error Handling Tests
// Tests for runtime error detection and handling

use rtfs::parser::parse_expression;
use rtfs::runtime::evaluator::Evaluator;
use rtfs::runtime::module_runtime::ModuleRegistry;
use rtfs::runtime::pure_host::create_pure_host;
use rtfs::runtime::secure_stdlib::SecureStandardLibrary;
use std::sync::Arc;

/// Test runner for runtime error tests
struct RuntimeErrorTestRunner {
    evaluator: Evaluator,
    env: rtfs::runtime::environment::Environment,
}

impl RuntimeErrorTestRunner {
    fn new() -> Self {
        let env = SecureStandardLibrary::create_secure_environment();
        let module_registry = Arc::new(ModuleRegistry::new());
        let security_context = rtfs::runtime::security::RuntimeContext::pure();
        let host = create_pure_host();
        let evaluator = Evaluator::new(
            module_registry,
            security_context,
            host,
            rtfs::compiler::expander::MacroExpander::default(),
        );

        Self { evaluator, env }
    }

    fn run_error_test(
        &mut self,
        source: &str,
        expected_error_contains: &str,
    ) -> Result<(), String> {
        let ast = parse_expression(source).map_err(|e| format!("Parse error: {:?}", e))?;

        match self.evaluator.evaluate_with_env(&ast, &mut self.env) {
            Ok(result) => Err(format!(
                "Expected error containing '{}', but got success: {:?}",
                expected_error_contains, result
            )),
            Err(error) => {
                let error_string = format!("{:?}", error);
                if error_string.contains(expected_error_contains) {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected error containing '{}', but got: {}",
                        expected_error_contains, error_string
                    ))
                }
            }
        }
    }

    fn run_success_test(
        &mut self,
        source: &str,
        expected_value: rtfs::runtime::values::Value,
    ) -> Result<(), String> {
        let ast = parse_expression(source).map_err(|e| format!("Parse error: {:?}", e))?;
        let outcome = self
            .evaluator
            .evaluate_with_env(&ast, &mut self.env)
            .map_err(|e| format!("Evaluation error: {:?}", e))?;
        match outcome {
            rtfs::runtime::execution_outcome::ExecutionOutcome::Complete(value) => {
                if value == expected_value {
                    Ok(())
                } else {
                    Err(format!("Expected {:?}, got {:?}", expected_value, value))
                }
            }
            other => Err(format!("Expected complete outcome but got {:?}", other)),
        }
    }

    fn run_division_by_zero_test(&mut self) -> Result<(), String> {
        println!("Testing division by zero...");

        // Test simple division by zero
        self.run_error_test("(/ 1 0)", "DivisionByZero")?;

        // Test division by zero in expression
        self.run_error_test("(/ (+ 5 3) 0)", "DivisionByZero")?;

        // Test division by zero with floats
        self.run_error_test("(/ 1.0 0.0)", "DivisionByZero")?;

        println!("✓ Division by zero tests passed!");
        Ok(())
    }

    fn run_undefined_symbol_tests(&mut self) -> Result<(), String> {
        println!("Testing undefined symbol errors...");

        // Test simple undefined symbol
        self.run_error_test("undefined-symbol", "UndefinedSymbol")?;

        // Test undefined symbol in expression
        self.run_error_test("(+ x 1)", "UndefinedSymbol")?;

        // Test undefined symbol in function call
        self.run_error_test("(undefined-function 1 2)", "UndefinedSymbol")?;

        println!("✓ Undefined symbol tests passed!");
        Ok(())
    }

    fn run_type_error_tests(&mut self) -> Result<(), String> {
        println!("Testing type mismatch errors...");

        // Test arithmetic type mismatch
        self.run_error_test("(+ 1 \"hello\")", "TypeError")?;

        // Test collection access type mismatch
        self.run_error_test("(get [1 2 3] \"key\")", "TypeError")?;

        // Test function call type mismatch
        self.run_error_test("(+ true false)", "TypeError")?;

        // Test comparison type mismatch
        self.run_error_test("(> \"hello\" 5)", "TypeError")?;

        println!("✓ Type error tests passed!");
        Ok(())
    }

    fn run_index_out_of_bounds_tests(&mut self) -> Result<(), String> {
        println!("Testing index out of bounds errors...");

        // RTFS design: safe-by-default collection access returns nil for out-of-bounds.
        self.run_success_test("(get [1 2 3] 5)", rtfs::runtime::values::Value::Nil)?;
        self.run_success_test("(get [1 2 3] -1)", rtfs::runtime::values::Value::Nil)?;
        // Strings are not indexable via `get` in RTFS; this is a type error.
        self.run_error_test("(get \"hello\" 10)", "TypeError")?;
        self.run_success_test("(get [] 0)", rtfs::runtime::values::Value::Nil)?;

        println!("✓ Index out of bounds tests passed!");
        Ok(())
    }

    fn run_arity_mismatch_tests(&mut self) -> Result<(), String> {
        println!("Testing arity mismatch errors...");

        // Test too few arguments
        self.run_error_test("(+)", "ArityMismatch")?;

        // Test too many arguments for some functions
        // Note: Some functions like + can take variable arguments, so we need specific cases
        self.run_error_test("(inc 1 2)", "ArityMismatch")?;

        println!("✓ Arity mismatch tests passed!");
        Ok(())
    }

    fn run_key_not_found_tests(&mut self) -> Result<(), String> {
        println!("Testing key not found errors...");

        // RTFS design: safe-by-default map access returns nil for missing keys.
        self.run_success_test("(get {:a 1} :b)", rtfs::runtime::values::Value::Nil)?;
        self.run_success_test(
            "(get (get {:a {:b 1}} :a) :c)",
            rtfs::runtime::values::Value::Nil,
        )?;

        println!("✓ Key not found tests passed!");
        Ok(())
    }

    fn run_resource_error_tests(&mut self) -> Result<(), String> {
        println!("Testing resource errors...");

        // Resource refs resolve via host context; in pure host they fall back to a symbolic "@..." string.
        self.run_success_test(
            "(resource:ref \"invalid://uri\")",
            rtfs::runtime::values::Value::String("@invalid://uri".to_string()),
        )?;

        println!("✓ Resource error tests passed!");
        Ok(())
    }

    fn run_complex_error_scenarios(&mut self) -> Result<(), String> {
        println!("Testing complex error scenarios...");

        // Test nested errors
        self.run_error_test("(let [x (/ 1 0)] (+ x 1))", "DivisionByZero")?;

        // Test error in function application
        self.run_error_test("((fn [x] (/ x 0)) 5)", "DivisionByZero")?;

        // Test error in conditional
        self.run_error_test("(if true (/ 1 0) 42)", "DivisionByZero")?;

        println!("✓ Complex error scenario tests passed!");
        Ok(())
    }

    fn run_error_recovery_tests(&mut self) -> Result<(), String> {
        println!("Testing error recovery mechanisms...");

        // Test try-catch with division by zero
        let source = "(try (/ 1 0) (catch DivisionByZero e 42))";

        self.run_success_test(source, rtfs::runtime::values::Value::Integer(42))?;

        // Test try-catch with undefined symbol
        let source = "(try undefined-symbol (catch UndefinedSymbol e \"recovered\"))";
        self.run_success_test(
            source,
            rtfs::runtime::values::Value::String("recovered".to_string()),
        )?;

        println!("✓ Error recovery tests passed!");
        Ok(())
    }

    fn run_error_propagation_tests(&mut self) -> Result<(), String> {
        println!("Testing error propagation...");

        // Test error propagation through function calls
        let source = "(do (defn failing-function [] (/ 1 0)) (failing-function))";

        self.run_error_test(source, "DivisionByZero")?;

        // Test error propagation through nested calls
        let source = "(do (defn failing-function [] (/ 1 0)) (defn outer [] (let [result (failing-function)] (+ result 1))) (outer))";

        self.run_error_test(source, "DivisionByZero")?;

        println!("✓ Error propagation tests passed!");
        Ok(())
    }
}

/// Test division by zero errors
#[test]
fn test_division_by_zero_errors() {
    let mut runner = RuntimeErrorTestRunner::new();
    runner
        .run_division_by_zero_test()
        .expect("division by zero tests");
}

/// Test undefined symbol errors
#[test]
fn test_undefined_symbol_errors() {
    let mut runner = RuntimeErrorTestRunner::new();
    runner
        .run_undefined_symbol_tests()
        .expect("undefined symbol tests");
}

/// Test type mismatch errors
#[test]
fn test_type_mismatch_errors() {
    let mut runner = RuntimeErrorTestRunner::new();
    runner.run_type_error_tests().expect("type mismatch tests");
}

/// Test index out of bounds errors
#[test]
fn test_index_out_of_bounds_errors() {
    let mut runner = RuntimeErrorTestRunner::new();
    runner
        .run_index_out_of_bounds_tests()
        .expect("index out of bounds tests");
}

/// Test arity mismatch errors
#[test]
fn test_arity_mismatch_errors() {
    let mut runner = RuntimeErrorTestRunner::new();
    runner
        .run_arity_mismatch_tests()
        .expect("arity mismatch tests");
}

/// Test key not found errors
#[test]
fn test_key_not_found_errors() {
    let mut runner = RuntimeErrorTestRunner::new();
    runner
        .run_key_not_found_tests()
        .expect("key not found tests");
}

/// Test resource errors
#[test]
fn test_resource_errors() {
    let mut runner = RuntimeErrorTestRunner::new();
    runner
        .run_resource_error_tests()
        .expect("resource reference tests");
}

/// Test complex error scenarios
#[test]
fn test_complex_error_scenarios() {
    let mut runner = RuntimeErrorTestRunner::new();
    runner
        .run_complex_error_scenarios()
        .expect("complex error scenarios");
}

/// Test error recovery mechanisms
#[test]
fn test_error_recovery() {
    let mut runner = RuntimeErrorTestRunner::new();
    runner
        .run_error_recovery_tests()
        .expect("error recovery tests");
}

/// Test error propagation
#[test]
fn test_error_propagation() {
    let mut runner = RuntimeErrorTestRunner::new();
    runner
        .run_error_propagation_tests()
        .expect("error propagation tests");
}

/// Test all runtime errors
#[test]
fn test_all_runtime_errors() {
    let mut runner = RuntimeErrorTestRunner::new();

    runner
        .run_division_by_zero_test()
        .expect("division by zero");
    runner
        .run_undefined_symbol_tests()
        .expect("undefined symbols");
    runner.run_type_error_tests().expect("type errors");
    runner
        .run_index_out_of_bounds_tests()
        .expect("index out of bounds");
    runner.run_arity_mismatch_tests().expect("arity mismatch");
    runner.run_key_not_found_tests().expect("key not found");
    runner.run_resource_error_tests().expect("resource refs");
    runner
        .run_complex_error_scenarios()
        .expect("complex scenarios");
    runner.run_error_recovery_tests().expect("recovery");
    runner.run_error_propagation_tests().expect("propagation");

    println!("✓ All runtime error tests passed!");
}
