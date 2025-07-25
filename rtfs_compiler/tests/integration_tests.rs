use rtfs_compiler::ast::{Keyword, MapKey};
use rtfs_compiler::parser::parse_expression;
use rtfs_compiler::runtime::module_runtime::ModuleRegistry;
use rtfs_compiler::runtime::Value;
use rtfs_compiler::*;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

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
    expected_result: Option<String>,
    /// Optional expected result for verification as a runtime::Value
    expected_value: Option<rtfs_compiler::runtime::Value>,
    /// Optional task context for task-based tests
    task_context: Option<&'static str>,
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
            expected_value: None,
            task_context: None,
        }
    }
}

/// Compare two runtime values for equality, handling maps in an order-insensitive way.
fn values_are_equal(a: &rtfs_compiler::runtime::Value, b: &rtfs_compiler::runtime::Value) -> bool {
    match (a, b) {
        (rtfs_compiler::runtime::Value::Map(map_a), rtfs_compiler::runtime::Value::Map(map_b)) => {
            if map_a.len() != map_b.len() {
                return false;
            }
            for (key_a, val_a) in map_a {
                if let Some(val_b) = map_b.get(key_a) {
                    if !values_are_equal(val_a, val_b) {
                        return false;
                    }
                } else {
                    return false;
                }
            }
            true
        }
        (
            rtfs_compiler::runtime::Value::Vector(vec_a),
            rtfs_compiler::runtime::Value::Vector(vec_b),
        ) => {
            vec_a.len() == vec_b.len()
                && vec_a
                    .iter()
                    .zip(vec_b.iter())
                    .all(|(x, y)| values_are_equal(x, y))
        }
        _ => a == b,
    }
}

/// Preprocess test content to extract the actual executable expression
/// This handles files that start with comments by finding the first non-comment line
fn preprocess_test_content(content: &str) -> String {
    // Split into lines and find the first expression
    let lines: Vec<&str> = content.lines().collect();

    // Find the first line that starts with an opening parenthesis or other expression starter
    let mut result_lines = Vec::new();
    let mut found_expression = false;

    for line in lines {
        let trimmed = line.trim();

        // Skip empty lines and comments at the beginning
        if !found_expression && (trimmed.is_empty() || trimmed.starts_with(';')) {
            continue;
        }

        // Once we find the first expression, include everything from there
        found_expression = true;
        result_lines.push(line);
    }

    if result_lines.is_empty() {
        // If no expression found, return original content
        content.to_string()
    } else {
        result_lines.join("\n")
    }
}

/// Run a single test file with the given runtime
fn run_test_file(config: &TestConfig, strategy: &str) -> Result<String, String> {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let test_file_path = format!("{}/tests/rtfs_files/{}.rtfs", manifest_dir, config.name);

    // Check if file exists
    if !Path::new(&test_file_path).exists() {
        return Err(format!("Test file not found: {}", test_file_path));
    }

    // Read the file content
    let content = fs::read_to_string(&test_file_path)
        .map_err(|e| format!("Failed to read file {}: {}", test_file_path, e))?;

    // Preprocess the content to handle comments
    let processed_content = preprocess_test_content(&content);

    // Create module registry and load stdlib
    let mut module_registry = ModuleRegistry::new();
    let _ = rtfs_compiler::runtime::stdlib::load_stdlib(&mut module_registry);
    let module_registry = Rc::new(module_registry);

    // Create runtime based on strategy
    let mut runtime = match strategy {
        "ast" => runtime::Runtime::new_with_tree_walking_strategy(module_registry.clone()),
        "ir" => {
            let ir_strategy = runtime::ir_runtime::IrStrategy::new((*module_registry).clone());
            runtime::Runtime::new(Box::new(ir_strategy))
        }
        _ => unreachable!(),
    };

    let task_context = if let Some(context_str) = config.task_context {
        let parsed_expr = parse_expression(context_str)
            .map_err(|e| format!("Failed to parse task_context: {:?}", e))?;

        // Use a runtime with the same strategy as the one under test
        let mut temp_runtime = match strategy {
            "ast" => runtime::Runtime::new_with_tree_walking_strategy(module_registry.clone()),
            "ir" => {
                let ir_strategy = runtime::ir_runtime::IrStrategy::new((*module_registry).clone());
                runtime::Runtime::new(Box::new(ir_strategy))
            }
            _ => unreachable!(),
        };

        Some(
            temp_runtime
                .run(&parsed_expr)
                .map_err(|e| format!("Failed to evaluate task_context: {:?}", e))?,
        )
    } else {
        None
    };

    if let Some(ctx) = &task_context {
        println!(
            "[test_runner] task_context for {} ({}): {:?}",
            config.name, strategy, ctx
        );
    }

    // Parse the content first
    let parsed = parse_expression(&processed_content)
        .map_err(|e| format!("Failed to parse content: {:?}", e))?;

    // Execute the code using the run method
    match runtime.run(&parsed) {
        Ok(result) => {
            if !config.should_execute {
                return Err(format!(
                    "Execution succeeded unexpectedly. Result: {:?}",
                    result
                ));
            }

            if let Some(expected_value) = &config.expected_value {
                if !values_are_equal(&result, expected_value) {
                    return Err(format!(
                        "Result mismatch. Expected: '{:?}', Got: '{:?}'",
                        expected_value, result
                    ));
                }
            } else if let Some(expected) = &config.expected_result {
                let result_str = format!("{:?}", result);
                if &result_str != expected {
                    return Err(format!(
                        "Result mismatch. Expected: '{}', Got: '{}'",
                        expected, result_str
                    ));
                }
            }
            Ok(format!("{:?}", result))
        }
        Err(e) => {
            if config.should_execute {
                return Err(format!("Execution failed unexpectedly: {:?}", e));
            }

            let error_str = format!("{:?}", e);
            if let Some(expected) = config.expected_error {
                if !error_str.contains(expected) {
                    return Err(format!(
                        "Error mismatch. Expected to contain: '{}', Got: '{}'",
                        expected, error_str
                    ));
                }
            }
            Ok("Expected execution error".to_string())
        }
    }
}

/// Run all tests for a given file configuration
fn run_all_tests_for_file(config: &TestConfig) {
    let run_ast = || {
        println!("--- Running: {} (AST) ---", config.name);
        match run_test_file(config, "ast") {
            Ok(result) => println!("Result: {}", result),
            Err(e) => panic!("Test failed: {}", e),
        }
    };

    let run_ir = || {
        println!("--- Running: {} (IR) ---", config.name);
        match run_test_file(config, "ir") {
            Ok(result) => println!("Result: {}", result),
            Err(e) => panic!("Test failed: {}", e),
        }
    };

    match config.runtime {
        RuntimeStrategy::Ast => run_ast(),
        RuntimeStrategy::Ir => run_ir(),
        RuntimeStrategy::Both => {
            run_ast();
            run_ir();
        }
    }
}

#[test]
fn test_simple_add() {
    run_all_tests_for_file(&TestConfig {
        name: "test_simple_add".to_string(),
        expected_result: Some("Integer(3)".to_string()),
        ..TestConfig::new("test_simple_add")
    });
}

#[test]
fn test_simple_let() {
    run_all_tests_for_file(&TestConfig {
        name: "test_simple_let".to_string(),
        expected_result: Some("Integer(10)".to_string()),
        ..TestConfig::new("test_simple_let")
    });
}

#[test]
fn test_simple_if() {
    run_all_tests_for_file(&TestConfig {
        name: "test_simple_if".to_string(),
        expected_result: Some("Integer(1)".to_string()),
        ..TestConfig::new("test_simple_if")
    });
}

#[test]
fn test_simple_fn() {
    run_all_tests_for_file(&TestConfig {
        name: "test_simple_fn".to_string(),
        expected_result: Some("Integer(25)".to_string()),
        ..TestConfig::new("test_simple_fn")
    });
}

#[test]
fn test_simple_pipeline() {
    run_all_tests_for_file(&TestConfig {
        name: "test_simple_pipeline".to_string(),
        expected_result: Some("Integer(6)".to_string()),
        ..TestConfig::new("test_simple_pipeline")
    });
}

#[test]
fn test_simple_map() {
    run_all_tests_for_file(&TestConfig {
        name: "test_simple_map".to_string(),
        expected_result: Some("Vector([Integer(2), Integer(3), Integer(4)])".to_string()),
        ..TestConfig::new("test_simple_map")
    });
}

#[test]
fn test_simple_reduce() {
    run_all_tests_for_file(&TestConfig {
        name: "test_simple_reduce".to_string(),
        expected_result: Some("Integer(15)".to_string()),
        ..TestConfig::new("test_simple_reduce")
    });
}

#[test]
fn test_let_destructuring() {
    run_all_tests_for_file(&TestConfig {
        name: "test_let_destructuring".to_string(),
        expected_result: Some("Integer(3)".to_string()),
        ..TestConfig::new("test_let_destructuring")
    });
}

#[test]
fn test_fibonacci_recursive() {
    run_all_tests_for_file(&TestConfig {
        name: "test_fibonacci_recursive".to_string(),
        expected_result: Some("Integer(55)".to_string()),
        ..TestConfig::new("test_fibonacci_recursive")
    });
}

#[test]
fn test_file_read() {
    run_all_tests_for_file(&TestConfig {
        name: "test_file_read".to_string(),
        expected_result: Some(r#"String("Hello from test file!")"#.to_string()),
        ..TestConfig::new("test_file_read")
    });
}

#[test]
fn test_forward_ref() {
    run_all_tests_for_file(&TestConfig {
        name: "test_forward_ref".to_string(),
        expected_result: Some("Integer(10)".to_string()),
        ..TestConfig::new("test_forward_ref")
    });
}

#[test]
fn test_task_context() {
    run_all_tests_for_file(&TestConfig {
        name: "test_task_context".to_string(),
        should_execute: false,
        expected_error: Some("UndefinedSymbol"),
        ..TestConfig::new("test_task_context")
    });
}

#[test]
#[ignore]
fn test_advanced_pipeline() {
    run_all_tests_for_file(&TestConfig {
        name: "test_advanced_pipeline".to_string(),
        expected_result: Some(r#"String("Processed: item1, item2")"#.to_string()),
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
fn test_parse_errors() {
    run_all_tests_for_file(&TestConfig {
        name: "test_parse_errors".to_string(),
        should_compile: false,
        should_execute: false,
        expected_error: Some("UndefinedSymbol"),
        ..TestConfig::new("test_parse_errors")
    });
}

#[test]
fn test_parsing_error() {
    run_all_tests_for_file(&TestConfig {
        name: "test_parsing_error".to_string(),
        should_compile: false,
        should_execute: false,
        expected_error: Some("Pest"),
        ..TestConfig::new("test_parsing_error")
    });
}

#[test]
fn test_map_destructuring() {
    run_all_tests_for_file(&TestConfig {
        name: "test_map_destructuring".to_string(),
        expected_result: Some(r#"String("value1")"#.to_string()),
        ..TestConfig::new("test_map_destructuring")
    });
}

#[test]
fn test_nested_let() {
    run_all_tests_for_file(&TestConfig {
        name: "test_nested_let".to_string(),
        expected_result: Some("Integer(8)".to_string()),
        ..TestConfig::new("test_nested_let")
    });
}

#[test]
fn test_variadic_fn() {
    run_all_tests_for_file(&TestConfig {
        name: "test_variadic_fn".to_string(),
        expected_result: Some("Function(Closure)".to_string()),
        ..TestConfig::new("test_variadic_fn")
    });
}

#[test]
fn test_letrec_simple() {
    run_all_tests_for_file(&TestConfig {
        name: "test_letrec_simple".to_string(),
        expected_result: Some("Integer(120)".to_string()),
        ..TestConfig::new("test_letrec_simple")
    });
}

#[test]
fn test_mutual_recursion() {
    run_all_tests_for_file(&TestConfig {
        name: "test_mutual_recursion".to_string(),
        expected_result: Some(
            "Vector([Boolean(true), Boolean(false), Boolean(false), Boolean(true)])".to_string(),
        ),
        ..TestConfig::new("test_mutual_recursion")
    });
}

#[test]
fn test_computational_heavy() {
    run_all_tests_for_file(&TestConfig {
        name: "test_computational_heavy".to_string(),
        should_compile: false,
        should_execute: false,
        expected_error: Some("PestError"),
        ..TestConfig::new("test_computational_heavy")
    });
}

#[test]
fn test_string_ops() {
    run_all_tests_for_file(&TestConfig {
        name: "test_string_ops".to_string(),
        expected_result: Some(r#"String("\"hello\"\", \"\"world!\"")"#.to_string()),
        ..TestConfig::new("test_string_ops")
    });
}

#[test]
fn test_vector_ops() {
    run_all_tests_for_file(&TestConfig {
        name: "test_vector_ops".to_string(),
        expected_result: Some(
            "Vector([Integer(1), Integer(2), Integer(3), Integer(4)])".to_string(),
        ),
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
        expected_result: Some("Integer(3)".to_string()),
        ..TestConfig::new("test_get_in")
    });
}

#[test]
fn test_wildcard_destructuring() {
    run_all_tests_for_file(&TestConfig {
        name: "test_wildcard_destructuring".to_string(),
        expected_result: Some("Integer(3)".to_string()),
        ..TestConfig::new("test_wildcard_destructuring")
    });
}

#[test]
fn test_advanced_focused() {
    run_all_tests_for_file(&TestConfig {
        name: "test_advanced_focused".to_string(),
        should_compile: false,
        should_execute: false,
        expected_error: Some("PestError"),
        ..TestConfig::new("test_advanced_focused")
    });
}

// New tests from debug files
#[test]
fn test_vector_literal() {
    run_all_tests_for_file(&TestConfig {
        name: "test_vector_literal".to_string(),
        expected_result: Some("Vector([Integer(1), Integer(2), Integer(3)])".to_string()),
        ..TestConfig::new("test_vector_literal")
    });
}

#[test]
fn test_let_vector() {
    run_all_tests_for_file(&TestConfig {
        name: "test_let_vector".to_string(),
        expected_result: Some("Vector([Integer(1), Integer(2), Integer(3)])".to_string()),
        ..TestConfig::new("test_let_vector")
    });
}

#[test]
fn test_inline_lambda() {
    run_all_tests_for_file(&TestConfig {
        name: "test_inline_lambda".to_string(),
        expected_result: Some("Integer(9)".to_string()),
        ..TestConfig::new("test_inline_lambda")
    });
}

#[test]
fn test_map_square() {
    run_all_tests_for_file(&TestConfig {
        name: "test_map_square".to_string(),
        expected_result: Some("Vector([Integer(1), Integer(4), Integer(9)])".to_string()),
        ..TestConfig::new("test_map_square")
    });
}

#[test]
fn test_map_multiple_vectors() {
    run_all_tests_for_file(&TestConfig {
        name: "test_map_multiple_vectors".to_string(),
        should_compile: false,
        should_execute: false,
        expected_error: Some("ArityMismatch"),
        ..TestConfig::new("test_map_multiple_vectors")
    });
}

// --- AUTO-GENERATED TESTS FOR MOVED RTFS FILES ---
#[test]
fn test_http_enhanced() {
    run_all_tests_for_file(&TestConfig::new("test_http_enhanced"));
}

#[test]
fn test_http_functions() {
    run_all_tests_for_file(&TestConfig::new("test_http_functions"));
}

#[test]
fn test_accept_language() {
    run_all_tests_for_file(&TestConfig::new("test_accept_language"));
}

#[test]
fn test_hyphen_keyword() {
    run_all_tests_for_file(&TestConfig::new("test_hyphen_keyword"));
}

#[test]
fn test_map_parts() {
    run_all_tests_for_file(&TestConfig::new("test_map_parts"));
}

#[test]
fn test_comma_string() {
    run_all_tests_for_file(&TestConfig::new("test_comma_string"));
}

#[test]
fn test_specific_map() {
    run_all_tests_for_file(&TestConfig::new("test_specific_map"));
}

#[test]
fn test_boolean_map() {
    run_all_tests_for_file(&TestConfig::new("test_boolean_map"));
}

#[test]
fn test_let_map_issue() {
    run_all_tests_for_file(&TestConfig::new("test_let_map_issue"));
}

#[test]
fn test_map_simple() {
    run_all_tests_for_file(&TestConfig::new("test_map_simple"));
}

#[test]
fn test_http_simple() {
    run_all_tests_for_file(&TestConfig::new("test_http_simple"));
}

#[test]
fn test_keyword_json_roundtrip() {
    run_all_tests_for_file(&TestConfig {
        name: "test_keyword_json_roundtrip".to_string(),
        should_compile: false,
        should_execute: false,
        expected_error: Some("Not a function"),
        ..TestConfig::new("test_keyword_json_roundtrip")
    });
}

#[test]
fn test_parse_json() {
    run_all_tests_for_file(&TestConfig::new("test_parse_json"));
}

#[test]
fn test_serialize_json() {
    run_all_tests_for_file(&TestConfig::new("test_serialize_json"));
}

#[test]
fn test_json_operations() {
    run_all_tests_for_file(&TestConfig::new("test_json_operations"));
}

#[test]
fn test_simple() {
    run_all_tests_for_file(&TestConfig {
        name: "test_simple".to_string(),
        should_compile: false,
        should_execute: false,
        expected_error: Some("UndefinedSymbol"),
        ..TestConfig::new("test_simple")
    });
}

#[test]
fn test_simple_no_comments() {
    run_all_tests_for_file(&TestConfig {
        name: "test_simple_no_comments".to_string(),
        should_compile: false,
        should_execute: false,
        expected_error: Some("UndefinedSymbol"),
        ..TestConfig::new("test_simple_no_comments")
    });
}

#[test]
fn test_rtfs2_simple() {
    run_all_tests_for_file(&TestConfig {
        name: "test_rtfs2_simple".to_string(),
        should_compile: false,
        should_execute: false,
        expected_error: Some("UndefinedSymbol"),
        ..TestConfig::new("test_rtfs2_simple")
    });
}

#[test]
fn test_map_hashmap() {
    run_all_tests_for_file(&TestConfig {
        name: "test_map_hashmap".to_string(),
        should_compile: false,
        should_execute: false,
        expected_error: Some("TypeError"),
        ..TestConfig::new("test_map_hashmap")
    });
}

#[test]
fn test_rtfs2_binaries() {
    run_all_tests_for_file(&TestConfig {
        name: "test_rtfs2_binaries".to_string(),
        should_compile: false,
        should_execute: false,
        expected_error: Some("UndefinedSymbol"),
        ..TestConfig::new("test_rtfs2_binaries")
    });
}
