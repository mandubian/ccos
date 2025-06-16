use std::fs;
use std::path::Path;
use rtfs_compiler::*;

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
fn run_test_file(config: &TestConfig, runtime: &str) -> Result<String, String> {
    let test_file_path = format!("tests/rtfs_files/{}.rtfs", config.name);
    
    // Check if file exists
    if !Path::new(&test_file_path).exists() {
        return Err(format!("Test file not found: {}", test_file_path));
    }

    // Read the file content
    let content = fs::read_to_string(&test_file_path)
        .map_err(|e| format!("Failed to read file {}: {}", test_file_path, e))?;

    // Pre-process content to handle comments and find the actual expression
    let processed_content = preprocess_test_content(&content);

    // Parse the code
    let parsed = match parser::parse_expression(&processed_content) {
        Ok(ast) => ast,
        Err(e) => {
            return if config.should_compile {
                Err(format!("Parsing failed unexpectedly: {:?}", e))
            } else {
                // Check if error matches expected pattern
                let error_str = format!("{:?}", e);
                if let Some(expected) = config.expected_error {
                    if error_str.contains(expected) {
                        Ok("Expected parsing error".to_string())
                    } else {
                        Err(format!("Expected error '{}' but got: {}", expected, error_str))
                    }
                } else {
                    Ok("Parsing failed as expected".to_string())
                }
            };
        }
    };

    if !config.should_compile {
        return Err("Expected compilation to fail, but parsing succeeded".to_string());
    }

    // Try to execute with the specified runtime
    let result = match runtime {
        "ast" => {
            let evaluator = runtime::evaluator::Evaluator::new();
            match evaluator.evaluate(&parsed) {
                Ok(value) => format!("{:?}", value),
                Err(e) => return Err(format!("AST runtime error: {:?}", e)),
            }
        }
        "ir" => {
            // Convert to IR
            let mut converter = ir_converter::IrConverter::new();
            let ir_node = match converter.convert_expression(parsed) {
                Ok(node) => node,
                Err(e) => return Err(format!("IR conversion error: {:?}", e)),
            };

            // Execute with IR runtime
            let agent_discovery = Box::new(agent::discovery_traits::NoOpAgentDiscovery);
            let mut runtime = runtime::Runtime::with_strategy_and_agent_discovery(
                runtime::RuntimeStrategy::Ir,
                agent_discovery
            );
            match runtime.evaluate_ir(&ir_node) {
                Ok(value) => format!("{:?}", value),
                Err(e) => return Err(format!("IR runtime error: {:?}", e)),
            }
        }
        _ => return Err(format!("Unknown runtime: {}", runtime)),
    };

    if !config.should_execute {
        return Err("Expected execution to fail, but it succeeded".to_string());
    }

    Ok(result)
}



// Individual test functions for better granularity and tracking

macro_rules! create_rtfs_test {
    ($test_name:ident, $file_name:expr) => {
        #[test]
        fn $test_name() {
            let config = TestConfig::new($file_name);
            
            let runtimes = match config.runtime {
                RuntimeStrategy::Ast => vec!["ast"],
                RuntimeStrategy::Ir => vec!["ir"],
                RuntimeStrategy::Both => vec!["ast", "ir"],
            };

            for runtime in runtimes {
                let test_name = format!("{} ({})", config.name, runtime);
                
                match run_test_file(&config, runtime) {
                    Ok(result) => {
                        println!("✅ {}: {}", test_name, result.trim());
                        
                        // Check expected result if specified
                        if let Some(expected) = config.expected_result {
                            assert!(result.contains(expected), 
                                "Expected '{}' but got '{}'", expected, result);
                        }
                    }
                    Err(error) => {
                        // Check if this was an expected failure
                        if !config.should_compile || !config.should_execute {
                            if let Some(expected_error) = config.expected_error {
                                assert!(error.contains(expected_error),
                                    "Expected error containing '{}' but got '{}'", expected_error, error);
                                println!("✅ {} (expected failure): {}", test_name, error);
                                continue;
                            }
                        }
                        
                        panic!("Test failed: {}: {}", test_name, error);
                    }
                }
            }
        }
    };
}

// Basic arithmetic tests
create_rtfs_test!(test_single_expression, "test_single_expression");
create_rtfs_test!(test_complex_expression, "test_complex_expression");
create_rtfs_test!(test_nested_ops, "test_nested_ops");
create_rtfs_test!(test_complex_math, "test_complex_math");

// Let binding tests
create_rtfs_test!(test_basic_let, "test_basic_let");
create_rtfs_test!(test_let_binding, "test_let_binding");
create_rtfs_test!(test_let_no_type, "test_let_no_type");
create_rtfs_test!(test_typed_let, "test_typed_let");
create_rtfs_test!(test_simple_dependent, "test_simple_dependent");
create_rtfs_test!(test_dependent_let, "test_dependent_let");
create_rtfs_test!(test_multi_let, "test_multi_let");
create_rtfs_test!(test_mixed_let_simple, "test_mixed_let_simple");
create_rtfs_test!(test_expression_in_let, "test_expression_in_let");

// Conditional tests
create_rtfs_test!(test_conditional, "test_conditional");

// Function tests
create_rtfs_test!(test_simple_function, "test_simple_function");
create_rtfs_test!(test_functions_control, "test_functions_control");

// Comprehensive tests
create_rtfs_test!(test_comprehensive, "test_comprehensive");
create_rtfs_test!(test_no_comments, "test_no_comments");
create_rtfs_test!(test_working_features, "test_working_features");

// Real-world tests
create_rtfs_test!(test_simple_real, "test_simple_real");
create_rtfs_test!(test_basic_real, "test_basic_real");
create_rtfs_test!(test_real_world, "test_real_world");
create_rtfs_test!(test_real_world_fixed, "test_real_world_fixed");
create_rtfs_test!(test_production, "test_production");

// Advanced tests
create_rtfs_test!(test_incrementally_complex, "test_incrementally_complex");
create_rtfs_test!(test_computational_heavy, "test_computational_heavy");
create_rtfs_test!(test_advanced_focused, "test_advanced_focused");
create_rtfs_test!(test_advanced_pipeline, "test_advanced_pipeline");

// Agent system tests
create_rtfs_test!(test_agent_coordination, "test_agent_coordination");
create_rtfs_test!(test_agent_discovery, "test_agent_discovery");
create_rtfs_test!(test_fault_tolerance, "test_fault_tolerance");

// Task context tests
create_rtfs_test!(test_task_context, "test_task_context");
create_rtfs_test!(simple_task_context_test, "simple_task_context_test");

// Special test - This test now passes so we'll treat it as a regular test
#[test]
fn test_mixed_let() {
    let config = TestConfig::new("test_mixed_let");
    
    let runtimes = match config.runtime {
        RuntimeStrategy::Ast => vec!["ast"],
        RuntimeStrategy::Ir => vec!["ir"],
        RuntimeStrategy::Both => vec!["ast", "ir"],
    };

    for runtime in runtimes {
        let test_name = format!("{} ({})", config.name, runtime);
        
        match run_test_file(&config, runtime) {
            Ok(result) => {
                println!("✅ {}: {}", test_name, result.trim());
                
                // Check expected result if specified
                if let Some(expected) = config.expected_result {
                    assert!(result.contains(expected), 
                        "Expected '{}' but got '{}'", expected, result);
                }
            }
            Err(error) => {
                panic!("Test failed: {}: {}", test_name, error);
            }
        }
    }
}

/// Test specific categories of functionality
#[test]
fn test_let_bindings() {
    let let_binding_tests = vec![
        "test_basic_let",
        "test_let_binding", 
        "test_let_no_type",
        "test_typed_let",
        "test_simple_dependent",
        "test_dependent_let",
        "test_mixed_let_simple",
    ];

    for test_name in let_binding_tests {
        let config = TestConfig::new(test_name);
        
        // Test both runtimes
        for runtime in ["ast", "ir"] {
            let result = run_test_file(&config, runtime);
            assert!(result.is_ok(), 
                "Let binding test '{}' with '{}' runtime failed: {:?}", 
                test_name, runtime, result.err());
            println!("✅ {} ({}): {}", test_name, runtime, result.unwrap().trim());
        }
    }
}

#[test]
fn debug_complex_expression_only() {
    let test_file_path = "tests/rtfs_files/test_complex_expression.rtfs";
    let content = fs::read_to_string(test_file_path).unwrap();
    println!("Content: '{}'", content);
    
    // Parse the code
    let parsed = parser::parse_expression(&content).unwrap();
    println!("Parsed AST: {:#?}", parsed);
    
    // Convert to IR and examine nodes
    let mut converter = ir_converter::IrConverter::new();
    let ir_node = converter.convert_expression(parsed).unwrap();
    println!("IR Node: {:#?}", ir_node);
    
    // Try to execute with IR runtime  
    let agent_discovery = Box::new(agent::discovery_traits::NoOpAgentDiscovery);
    let mut runtime = runtime::Runtime::with_strategy_and_agent_discovery(
        runtime::RuntimeStrategy::Ir,
        agent_discovery
    );
    
    match runtime.evaluate_ir(&ir_node) {
        Ok(value) => println!("Result: {:?}", value),
        Err(e) => {
            println!("Error: {:?}", e);
            panic!("IR execution failed: {:?}", e);
        }
    }
}

#[test]
fn test_basic_arithmetic() {
    let arithmetic_tests = vec![
        "test_single_expression",
        "test_complex_expression", 
        "test_nested_ops",
    ];

    for test_name in arithmetic_tests {
        let config = TestConfig::new(test_name);
        
        // Test both runtimes
        for runtime in ["ast", "ir"] {
            let result = run_test_file(&config, runtime);
            assert!(result.is_ok(), 
                "Arithmetic test '{}' with '{}' runtime failed: {:?}", 
                test_name, runtime, result.err());
            println!("✅ {} ({}): {}", test_name, runtime, result.unwrap().trim());
        }
    }
}
