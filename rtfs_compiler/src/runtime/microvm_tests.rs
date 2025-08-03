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

#[test]
fn test_enhanced_execution_context() {
    let program = Program::RtfsSource("(+ 1 2)".to_string());
    let permissions = vec!["ccos.math.add".to_string()];
    let runtime_context = RuntimeContext::controlled(permissions.clone());
    
    let context = ExecutionContext {
        execution_id: "test-exec-1".to_string(),
        program: Some(program),
        capability_id: None,
        capability_permissions: permissions,
        args: vec![Value::Integer(1), Value::Integer(2)],
        config: MicroVMConfig::default(),
        runtime_context: Some(runtime_context),
    };
    
    assert_eq!(context.execution_id, "test-exec-1");
    assert!(context.program.is_some());
    assert!(context.capability_id.is_none());
    assert_eq!(context.capability_permissions.len(), 1);
    assert_eq!(context.args.len(), 2);
}

#[test]
fn test_mock_provider_program_execution() {
    let mut factory = MicroVMFactory::new();
    
    // Initialize the provider
    factory.initialize_provider("mock").expect("Failed to initialize mock provider");
    
    let provider = factory.get_provider("mock").expect("Mock provider should be available");
    
    let program = Program::RtfsSource("(+ 3 4)".to_string());
    let mut runtime_context = RuntimeContext::controlled(vec![
        "ccos.math.add".to_string(),
        "ccos.math".to_string(),
        "math".to_string(),
    ]);
    
    let context = ExecutionContext {
        execution_id: "test-exec-2".to_string(),
        program: Some(program),
        capability_id: None,
        capability_permissions: vec!["ccos.math.add".to_string()],
        args: vec![Value::Integer(3), Value::Integer(4)],
        config: MicroVMConfig::default(),
        runtime_context: Some(runtime_context),
    };
    
    let result = provider.execute_program(context);
    if let Err(ref e) = result {
        println!("Mock provider execution failed: {:?}", e);
    }
    assert!(result.is_ok());
    
    let execution_result = result.unwrap();
    // The RTFS expression (+ 3 4) should evaluate to 7
    if let Value::Integer(value) = execution_result.value {
        assert_eq!(value, 7);
    } else {
        panic!("Expected integer result, got {:?}", execution_result.value);
    }
    assert!(execution_result.metadata.duration.as_millis() > 0);
}



#[test]
fn test_backward_compatibility() {
    let mut factory = MicroVMFactory::new();
    
    // Initialize the provider
    factory.initialize_provider("mock").expect("Failed to initialize mock provider");
    
    let provider = factory.get_provider("mock").expect("Mock provider should be available");
    
    // Test legacy capability-based execution
    let context = ExecutionContext {
        execution_id: "test-exec-3".to_string(),
        program: None,
        capability_id: Some("ccos.math.add".to_string()),
        capability_permissions: vec!["ccos.math.add".to_string()],
        args: vec![Value::Integer(1), Value::Integer(2)],
        config: MicroVMConfig::default(),
        runtime_context: None,
    };
    
    let result = provider.execute_capability(context);
    assert!(result.is_ok());
    
    let execution_result = result.unwrap();
    // For backward compatibility, it should return a string message
    assert!(execution_result.value.as_string().is_some());
    assert!(execution_result.value.as_string().unwrap().contains("Mock capability execution"));
}

#[test]
fn test_program_description() {
    let bytecode_program = Program::RtfsBytecode(vec![1, 2, 3, 4, 5]);
    assert_eq!(bytecode_program.description(), "RTFS bytecode (5 bytes)");
    
    let source_program = Program::RtfsSource("(+ 1 2)".to_string());
    assert_eq!(source_program.description(), "RTFS source: (+ 1 2)");
    
    let external_program = Program::ExternalProgram {
        path: "/bin/ls".to_string(),
        args: vec!["-la".to_string()],
    };
    assert_eq!(external_program.description(), "External program: /bin/ls [\"-la\"]");
}

 

#[test]
fn test_process_provider_program_execution() {
    let mut factory = MicroVMFactory::new();
    
    // Initialize the provider
    factory.initialize_provider("process").expect("Failed to initialize process provider");
    
    let provider = factory.get_provider("process").expect("Process provider should be available");
    
    // Test external program execution
    let external_program = Program::ExternalProgram {
        path: "/bin/echo".to_string(),
        args: vec!["hello world".to_string()],
    };
    let runtime_context = RuntimeContext::controlled(vec![
        "external_program".to_string(),
    ]);
    
    let context = ExecutionContext {
        execution_id: "test-exec-process-1".to_string(),
        program: Some(external_program),
        capability_id: None,
        capability_permissions: vec!["external_program".to_string()],
        args: vec![],
        config: MicroVMConfig::default(),
        runtime_context: Some(runtime_context),
    };
    
    let result = provider.execute_program(context);
    if let Err(ref e) = result {
        println!("Process provider execution failed: {:?}", e);
    }
    assert!(result.is_ok());
    
    let execution_result = result.unwrap();
    // The echo command should return "hello world"
    if let Value::String(value) = execution_result.value {
        assert!(value.contains("hello world"));
    } else {
        panic!("Expected string result, got {:?}", execution_result.value);
    }
    assert!(execution_result.metadata.duration.as_millis() > 0);
}

#[test]
fn test_process_provider_native_function() {
    let mut factory = MicroVMFactory::new();
    
    // Initialize the provider
    factory.initialize_provider("process").expect("Failed to initialize process provider");
    
    let provider = factory.get_provider("process").expect("Process provider should be available");
    
    // Test native function execution
    let native_func = |args: Vec<Value>| -> RuntimeResult<Value> {
        Ok(Value::String(format!("Native function called with {} args", args.len())))
    };
    let native_program = Program::NativeFunction(native_func);
    let runtime_context = RuntimeContext::controlled(vec![
        "native_function".to_string(),
    ]);
    
    let context = ExecutionContext {
        execution_id: "test-exec-process-2".to_string(),
        program: Some(native_program),
        capability_id: None,
        capability_permissions: vec!["native_function".to_string()],
        args: vec![Value::Integer(1), Value::Integer(2)],
        config: MicroVMConfig::default(),
        runtime_context: Some(runtime_context),
    };
    
    let result = provider.execute_program(context);
    assert!(result.is_ok());
    
    let execution_result = result.unwrap();
    if let Value::String(value) = execution_result.value {
        assert!(value.contains("Native function called with 2 args"));
    } else {
        panic!("Expected string result, got {:?}", execution_result.value);
    }
}

#[test]
fn test_process_provider_rtfs_execution() {
    let mut factory = MicroVMFactory::new();
    
    // Initialize the provider
    factory.initialize_provider("process").expect("Failed to initialize process provider");
    
    let provider = factory.get_provider("process").expect("Process provider should be available");
    
    // Test RTFS execution in process provider
    let rtfs_program = Program::RtfsSource("(+ 5 3)".to_string());
    let runtime_context = RuntimeContext::controlled(vec![
        "ccos.math.add".to_string(),
        "ccos.math".to_string(),
        "math".to_string(),
    ]);
    
    let context = ExecutionContext {
        execution_id: "test-exec-process-rtfs".to_string(),
        program: Some(rtfs_program),
        capability_id: None,
        capability_permissions: vec!["ccos.math.add".to_string()],
        args: vec![Value::Integer(5), Value::Integer(3)],
        config: MicroVMConfig::default(),
        runtime_context: Some(runtime_context),
    }; 
    
    let result = provider.execute_program(context);
    if let Err(ref e) = result {
        println!("Process provider RTFS execution failed: {:?}", e);
    }
    assert!(result.is_ok());
    
    let execution_result = result.unwrap();
    // The RTFS expression (+ 5 3) should evaluate to 8
    if let Value::Integer(value) = execution_result.value {
        assert_eq!(value, 8);
    } else {
        panic!("Expected integer result, got {:?}", execution_result.value);
    }
    assert!(execution_result.metadata.duration.as_millis() > 0);
}

#[test]
fn test_process_provider_permission_validation() {
    let mut factory = MicroVMFactory::new();
    
    // Initialize the provider
    factory.initialize_provider("process").expect("Failed to initialize process provider");
    
    let provider = factory.get_provider("process").expect("Process provider should be available");
    
    // Test that external program execution is blocked without permission
    let external_program = Program::ExternalProgram {
        path: "/bin/echo".to_string(),
        args: vec!["test".to_string()],
    };
    let runtime_context = RuntimeContext::controlled(vec![
        "ccos.math.add".to_string(), // No external_program permission
    ]);
    
    let context = ExecutionContext {
        execution_id: "test-exec-process-3".to_string(),
        program: Some(external_program),
        capability_id: None,
        capability_permissions: vec!["ccos.math.add".to_string()],
        args: vec![],
        config: MicroVMConfig::default(),
        runtime_context: Some(runtime_context),
    };
    
    let result = provider.execute_program(context);
    // This should fail because we don't have external_program permission
    assert!(result.is_err());
    if let Err(RuntimeError::SecurityViolation { capability, .. }) = result {
        assert_eq!(capability, "external_program");
    } else {
        panic!("Expected SecurityViolation error");
    }
} 