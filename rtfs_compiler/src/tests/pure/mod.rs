//! Pure RTFS tests - no CCOS dependencies
//!
//! These tests use PureHost and test RTFS language features in isolation
//! without requiring CCOS orchestration, capabilities, or external dependencies.

pub mod collections_tests;
pub mod control_flow_tests;
pub mod cross_module_ir_tests;
pub mod function_tests;
pub mod grammar_tests;
pub mod module_loading_tests;
pub mod object_tests;
pub mod primitives_tests;
pub mod pure_test_utils;
