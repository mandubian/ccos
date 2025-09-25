//! Comprehensive MicroVM Provider Lifecycle Tests
//!
//! This test suite validates the complete lifecycle of MicroVM providers:
//! - Initialization
//! - Availability checking
//! - Program execution
//! - Capability execution
//! - Resource cleanup
//! - Error handling

use rtfs_compiler::runtime::microvm::*;
use rtfs_compiler::runtime::security::RuntimeContext;
use rtfs_compiler::runtime::values::Value;
use rtfs_compiler::runtime::RuntimeError;
use std::time::Duration;

#[test]
fn test_mock_provider_lifecycle() {
    // Test 1: Create provider
    let mut provider = providers::mock::MockMicroVMProvider::new();
    assert_eq!(provider.name(), "mock");
    assert!(provider.is_available());

    // Test 2: Initial state
    assert!(!provider.initialized);

    // Test 3: Initialize
    let init_result = provider.initialize();
    assert!(init_result.is_ok());
    assert!(provider.initialized);

    // Test 4: Execute program before initialization (should fail)
    let uninit_provider = providers::mock::MockMicroVMProvider::new();
    let context = ExecutionContext {
        execution_id: "test-exec-1".to_string(),
        program: Some(Program::RtfsSource("(+ 1 2)".to_string())),
        capability_id: None,
        capability_permissions: vec![],
        args: vec![],
        config: config::MicroVMConfig::default(),
        runtime_context: None,
    };

    let result = uninit_provider.execute_program(context);
    assert!(result.is_err());
    if let Err(RuntimeError::Generic(msg)) = result {
        assert!(msg.contains("not initialized"));
    }

    // Test 5: Execute program after initialization
    let context = ExecutionContext {
        execution_id: "test-exec-2".to_string(),
        program: Some(Program::RtfsSource("(+ 3 4)".to_string())),
        capability_id: None,
        capability_permissions: vec![],
        args: vec![],
        config: config::MicroVMConfig::default(),
        runtime_context: None,
    };

    let result = provider.execute_program(context);
    assert!(result.is_ok());

    let execution_result = result.unwrap();
    if let Value::Integer(value) = execution_result.value {
        assert_eq!(value, 7);
    } else {
        panic!("Expected integer result, got {:?}", execution_result.value);
    }

    // Test 6: Execute capability
    let context = ExecutionContext {
        execution_id: "test-exec-3".to_string(),
        program: None,
        capability_id: Some("ccos.math.add".to_string()),
        capability_permissions: vec!["ccos.math.add".to_string()],
        args: vec![Value::Integer(5), Value::Integer(6)],
        config: config::MicroVMConfig::default(),
        runtime_context: None,
    };

    let result = provider.execute_capability(context);
    assert!(result.is_ok());

    let execution_result = result.unwrap();
    if let Value::String(msg) = &execution_result.value {
        assert!(msg.contains("5 + 6 = 11"));
    } else {
        panic!("Expected string result, got {:?}", execution_result.value);
    }

    // Test 7: Cleanup
    let cleanup_result = provider.cleanup();
    assert!(cleanup_result.is_ok());
    assert!(!provider.initialized);
}

#[test]
fn test_mock_provider_program_types() {
    let mut provider = providers::mock::MockMicroVMProvider::new();
    provider.initialize().unwrap();

    // Test RTFS Source
    let context = ExecutionContext {
        execution_id: "test-rtfs-source".to_string(),
        program: Some(Program::RtfsSource("(* 2 3)".to_string())),
        capability_id: None,
        capability_permissions: vec![],
        args: vec![],
        config: config::MicroVMConfig::default(),
        runtime_context: None,
    };

    let result = provider.execute_program(context);
    assert!(result.is_ok());
    let execution_result = result.unwrap();
    if let Value::Integer(value) = execution_result.value {
        assert_eq!(value, 6);
    } else {
        panic!("Expected integer result, got {:?}", execution_result.value);
    }

    // Test External Program
    let context = ExecutionContext {
        execution_id: "test-external".to_string(),
        program: Some(Program::ExternalProgram {
            path: "/bin/echo".to_string(),
            args: vec!["hello".to_string()],
        }),
        capability_id: None,
        capability_permissions: vec![],
        args: vec![],
        config: config::MicroVMConfig::default(),
        runtime_context: None,
    };

    let result = provider.execute_program(context);
    assert!(result.is_ok());
    let execution_result = result.unwrap();
    if let Value::String(msg) = &execution_result.value {
        assert!(msg.contains("Mock external program"));
        assert!(msg.contains("/bin/echo"));
    } else {
        panic!("Expected string result, got {:?}", execution_result.value);
    }

    // Test Native Function
    let context = ExecutionContext {
        execution_id: "test-native".to_string(),
        program: Some(Program::NativeFunction(|args| {
            if args.len() == 2 {
                if let (Some(Value::Integer(a)), Some(Value::Integer(b))) =
                    (args.get(0), args.get(1))
                {
                    Ok(Value::Integer(a + b))
                } else {
                    Err(RuntimeError::Generic("Invalid arguments".to_string()))
                }
            } else {
                Err(RuntimeError::Generic(
                    "Wrong number of arguments".to_string(),
                ))
            }
        })),
        capability_id: None,
        capability_permissions: vec![],
        args: vec![Value::Integer(10), Value::Integer(20)],
        config: config::MicroVMConfig::default(),
        runtime_context: None,
    };

    let result = provider.execute_program(context);
    assert!(result.is_ok());
    let execution_result = result.unwrap();
    if let Value::Integer(value) = execution_result.value {
        assert_eq!(value, 30);
    } else {
        panic!("Expected integer result, got {:?}", execution_result.value);
    }
}

#[test]
fn test_mock_provider_security_context() {
    let mut provider = providers::mock::MockMicroVMProvider::new();
    provider.initialize().unwrap();

    // Test with security context
    let runtime_context =
        RuntimeContext::controlled(vec!["ccos.math.add".to_string(), "ccos.math".to_string()]);

    let context = ExecutionContext {
        execution_id: "test-security".to_string(),
        program: Some(Program::RtfsSource("(+ 1 2)".to_string())),
        capability_id: None,
        capability_permissions: vec!["ccos.math.add".to_string()],
        args: vec![],
        config: config::MicroVMConfig::default(),
        runtime_context: Some(runtime_context),
    };

    let result = provider.execute_program(context);
    assert!(result.is_ok());

    let execution_result = result.unwrap();
    if let Value::Integer(value) = execution_result.value {
        assert_eq!(value, 3);
    } else {
        panic!("Expected integer result, got {:?}", execution_result.value);
    }
}

#[test]
fn test_mock_provider_metadata() {
    let mut provider = providers::mock::MockMicroVMProvider::new();
    provider.initialize().unwrap();

    let context = ExecutionContext {
        execution_id: "test-metadata".to_string(),
        program: Some(Program::RtfsSource("(+ 1 2)".to_string())),
        capability_id: None,
        capability_permissions: vec![],
        args: vec![],
        config: config::MicroVMConfig::default(),
        runtime_context: None,
    };

    let result = provider.execute_program(context);
    assert!(result.is_ok());

    let execution_result = result.unwrap();
    let metadata = execution_result.metadata;

    // Verify metadata fields
    assert!(metadata.duration > Duration::from_nanos(0));
    assert_eq!(metadata.memory_used_mb, 1);
    assert!(metadata.cpu_time > Duration::from_nanos(0));
    assert!(metadata.network_requests.is_empty());
    assert!(metadata.file_operations.is_empty());
}

#[test]
fn test_mock_provider_config_schema() {
    let provider = providers::mock::MockMicroVMProvider::new();
    let schema = provider.get_config_schema();

    // Verify schema structure
    assert!(schema.is_object());
    if let Some(obj) = schema.as_object() {
        assert!(obj.contains_key("type"));
        assert!(obj.contains_key("properties"));

        if let Some(properties) = obj.get("properties") {
            if let Some(properties_obj) = properties.as_object() {
                assert!(properties_obj.contains_key("mock_mode"));
            }
        }
    }
}

#[test]
fn test_mock_provider_error_handling() {
    let provider = providers::mock::MockMicroVMProvider::new();

    // Test execution without initialization
    let context = ExecutionContext {
        execution_id: "test-error".to_string(),
        program: Some(Program::RtfsSource("(+ 1 2)".to_string())),
        capability_id: None,
        capability_permissions: vec![],
        args: vec![],
        config: config::MicroVMConfig::default(),
        runtime_context: None,
    };

    let result = provider.execute_program(context);
    assert!(result.is_err());

    if let Err(RuntimeError::Generic(msg)) = result {
        assert!(msg.contains("not initialized"));
    } else {
        panic!("Expected Generic error, got {:?}", result);
    }

    // Test capability execution without initialization
    let context = ExecutionContext {
        execution_id: "test-capability-error".to_string(),
        program: None,
        capability_id: Some("ccos.math.add".to_string()),
        capability_permissions: vec![],
        args: vec![Value::Integer(1), Value::Integer(2)],
        config: config::MicroVMConfig::default(),
        runtime_context: None,
    };

    let result = provider.execute_capability(context);
    assert!(result.is_err());

    if let Err(RuntimeError::Generic(msg)) = result {
        assert!(msg.contains("not initialized"));
    } else {
        panic!("Expected Generic error, got {:?}", result);
    }
}

#[test]
fn test_mock_provider_multiple_executions() {
    let mut provider = providers::mock::MockMicroVMProvider::new();
    provider.initialize().unwrap();

    // Execute multiple programs
    for i in 0..5 {
        let context = ExecutionContext {
            execution_id: format!("test-multi-{}", i),
            program: Some(Program::RtfsSource(format!("(+ {} {})", i, i))),
            capability_id: None,
            capability_permissions: vec![],
            args: vec![],
            config: config::MicroVMConfig::default(),
            runtime_context: None,
        };

        let result = provider.execute_program(context);
        assert!(result.is_ok());

        let execution_result = result.unwrap();
        if let Value::Integer(value) = execution_result.value {
            assert_eq!(value, i + i);
        } else {
            panic!("Expected integer result, got {:?}", execution_result.value);
        }
    }
}

#[test]
fn test_mock_provider_cleanup_and_reinitialize() {
    let mut provider = providers::mock::MockMicroVMProvider::new();

    // Initialize
    assert!(provider.initialize().is_ok());
    assert!(provider.initialized);

    // Cleanup
    assert!(provider.cleanup().is_ok());
    assert!(!provider.initialized);

    // Re-initialize
    assert!(provider.initialize().is_ok());
    assert!(provider.initialized);

    // Execute after re-initialization
    let context = ExecutionContext {
        execution_id: "test-reinit".to_string(),
        program: Some(Program::RtfsSource("(+ 5 5)".to_string())),
        capability_id: None,
        capability_permissions: vec![],
        args: vec![],
        config: config::MicroVMConfig::default(),
        runtime_context: None,
    };

    let result = provider.execute_program(context);
    assert!(result.is_ok());

    let execution_result = result.unwrap();
    if let Value::Integer(value) = execution_result.value {
        assert_eq!(value, 10);
    } else {
        panic!("Expected integer result, got {:?}", execution_result.value);
    }
}
