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

    let module_registry = Rc::new(ModuleRegistry::new());

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
    let parsed =
        parse_expression(&content).map_err(|e| format!("Failed to parse content: {:?}", e))?;

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
        task_context: Some(r#"{"key" "value"}"#),
        expected_result: Some(r#"String("value")"#.to_string()),
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
        expected_result: Some("Integer(10)".to_string()),
        runtime: RuntimeStrategy::Both,
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
    use rtfs_compiler::ast::{Keyword, MapKey};
    use rtfs_compiler::runtime::Value;
    use std::collections::HashMap;

    let mut calculations = HashMap::new();
    calculations.insert(
        MapKey::Keyword(Keyword("product".to_string())),
        Value::Integer(6000),
    );
    calculations.insert(
        MapKey::Keyword(Keyword("difference".to_string())),
        Value::Integer(-5940),
    );
    calculations.insert(
        MapKey::Keyword(Keyword("category".to_string())),
        Value::Keyword(Keyword("medium".to_string())),
    );
    calculations.insert(
        MapKey::Keyword(Keyword("sum".to_string())),
        Value::Integer(60),
    );
    calculations.insert(
        MapKey::Keyword(Keyword("bonus".to_string())),
        Value::Integer(-11880),
    );

    let mut expected_map = HashMap::new();
    expected_map.insert(
        MapKey::Keyword(Keyword("performance-score".to_string())),
        Value::Integer(135),
    );
    expected_map.insert(
        MapKey::Keyword(Keyword("calculations".to_string())),
        Value::Map(calculations),
    );
    expected_map.insert(
        MapKey::Keyword(Keyword("efficiency-rating".to_string())),
        Value::Keyword(Keyword("good".to_string())),
    );
    expected_map.insert(
        MapKey::Keyword(Keyword("recursive-sum".to_string())),
        Value::Integer(15),
    );
    expected_map.insert(
        MapKey::Keyword(Keyword("input-values".to_string())),
        Value::Vector(vec![
            Value::Integer(10),
            Value::Integer(20),
            Value::Integer(30),
        ]),
    );

    run_all_tests_for_file(&TestConfig {
        name: "test_computational_heavy".to_string(),
        expected_value: Some(Value::Map(expected_map)),
        ..TestConfig::new("test_computational_heavy")
    });
}

#[test]
fn test_string_ops() {
    run_all_tests_for_file(&TestConfig {
        name: "test_string_ops".to_string(),
        expected_result: Some(r#"String("hello, world!")"#.to_string()),
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
    let mut batch1_results = HashMap::new();
    batch1_results.insert(
        MapKey::Keyword(Keyword("batch-size".to_string())),
        Value::Integer(3),
    );
    batch1_results.insert(
        MapKey::Keyword(Keyword("sum".to_string())),
        Value::Integer(6),
    );
    batch1_results.insert(
        MapKey::Keyword(Keyword("average".to_string())),
        Value::Float(2.0),
    );
    batch1_results.insert(
        MapKey::Keyword(Keyword("max".to_string())),
        Value::Integer(3),
    );
    batch1_results.insert(
        MapKey::Keyword(Keyword("min".to_string())),
        Value::Integer(1),
    );

    let mut batch2_results = HashMap::new();
    batch2_results.insert(
        MapKey::Keyword(Keyword("batch-size".to_string())),
        Value::Integer(3),
    );
    batch2_results.insert(
        MapKey::Keyword(Keyword("sum".to_string())),
        Value::Integer(15),
    );
    batch2_results.insert(
        MapKey::Keyword(Keyword("average".to_string())),
        Value::Float(5.0),
    );
    batch2_results.insert(
        MapKey::Keyword(Keyword("max".to_string())),
        Value::Integer(6),
    );
    batch2_results.insert(
        MapKey::Keyword(Keyword("min".to_string())),
        Value::Integer(4),
    );

    let mut batch3_results = HashMap::new();
    batch3_results.insert(
        MapKey::Keyword(Keyword("batch-size".to_string())),
        Value::Integer(3),
    );
    batch3_results.insert(
        MapKey::Keyword(Keyword("sum".to_string())),
        Value::Integer(24),
    );
    batch3_results.insert(
        MapKey::Keyword(Keyword("average".to_string())),
        Value::Float(8.0),
    );
    batch3_results.insert(
        MapKey::Keyword(Keyword("max".to_string())),
        Value::Integer(9),
    );
    batch3_results.insert(
        MapKey::Keyword(Keyword("min".to_string())),
        Value::Integer(7),
    );

    let mut expected_map = HashMap::new();
    expected_map.insert(
        MapKey::Keyword(Keyword("status".to_string())),
        Value::Keyword(Keyword("success".to_string())),
    );
    expected_map.insert(
        MapKey::Keyword(Keyword("processed-batches".to_string())),
        Value::Integer(3),
    );
    expected_map.insert(
        MapKey::Keyword(Keyword("total-sum".to_string())),
        Value::Integer(45),
    );
    expected_map.insert(
        MapKey::Keyword(Keyword("overall-average".to_string())),
        Value::Float(5.0),
    );
    expected_map.insert(
        MapKey::Keyword(Keyword("batch-results".to_string())),
        Value::Vector(vec![
            Value::Map(batch1_results),
            Value::Map(batch2_results),
            Value::Map(batch3_results),
        ]),
    );

    run_all_tests_for_file(&TestConfig {
        name: "test_advanced_focused".to_string(),
        task_context: Some(
            r#"{:description "Process data with advanced error handling and parallel execution" :input-data [1 2 3 4 5 6 7 8 9 10] :batch-size 3}"#,
        ),
        expected_value: Some(Value::Map(expected_map)),
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
        expected_result: Some("Vector([Integer(2), Integer(4), Integer(6)])".to_string()),
        ..TestConfig::new("test_map_multiple_vectors")
    });
}
