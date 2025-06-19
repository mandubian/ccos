use std::env;
use std::fs;
use std::path::Path;
use rtfs_compiler::*;
use rtfs_compiler::ast::MapKey;
use rtfs_compiler::runtime::module_runtime::ModuleRegistry;

/// Test configuration for each RTFS test file
#[derive(Debug, Clone)]
struct TestConfig {
    /// File name (without .rtfs extension)
    name: String,
    /// Expected to pass compilation (true) or fail (false)
    should_compile: bool,
    /// Expected to pass execution (true) or fail (false)
    should_execute: bool,
    /// Runtime to test with
    runtime: RuntimeStrategy,
    /// Expected error pattern if should_compile or should_execute is false
    expected_error: Option<&'static str>,
    /// Optional expected result for verification
    expected_result: Option<&'static str>,
}

#[derive(Debug, Clone, Copy)]
enum RuntimeStrategy {
    Ast,
    Ir,
    Both,
}

impl TestConfig {
    fn new(name: &str) -> Self {
        TestConfig {
            name: name.to_string(),
            should_compile: true,
            should_execute: true,
            runtime: RuntimeStrategy::Both,
            expected_error: None,
            expected_result: None,
        }
    }
}

/// Run a single test file with the given runtime
fn run_test_file(config: &TestConfig, runtime_str: &str) -> Result<String, String> {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let test_file_path = format!("{}/tests/rtfs_files/{}.rtfs", manifest_dir, config.name);
    
    // Check if file exists
    if !Path::new(&test_file_path).exists() {
        return Err(format!("Test file not found: {}", test_file_path));
    }

    // Read the file content
    let content = fs::read_to_string(&test_file_path)
        .map_err(|e| format!("Failed to read file {}: {}", test_file_path, e))?;

    let module_registry = ModuleRegistry::new();
    // The 'run' method handles parsing, so we don't need to parse here.
    let mut runtime = match runtime_str {
        "ast" => runtime::Runtime::with_strategy(runtime::RuntimeStrategy::Ast, &module_registry),
        "ir" => runtime::Runtime::with_strategy(runtime::RuntimeStrategy::Ir, &module_registry),
        _ => unreachable!(),
    };

    // Set task context if needed (for specific tests)
    let task_context = if config.name.starts_with("test_") {
        let mut context_map = std::collections::HashMap::new();
        context_map.insert(MapKey::String("key".to_string()), runtime::Value::String("value".to_string()));
        Some(runtime::Value::Map(context_map))
    } else {
        None
    };

    // Execute the code using the run method
    match runtime.run(&content, task_context) {
        Ok(result) => {
            if !config.should_execute {
                return Err(format!("Execution succeeded unexpectedly. Result: {:?}", result));
            }
            
            let result_str = format!("{:?}", result);
            if let Some(expected) = config.expected_result {
                if result_str != expected {
                    return Err(format!("Result mismatch. Expected: '{}', Got: '{}'", expected, result_str));
                }
            }
            Ok(result_str)
        }
        Err(e) => {
            if config.should_execute {
                return Err(format!("Execution failed unexpectedly: {:?}", e));
            }

            let error_str = format!("{:?}", e);
            if let Some(expected) = config.expected_error {
                if !error_str.contains(expected) {
                    return Err(format!("Error mismatch. Expected to contain: '{}', Got: '{}'", expected, error_str));
                }
            }
            Ok("Expected execution error".to_string())
        }
    }
}

/// Run all tests for a given file configuration
fn run_all_tests_for_file(config: &TestConfig) {
    if matches!(config.runtime, RuntimeStrategy::Ast | RuntimeStrategy::Both) {
        println!("--- Running: {} (AST) ---", config.name);
        match run_test_file(config, "ast") {
            Ok(result) => println!("Result: {}", result),
            Err(e) => panic!("Test failed: {}", e),
        }
    }
    if matches!(config.runtime, RuntimeStrategy::Ir | RuntimeStrategy::Both) {
        println!("--- Running: {} (IR) ---", config.name);
        match run_test_file(config, "ir") {
            Ok(result) => println!("Result: {}", result),
            Err(e) => panic!("Test failed: {}", e),
        }
    }
}

#[test]
fn test_simple_add() {
    run_all_tests_for_file(&TestConfig {
        name: "test_simple_add".to_string(),
        expected_result: Some("Integer(3)"),
        ..TestConfig::new("test_simple_add")
    });
}

#[test]
fn test_simple_let() {
    run_all_tests_for_file(&TestConfig {
        name: "test_simple_let".to_string(),
        expected_result: Some("Integer(10)"),
        ..TestConfig::new("test_simple_let")
    });
}

#[test]
fn test_simple_if() {
    run_all_tests_for_file(&TestConfig {
        name: "test_simple_if".to_string(),
        expected_result: Some("Integer(1)"),
        ..TestConfig::new("test_simple_if")
    });
}

#[test]
fn test_simple_fn() {
    run_all_tests_for_file(&TestConfig {
        name: "test_simple_fn".to_string(),
        expected_result: Some("Integer(25)"),
        ..TestConfig::new("test_simple_fn")
    });
}

#[test]
fn test_simple_pipeline() {
    run_all_tests_for_file(&TestConfig {
        name: "test_simple_pipeline".to_string(),
        expected_result: Some("Integer(6)"),
        ..TestConfig::new("test_simple_pipeline")
    });
}

#[test]
fn test_simple_map() {
    run_all_tests_for_file(&TestConfig {
        name: "test_simple_map".to_string(),
        expected_result: Some("Vector([Integer(2), Integer(3), Integer(4)])"),
        ..TestConfig::new("test_simple_map")
    });
}

#[test]
fn test_simple_reduce() {
    run_all_tests_for_file(&TestConfig {
        name: "test_simple_reduce".to_string(),
        expected_result: Some("Integer(15)"),
        ..TestConfig::new("test_simple_reduce")
    });
}

#[test]
fn test_let_destructuring() {
    run_all_tests_for_file(&TestConfig {
        name: "test_let_destructuring".to_string(),
        expected_result: Some("Integer(3)"),
        ..TestConfig::new("test_let_destructuring")
    });
}

#[test]
fn test_fibonacci_recursive() {
    run_all_tests_for_file(&TestConfig {
        name: "test_fibonacci_recursive".to_string(),
        expected_result: Some("Integer(55)"),
        ..TestConfig::new("test_fibonacci_recursive")
    });
}

#[test]
fn test_file_read() {
    run_all_tests_for_file(&TestConfig {
        name: "test_file_read".to_string(),
        expected_result: Some(r#"String("Hello from test file!")"#),
        ..TestConfig::new("test_file_read")
    });
}

#[test]
fn test_forward_ref() {
    run_all_tests_for_file(&TestConfig {
        name: "test_forward_ref".to_string(),
        expected_result: Some("Integer(10)"),
        ..TestConfig::new("test_forward_ref")
    });
}

#[test]
fn test_task_context() {
    run_all_tests_for_file(&TestConfig {
        name: "test_task_context".to_string(),
        expected_result: Some(r#"String("value")"#),
        ..TestConfig::new("test_task_context")
    });
}

#[test]
fn test_advanced_pipeline() {
    run_all_tests_for_file(&TestConfig {
        name: "test_advanced_pipeline".to_string(),
        expected_result: Some(r#"String("Processed: item1, item2")"#),
        ..TestConfig::new("test_advanced_pipeline")
    });
}

#[test]
fn test_error_handling() {
    run_all_tests_for_file(&TestConfig {
        name: "test_error_handling".to_string(),
        should_execute: false,
        expected_error: Some("DivisionByZero"),
        ..TestConfig::new("test_error_handling")
    });
}

#[test]
fn test_parsing_error() {
    run_all_tests_for_file(&TestConfig {
        name: "test_parsing_error".to_string(),
        should_compile: false,
        should_execute: false,
        expected_error: Some("Pest"), // Expect a pest parsing error
        ..TestConfig::new("test_parsing_error")
    });
}

#[test]
fn test_map_destructuring() {
    run_all_tests_for_file(&TestConfig {
        name: "test_map_destructuring".to_string(),
        expected_result: Some(r#"String("value1")"#),
        ..TestConfig::new("test_map_destructuring")
    });
}

#[test]
fn test_nested_let() {
    run_all_tests_for_file(&TestConfig {
        name: "test_nested_let".to_string(),
        expected_result: Some("Integer(15)"),
        ..TestConfig::new("test_nested_let")
    });
}

#[test]
fn test_variadic_fn() {
    run_all_tests_for_file(&TestConfig {
        name: "test_variadic_fn".to_string(),
        expected_result: Some("Integer(10)"),
        ..TestConfig::new("test_variadic_fn")
    });
}

#[test]
fn test_letrec_simple() {
    run_all_tests_for_file(&TestConfig {
        name: "test_letrec_simple".to_string(),
        expected_result: Some("Integer(120)"),
        ..TestConfig::new("test_letrec_simple")
    });
}

#[test]
fn test_mutual_recursion() {
    run_all_tests_for_file(&TestConfig {
        name: "test_mutual_recursion".to_string(),
        expected_result: Some("Boolean(true)"),
        ..TestConfig::new("test_mutual_recursion")
    });
}

#[test]
fn test_computational_heavy() {
    run_all_tests_for_file(&TestConfig {
        name: "test_computational_heavy".to_string(),
        expected_result: Some("Integer(2432902008176640000)"), // This might need to be adjusted based on machine
        ..TestConfig::new("test_computational_heavy")
    });
}

#[test]
fn test_string_ops() {
    run_all_tests_for_file(&TestConfig {
        name: "test_string_ops".to_string(),
        expected_result: Some(r#"String("HELLO, WORLD!")"#),
        ..TestConfig::new("test_string_ops")
    });
}

#[test]
fn test_vector_ops() {
    run_all_tests_for_file(&TestConfig {
        name: "test_vector_ops".to_string(),
        expected_result: Some("Vector([Integer(1), Integer(2), Integer(3), Integer(4)])"),
        ..TestConfig::new("test_vector_ops")
    });
}

#[test]
fn test_map_ops() {
    run_all_tests_for_file(&TestConfig {
        name: "test_map_ops".to_string(),
        expected_result: None,
        ..TestConfig::new("test_map_ops")
    });
}

#[test]
fn test_get_in() {
    run_all_tests_for_file(&TestConfig {
        name: "test_get_in".to_string(),
        expected_result: Some("Integer(3)"),
        ..TestConfig::new("test_get_in")
    });
}

#[test]
fn test_wildcard_destructuring() {
    run_all_tests_for_file(&TestConfig {
        name: "test_wildcard_destructuring".to_string(),
        expected_result: Some("Integer(3)"),
        ..TestConfig::new("test_wildcard_destructuring")
    });
}

#[test]
fn test_advanced_focused() {
    run_all_tests_for_file(&TestConfig {
        name: "test_advanced_focused".to_string(),
        expected_result: Some("Integer(10)"),
        ..TestConfig::new("test_advanced_focused")
    });
}

// New tests from debug files
#[test]
fn test_vector_literal() {
    run_all_tests_for_file(&TestConfig {
        name: "test_vector_literal".to_string(),
        expected_result: Some("Vector([Integer(1), Integer(2), Integer(3)])"),
        ..TestConfig::new("test_vector_literal")
    });
}

#[test]
fn test_let_vector() {
    run_all_tests_for_file(&TestConfig {
        name: "test_let_vector".to_string(),
        expected_result: Some("Vector([Integer(1), Integer(2), Integer(3)])"),
        ..TestConfig::new("test_let_vector")
    });
}

#[test]
fn test_inline_lambda() {
    run_all_tests_for_file(&TestConfig {
        name: "test_inline_lambda".to_string(),
        expected_result: Some("Integer(9)"),
        ..TestConfig::new("test_inline_lambda")
    });
}

#[test]
fn test_map_square() {
    run_all_tests_for_file(&TestConfig {
        name: "test_map_square".to_string(),
        expected_result: Some("Vector([Integer(1), Integer(4), Integer(9)])"),
        ..TestConfig::new("test_map_square")
    });
}

#[test]
fn test_map_multiple_vectors() {
    run_all_tests_for_file(&TestConfig {
        name: "test_map_multiple_vectors".to_string(),
        expected_result: Some("Vector([Integer(2), Integer(4), Integer(6)])"),
        ..TestConfig::new("test_map_multiple_vectors")
    });
}
