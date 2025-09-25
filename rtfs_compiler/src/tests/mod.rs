// Tests module for RTFS compiler
// This module contains unit tests for various components

// Pure RTFS tests (no CCOS dependencies)
pub mod pure;

// CCOS integration tests
pub mod ccos;

// Legacy test utilities (to be phased out)
pub mod test_helpers;
pub mod test_utils;
