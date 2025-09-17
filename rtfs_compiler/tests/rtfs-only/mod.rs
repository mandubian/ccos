// RTFS-Only Tests Module
// This module contains tests for pure RTFS functionality without CCOS dependencies

// Include all RTFS-only test files
mod ast_coverage;
mod debug_parse_failures;
mod enhanced_error_reporting;
mod get_shorthand;
mod ir_language_coverage;
mod ir_optimization;
mod ir_step_params_additional_tests;
mod ir_step_params_tests;
mod l4_cache_ir_integration;
mod parser;
mod realistic_model_tests;
mod secure_stdlib_comprehensive_tests;
mod set_form_tests;
mod simple_secure_stdlib_test;
mod stdlib_e2e_tests;
mod test_comment_preprocessing;
mod test_helpers;
mod test_implemented_functions;
mod test_missing_stdlib_functions;
mod test_qualified_symbols;
mod test_recursive_patterns;
mod test_simple_recursion;
mod test_simple_recursion_new;
mod test_type_annotation_whitespace;
mod type_system_tests;
