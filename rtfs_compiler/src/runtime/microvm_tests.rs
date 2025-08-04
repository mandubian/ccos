//! Tests for enhanced MicroVM functionality with program execution and capability permissions

use super::microvm::*;
use crate::runtime::security::RuntimeContext;
use crate::runtime::values::Value;
use crate::runtime::{RuntimeResult, RuntimeError};

#[test]
fn test_program_enum_helpers() {
    // Test network operation detection
    let network_program = Program::RtfsSource("(http-fetch \"https://api.example.com\")".to_string());
    assert!(network_program.is_network_operation());
    
    let file_program = Program::RtfsSource("(read-file \"/tmp/test.txt\")".to_string());
    assert!(file_program.is_file_operation());
    
    let math_program = Program::RtfsSource("(+ 1 2)".to_string());
    assert!(!math_program.is_network_operation());
    assert!(!math_program.is_file_operation());
    
    // Test external program detection
    let curl_program = Program::ExternalProgram {
        path: "/usr/bin/curl".to_string(),
        args: vec!["https://api.example.com".to_string()],
    };
    assert!(curl_program.is_network_operation());
    
    let cat_program = Program::ExternalProgram {
        path: "/bin/cat".to_string(),
        args: vec!["/tmp/test.txt".to_string()],
    };
    assert!(cat_program.is_file_operation());
}
// ...existing code...
