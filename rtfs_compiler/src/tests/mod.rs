// Tests module for RTFS compiler
// This module contains unit tests for various components

// Pure RTFS tests (no CCOS dependencies)
pub mod pure_test_utils;
pub mod collections_tests;
pub mod control_flow_tests;
pub mod cross_module_ir_tests;
pub mod function_tests;
pub mod grammar_tests;
pub mod module_loading_tests;
pub mod object_tests;
pub mod primitives_tests;

// CCOS integration tests
pub mod ccos_test_utils;
pub mod rtfs2_tests;
pub mod intent_storage_tests;
pub mod llm_execute_tests;

// Legacy test utilities (to be phased out)
pub mod test_utils;
pub mod test_helpers;
