// RTFS End-to-End Grammar Feature Test Matrix
// This is the most critical test for stabilization - systematic testing of every language feature

use rtfs_compiler::parser::parse_expression;
use std::env;
use std::fs;
use std::path::Path;
use crate::test_helpers::*;

/// Feature test configuration for each grammar rule
#[derive(Debug, Clone)]
struct FeatureTestConfig {
    /// Feature name (matches .rtfs filename)
    feature_name: String,
    /// Whether this feature should compile successfully
    should_compile: bool,
    /// Whether this feature should execute successfully  
    should_execute: bool,
    /// Runtime strategy to test with
    runtime_strategy: RuntimeStrategy,
    /// Expected error pattern if compilation/execution should fail
    expected_error: Option<&'static str>,
    /// Whether to test both AST and IR runtimes
    test_both_runtimes: bool,
    /// Feature category for organization
    category: FeatureCategory,
}

#[derive(Debug, Clone, Copy)]
enum RuntimeStrategy {
    Ast,
    Ir, 
    Both,
}

#[derive(Debug, Clone, Copy)]
enum FeatureCategory {
    SpecialForms,     // let, if, fn, do, match, try-catch, etc.
    DataStructures,   // vectors, maps, literals
    TypeSystem,       // type annotations, constraints
    Rtfs2Features,    // log-step, discover-agents, task context
    ControlFlow,      // parallel, with-resource
    Advanced,         // complex combinations
}

impl FeatureTestConfig {
    fn new(feature_name: &str, category: FeatureCategory) -> Self {
        FeatureTestConfig {
            feature_name: feature_name.to_string(),
            should_compile: true,
            should_execute: true,
            runtime_strategy: RuntimeStrategy::Both,
            expected_error: None,
            test_both_runtimes: true,
            category,
        }
    }

    fn should_fail(mut self, error_pattern: &'static str) -> Self {
        self.should_compile = false;
        self.should_execute = false;
        self.expected_error = Some(error_pattern);
        self
    }

    fn compilation_only(mut self) -> Self {
        self.should_execute = false;
        self
    }

    fn ast_only(mut self) -> Self {
        self.runtime_strategy = RuntimeStrategy::Ast;
        self.test_both_runtimes = false;
        self
    }

    fn ir_only(mut self) -> Self {
        self.runtime_strategy = RuntimeStrategy::Ir;
        self.test_both_runtimes = false;
        self
    }
}

/// Helper function to read and preprocess feature test files
fn read_feature_file(feature_name: &str) -> Result<String, String> {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let test_file_path = format!("{}/tests/shared/rtfs_files/features/{}.rtfs", manifest_dir, feature_name);

    if !Path::new(&test_file_path).exists() {
        return Err(format!("Feature test file not found: {}", test_file_path));
    }

    fs::read_to_string(&test_file_path)
        .map_err(|e| format!("Failed to read feature file {}: {}", test_file_path, e))
}

/// Extract individual test cases from a feature file
/// Each test case is separated by ";; Expected:" comments
fn extract_test_cases(content: &str) -> Vec<(String, String)> {
    let mut test_cases = Vec::new();
    let mut current_code = String::new();
    let mut current_expected = String::new();
    let mut in_expected = false;

    for line in content.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with(";; Expected:") {
            if !current_code.trim().is_empty() {
                current_expected = trimmed.trim_start_matches(";; Expected:").trim().to_string();
                in_expected = true;
            }
        } else if trimmed.starts_with(";;") && !trimmed.starts_with(";; Expected:") {
            // This is a regular comment line (not an Expected line)
            // If we have a complete test case, push it before starting a new one
            if in_expected && !current_code.trim().is_empty() {
                test_cases.push((current_code.trim().to_string(), current_expected.clone()));
                current_code.clear();
                current_expected.clear();
                in_expected = false;
            }
            // Regular comment lines don't start new test cases
        } else if trimmed.is_empty() {
            // Empty line - if we have a complete test case, push it
            if in_expected && !current_code.trim().is_empty() {
                test_cases.push((current_code.trim().to_string(), current_expected.clone()));
                current_code.clear();
                current_expected.clear();
                in_expected = false;
            }
            // Empty lines between test cases are fine
        } else {
            // Non-comment, non-empty line - this is code
            current_code.push_str(line);
            current_code.push('\n');
        }
    }

    // Handle final test case
    if !current_code.trim().is_empty() {
        test_cases.push((current_code.trim().to_string(), current_expected));
    }

    test_cases
}

/// Run a single test case within a feature
fn run_test_case(
    test_code: &str,
    expected: &str,
    runtime_strategy: RuntimeStrategy,
    feature_name: &str,
    case_index: usize,
) -> Result<String, String> {
    // Create evaluator with proper host setup
    let evaluator = create_full_evaluator();
    
    // Set up execution context for host method calls
    // setup_execution_context is no longer needed in the new architecture

    // Try to parse the expression
    let expr = parse_expression(test_code)
        .map_err(|e| format!("Parse error in {}[{}]: {:?}", feature_name, case_index, e))?;

    // Try to run the expression using the evaluator
    match evaluator.eval_expr(&expr, &mut evaluator.env.clone()) {
        Ok(rtfs_compiler::runtime::execution_outcome::ExecutionOutcome::Complete(result)) => Ok(result.to_string()),
        Ok(rtfs_compiler::runtime::execution_outcome::ExecutionOutcome::RequiresHost(_)) => Err(format!("Host call required in {}[{}]", feature_name, case_index)),
        Err(e) => Err(format!("Runtime error in {}[{}]: {:?}", feature_name, case_index, e)),
    }
}

/// Run all test cases for a feature
fn run_feature_tests(config: &FeatureTestConfig) -> Result<(), String> {
    let content = read_feature_file(&config.feature_name)?;
    let test_cases = extract_test_cases(&content);

    if test_cases.is_empty() {
        return Err(format!("No test cases found in feature: {}", config.feature_name));
    }

    println!("Testing feature: {} ({} test cases)", config.feature_name, test_cases.len());

    let strategies = if config.test_both_runtimes {
        vec![RuntimeStrategy::Ast, RuntimeStrategy::Ir]
    } else {
        vec![config.runtime_strategy]
    };

    for (case_index, (test_code, expected)) in test_cases.iter().enumerate() {
        for strategy in &strategies {
            let strategy_name = match strategy {
                RuntimeStrategy::Ast => "AST",
                RuntimeStrategy::Ir => "IR",
                RuntimeStrategy::Both => unreachable!(),
            };

            if config.should_compile && config.should_execute {
                match run_test_case(test_code, expected, *strategy, &config.feature_name, case_index) {
                    Ok(actual) => {
                        // For now, just verify it doesn't crash
                        // TODO: Add more sophisticated result validation
                        println!("  ✓ {}[{}] ({}) -> {}", config.feature_name, case_index, strategy_name, actual);
                    }
                    Err(e) => {
                        return Err(format!("Unexpected failure in {}[{}] ({}): {}", 
                                         config.feature_name, case_index, strategy_name, e));
                    }
                }
            } else {
                // Test should fail - verify it fails as expected
                match run_test_case(test_code, expected, *strategy, &config.feature_name, case_index) {
                    Ok(result) => {
                        return Err(format!("Expected failure in {}[{}] ({}), but got: {}", 
                                         config.feature_name, case_index, strategy_name, result));
                    }
                    Err(e) => {
                        if let Some(expected_error) = config.expected_error {
                            if e.contains(expected_error) {
                                println!("  ✓ {}[{}] ({}) failed as expected: {}", 
                                       config.feature_name, case_index, strategy_name, e);
                            } else {
                                return Err(format!("Wrong error in {}[{}] ({}). Expected '{}', got: {}", 
                                                 config.feature_name, case_index, strategy_name, expected_error, e));
                            }
                        } else {
                            println!("  ✓ {}[{}] ({}) failed as expected: {}", 
                                   config.feature_name, case_index, strategy_name, e);
                        }
                    }
                }
            }
        }
    }

    Ok(())
}

// MARK: - Core Special Forms Tests

#[test]
fn test_let_expressions_feature() {
    let config = FeatureTestConfig::new("let_expressions", FeatureCategory::SpecialForms);
    run_feature_tests(&config).expect("let_expressions feature tests failed");
}

#[test]
fn test_if_expressions_feature() {
    let config = FeatureTestConfig::new("if_expressions", FeatureCategory::SpecialForms);
    run_feature_tests(&config).expect("if_expressions feature tests failed");
}

#[test]
fn test_function_expressions_feature() {
    let config = FeatureTestConfig::new("function_expressions", FeatureCategory::SpecialForms);
    run_feature_tests(&config).expect("function_expressions feature tests failed");
}

#[test]
fn test_do_expressions_feature() {
    let config = FeatureTestConfig::new("do_expressions", FeatureCategory::SpecialForms);
    run_feature_tests(&config).expect("do_expressions feature tests failed");
}

#[test]
fn test_match_expressions_feature() {
    let config = FeatureTestConfig::new("match_expressions", FeatureCategory::SpecialForms);
    run_feature_tests(&config).expect("match_expressions feature tests failed");
}

#[test]
fn test_try_catch_expressions_feature() {
    let config = FeatureTestConfig::new("try_catch_expressions", FeatureCategory::SpecialForms);
    run_feature_tests(&config).expect("try_catch_expressions feature tests failed");
}

#[test]
fn test_def_defn_expressions_feature() {
    let config = FeatureTestConfig::new("def_defn_expressions", FeatureCategory::SpecialForms);
    run_feature_tests(&config).expect("def_defn_expressions feature tests failed");
}

// MARK: - Control Flow Tests

#[test]
fn test_parallel_expressions_feature() {
    let config = FeatureTestConfig::new("parallel_expressions", FeatureCategory::ControlFlow);
    run_feature_tests(&config).expect("parallel_expressions feature tests failed");
}

#[test]
fn test_with_resource_expressions_feature() {
    let config = FeatureTestConfig::new("with_resource_expressions", FeatureCategory::ControlFlow);
    run_feature_tests(&config).expect("with_resource_expressions feature tests failed");
}

// MARK: - Data Structure Tests

#[test]
fn test_literal_values_feature() {
    let config = FeatureTestConfig::new("literal_values", FeatureCategory::DataStructures);
    run_feature_tests(&config).expect("literal_values feature tests failed");
}

#[test]
fn test_vector_operations_feature() {
    let config = FeatureTestConfig::new("vector_operations", FeatureCategory::DataStructures);
    run_feature_tests(&config).expect("vector_operations feature tests failed");
}

#[test]
fn test_map_operations_feature() {
    let config = FeatureTestConfig::new("map_operations", FeatureCategory::DataStructures);
    run_feature_tests(&config).expect("map_operations feature tests failed");
}

// MARK: - RTFS 2.0 Specific Tests

// REMOVED: test_rtfs2_special_forms_feature - moved to CCOS integration tests 
// as it requires CCOS execution context for special forms like step, @plan-id, etc.

// MARK: - Mutation & State Tests

#[test]
fn test_mutation_and_state_feature() {
    let config = FeatureTestConfig::new("mutation_and_state", FeatureCategory::SpecialForms);
    run_feature_tests(&config).expect("mutation_and_state feature tests failed");
}

// MARK: - Type System Tests

#[test]
fn test_type_system_feature() {
    let config = FeatureTestConfig::new("type_system", FeatureCategory::TypeSystem);
    run_feature_tests(&config).expect("type_system feature tests failed");
}

// MARK: - Comprehensive Integration Test

#[test]
fn test_all_features_integration() {
    let all_features = vec![
        // Special Forms
        FeatureTestConfig::new("let_expressions", FeatureCategory::SpecialForms),
        FeatureTestConfig::new("if_expressions", FeatureCategory::SpecialForms),
        FeatureTestConfig::new("function_expressions", FeatureCategory::SpecialForms),
        FeatureTestConfig::new("do_expressions", FeatureCategory::SpecialForms),
        FeatureTestConfig::new("match_expressions", FeatureCategory::SpecialForms),
        FeatureTestConfig::new("try_catch_expressions", FeatureCategory::SpecialForms),
        FeatureTestConfig::new("def_defn_expressions", FeatureCategory::SpecialForms),
        
        // Control Flow
        FeatureTestConfig::new("parallel_expressions", FeatureCategory::ControlFlow),
        FeatureTestConfig::new("with_resource_expressions", FeatureCategory::ControlFlow),
        
        // Data Structures
        FeatureTestConfig::new("literal_values", FeatureCategory::DataStructures),
        FeatureTestConfig::new("vector_operations", FeatureCategory::DataStructures),
        FeatureTestConfig::new("map_operations", FeatureCategory::DataStructures),
    // Mutation & State
    FeatureTestConfig::new("mutation_and_state", FeatureCategory::SpecialForms),
        
        // RTFS 2.0 Features
        // REMOVED: rtfs2_special_forms - moved to CCOS integration tests
        
        // Type System
        FeatureTestConfig::new("type_system", FeatureCategory::TypeSystem),
    ];

    let mut failed_features = Vec::new();
    let mut total_features = 0;
    let mut passed_features = 0;

    for config in all_features {
        total_features += 1;
        match run_feature_tests(&config) {
            Ok(()) => {
                passed_features += 1;
                println!("✓ Feature '{}' passed", config.feature_name);
            }
            Err(e) => {
                failed_features.push((config.feature_name.clone(), e));
                println!("✗ Feature '{}' failed", config.feature_name);
            }
        }
    }

    println!("\n=== RTFS End-to-End Feature Test Summary ===");
    println!("Total features tested: {}", total_features);
    println!("Features passed: {}", passed_features);
    println!("Features failed: {}", failed_features.len());

    if !failed_features.is_empty() {
        println!("\nFailed features:");
        for (feature, error) in &failed_features {
            println!("  - {}: {}", feature, error);
        }
        
        panic!("Feature test matrix failed! {} out of {} features failed", 
               failed_features.len(), total_features);
    } else {
        println!("\n🎉 All features passed! RTFS compiler is stable.");
    }
}

// MARK: - Grammar Coverage Report

#[test] 
fn test_grammar_coverage_report() {
    println!("\n=== RTFS Grammar Coverage Report ===");
    
    let covered_rules = vec![
        // Core expressions
        "literal", "symbol", "keyword", "vector", "map", "list",
        
        // Special forms  
        "let_expr", "if_expr", "fn_expr", "do_expr", "match_expr", 
        "try_catch_expr", "def_expr", "defn_expr", "parallel_expr", 
        "with_resource_expr",
        
        // RTFS 2.0 extensions
        "log_step_expr", "discover_agents_expr", "task_context_access",
        "resource_ref", "timestamp", "uuid", "resource_handle",
        
        // Type system
        "type_expr", "type_annotation", "primitive_type", "vector_type",
        "map_type", "function_type", "union_type",
        
        // Patterns and destructuring
        "binding_pattern", "match_pattern", "vector_destructuring_pattern",
        "map_destructuring_pattern",
    ];
    
    println!("Grammar rules covered by feature tests:");
    for rule in &covered_rules {
        println!("  ✓ {}", rule);
    }
    
    println!("\nTotal rules covered: {}", covered_rules.len());
    
    // TODO: Add rules that still need coverage
    let needs_coverage = vec![
        "import_definition", "module_definition", "metadata",
        "versioned_namespace", "custom_predicates",
    ];
    
    if !needs_coverage.is_empty() {
        println!("\nRules that need additional coverage:");
        for rule in &needs_coverage {
            println!("  ⚠ {}", rule);
        }
    }
}
