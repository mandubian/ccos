use rtfs_compiler::parser::parse_expression;
use rtfs_compiler::runtime::module_runtime::ModuleRegistry;
use rtfs_compiler::*;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

use crate::test_helpers::*;

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
    let test_file_path = format!("{}/tests/shared/rtfs_files/{}.rtfs", manifest_dir, config.name);

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
    
    // Debug: Check if stdlib module exists
    if let Some(stdlib_module) = module_registry.get_module("stdlib") {
        println!("DEBUG: stdlib module found with {} exports", stdlib_module.exports.read().unwrap().len());
        for (name, export) in stdlib_module.exports.read().unwrap().iter() {
            if name == "*" {
                println!("DEBUG: * function found: {:?}", export.value);
            }
        }
    } else {
        println!("DEBUG: stdlib module not found!");
    }
    
    let module_registry = std::sync::Arc::new(module_registry);

    // Create runtime based on strategy
    let mut runtime = match strategy {
    "ast" => runtime::Runtime::new_with_tree_walking_strategy(module_registry.clone()),
        "ir" => {
            let ir_strategy = runtime::ir_runtime::IrStrategy::new((*module_registry).clone());
            
            // Debug: Check if the environment has the * function
            let mut debug_env = rtfs_compiler::runtime::environment::IrEnvironment::with_stdlib(&module_registry).unwrap();
            if let Some(value) = debug_env.get("*") {
                println!("DEBUG: * function found in environment: {:?}", value);
                match &value {
                    rtfs_compiler::runtime::values::Value::Function(func) => {
                        match func {
                            rtfs_compiler::runtime::values::Function::Builtin(builtin) => {
                                println!("DEBUG: * function is a Builtin function: {}", builtin.name);
                            },
                            rtfs_compiler::runtime::values::Function::Ir(_) => {
                                println!("DEBUG: * function is an IR function (this is the problem!)");
                            },
                            _ => {
                                println!("DEBUG: * function is some other type: {:?}", func);
                            }
                        }
                    },
                    _ => {
                        println!("DEBUG: * function is not a function: {:?}", value);
                    }
                }
            } else {
                println!("DEBUG: * function NOT found in environment!");
            }
            
            // Debug: Check what functions are available in the environment
            println!("DEBUG: Available functions in environment: {:?}", debug_env.binding_names());
            
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
    let parsed = match parse_expression(&processed_content) {
        Ok(parsed) => parsed,
        Err(e) => {
            // Handle parse errors for tests that should fail to compile
            if !config.should_compile {
                let error_str = format!("Failed to parse content: {:?}", e);
                if let Some(expected) = config.expected_error {
                    if !error_str.contains(expected) {
                        return Err(format!(
                            "Parse error mismatch. Expected to contain: '{}', Got: '{}'",
                            expected, error_str
                        ));
                    }
                }
                return Ok("Expected parse error".to_string());
            } else {
                return Err(format!("Failed to parse content: {:?}", e));
            }
        }
    };

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
        expected_result: Some("Keyword(Keyword(\"then-value\"))".to_string()),
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
fn test_unknown_escape() {
    run_all_tests_for_file(&TestConfig {
        name: "test_unknown_escape".to_string(),
        should_compile: false,
        should_execute: false,
    // Expect a parser/unescape error; we now surface InvalidEscapeSequence from the parser
    expected_error: Some("InvalidEscapeSequence"),
        ..TestConfig::new("test_unknown_escape")
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
    run_all_tests_for_file(&TestConfig::new("test_computational_heavy"));
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
fn test_map_creation() {
    use rtfs_compiler::ast::{Keyword, MapKey};
    use rtfs_compiler::runtime::Value;
    use std::collections::HashMap;
    
    let mut expected_map = HashMap::new();
    expected_map.insert(MapKey::Keyword(Keyword::new("category")), Value::Keyword(Keyword::new("medium")));
    expected_map.insert(MapKey::Keyword(Keyword::new("sum")), Value::Integer(60));
    
    run_all_tests_for_file(&TestConfig {
        name: "test_map_creation".to_string(),
        expected_value: Some(Value::Map(expected_map)),
        ..TestConfig::new("test_map_creation")
    });
}



#[test]
fn test_if_map_return() {
    run_all_tests_for_file(&TestConfig {
        name: "test_if_map_return".to_string(),
        expected_result: Some("Integer(60)".to_string()),
        ..TestConfig::new("test_if_map_return")
    });
}


#[test]
fn test_simple_if_keyword() {
    run_all_tests_for_file(&TestConfig {
        name: "test_simple_if".to_string(),
        expected_result: Some("Keyword(Keyword(\"then-value\"))".to_string()),
        ..TestConfig::new("test_simple_if")
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

#[ignore] // RTFS 1.0 task syntax - needs conversion to RTFS 2.0/CCOS
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
    run_all_tests_for_file(&TestConfig::new("test_keyword_json_roundtrip"));
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
    run_all_tests_for_file(&TestConfig::new("test_simple"));
}

#[test]
fn test_simple_no_comments() {
    run_all_tests_for_file(&TestConfig::new("test_simple_no_comments"));
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

#[test]
fn test_orchestration_primitives() {
    // Orchestration primitives require CCOS integration
    // Use CCOS environment instead of basic runtime
    use rtfs_compiler::ccos::environment::{CCOSEnvironment, CCOSBuilder, SecurityLevel};
    
    let env = CCOSBuilder::new()
        .security_level(SecurityLevel::Standard)
        .build()
        .expect("Failed to create CCOS environment");
    
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let test_file_path = format!("{}/tests/shared/rtfs_files/orchestration_primitives_test.rtfs", manifest_dir);
    
    let result = env.execute_file(&test_file_path)
        .expect("Failed to execute orchestration primitives test");
    
    // Handle ExecutionOutcome return type
    let actual_value = match result {
        rtfs_compiler::runtime::execution_outcome::ExecutionOutcome::Complete(value) => value,
        rtfs_compiler::runtime::execution_outcome::ExecutionOutcome::RequiresHost(_) => {
            panic!("Unexpected host call in orchestration primitives test");
        }
    };
    
    let expected = r#"String("All orchestration primitive tests completed!")"#;
    let actual = format!("{:?}", actual_value);
    
    assert_eq!(actual, expected, "Orchestration primitives test result mismatch");
}

#[test]
fn test_defstruct_evaluation_basic() {
    use rtfs_compiler::ast::{DefstructExpr, DefstructField, Symbol, Keyword, TypeExpr, PrimitiveType, MapKey};
    use rtfs_compiler::runtime::{Environment};
    use rtfs_compiler::runtime::values::{Value};
    use std::collections::HashMap;
    use crate::test_helpers;

    // Create a defstruct for testing
    let defstruct_expr = DefstructExpr {
        name: Symbol::new("TestStruct"),
        fields: vec![
            DefstructField {
                key: Keyword::new("name"),
                field_type: TypeExpr::Primitive(PrimitiveType::String),
            },
            DefstructField {
                key: Keyword::new("age"),
                field_type: TypeExpr::Primitive(PrimitiveType::Int),
            },
        ],
    };

    // Create evaluator using test helpers
    let evaluator = test_helpers::create_full_evaluator();
    let mut env = Environment::new();

    // Evaluate the defstruct
    let result = evaluator.eval_defstruct(&defstruct_expr, &mut env);
    assert!(result.is_ok(), "Failed to evaluate defstruct: {:?}", result);

    // Check that a constructor function was created
    let constructor = env.lookup(&Symbol::new("TestStruct"));
    assert!(constructor.is_some(), "Constructor function not found in environment");

    if let Some(constructor_value) = constructor {
        if let Value::Function(_) = &constructor_value {
            // Test valid input
            let mut valid_map = HashMap::new();
            valid_map.insert(MapKey::Keyword(Keyword::new("name")), Value::String("Alice".to_string()));
            valid_map.insert(MapKey::Keyword(Keyword::new("age")), Value::Integer(30));
            
            let valid_input = vec![Value::Map(valid_map)];
            let construct_result = evaluator.call_function(constructor_value.clone(), &valid_input, &mut env);
            assert!(construct_result.is_ok(), "Failed to construct valid struct: {:?}", construct_result);

            // Test invalid input - missing field
            let mut invalid_map = HashMap::new();
            invalid_map.insert(MapKey::Keyword(Keyword::new("name")), Value::String("Bob".to_string()));
            // Missing age field
            
            let invalid_input = vec![Value::Map(invalid_map)];
            let invalid_result = evaluator.call_function(constructor_value.clone(), &invalid_input, &mut env);
            assert!(invalid_result.is_err(), "Should have failed for missing field");

            // Test invalid type
            let mut wrong_type_map = HashMap::new();
            wrong_type_map.insert(MapKey::Keyword(Keyword::new("name")), Value::String("Charlie".to_string()));
            wrong_type_map.insert(MapKey::Keyword(Keyword::new("age")), Value::String("thirty".to_string())); // Wrong type
            
            let wrong_type_input = vec![Value::Map(wrong_type_map)];
            let wrong_type_result = evaluator.call_function(constructor_value.clone(), &wrong_type_input, &mut env);
            assert!(wrong_type_result.is_err(), "Should have failed for wrong type");
        } else {
            panic!("Expected a function value for the constructor");
        }
    } else {
        panic!("Constructor function not found");
    }
}

#[test]
fn test_defstruct_evaluation_empty() {
    use rtfs_compiler::ast::{DefstructExpr, Symbol, MapKey};
    use rtfs_compiler::runtime::{Environment};
    use rtfs_compiler::runtime::values::{Value};
    use std::collections::HashMap;
    use crate::test_helpers;

    // Create an empty defstruct for testing
    let defstruct_expr = DefstructExpr {
        name: Symbol::new("EmptyStruct"),
        fields: vec![],
    };

    // Create evaluator using test helpers
    let evaluator = test_helpers::create_full_evaluator();
    let mut env = Environment::new();

    // Evaluate the defstruct
    let result = evaluator.eval_defstruct(&defstruct_expr, &mut env);
    assert!(result.is_ok(), "Failed to evaluate empty defstruct: {:?}", result);

    // Check that a constructor function was created
    let constructor = env.lookup(&Symbol::new("EmptyStruct"));
    assert!(constructor.is_some(), "Constructor function not found in environment");

    if let Some(constructor_value) = constructor {
        if let Value::Function(_) = &constructor_value {
            // Test empty map - should work
            let empty_map = HashMap::new();
            let valid_input = vec![Value::Map(empty_map)];
            let construct_result = evaluator.call_function(constructor_value, &valid_input, &mut env);
            assert!(construct_result.is_ok(), "Failed to construct empty struct: {:?}", construct_result);
        } else {
            panic!("Expected a function value for the constructor");
        }
    } else {
        panic!("Constructor function not found");
    }
}
