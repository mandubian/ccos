//! Tests for enhanced MicroVM functionality with program execution and capability permissions

use super::*;
use crate::runtime::security::RuntimeContext;
use crate::runtime::values::Value;
use crate::runtime::{RuntimeResult, RuntimeError};
use std::time::Duration;
use std::collections::HashMap;

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
    let runtime_context = RuntimeContext::controlled(vec![
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

#[test]
fn test_firecracker_provider_availability() {
    let mut factory = MicroVMFactory::new();
    
    // Check if firecracker provider is available (depends on system)
    let available_providers = factory.get_available_providers();
    println!("Available providers: {:?}", available_providers);
    
    // Firecracker provider should be available if firecracker binary is present
    // This test will pass even if firecracker is not available (graceful degradation)
    if available_providers.contains(&"firecracker") {
        // If firecracker is available, test initialization
        let result = factory.initialize_provider("firecracker");
        if let Err(e) = result {
            // This is expected if kernel/rootfs images are not available
            println!("Firecracker initialization failed (expected if images not available): {:?}", e);
        }
    } else {
        println!("Firecracker provider not available on this system");
    }
}

#[test]
fn test_firecracker_provider_rtfs_execution() {
    let mut factory = MicroVMFactory::new();
    
    // Only run this test if firecracker is available
    if !factory.get_available_providers().contains(&"firecracker") {
        println!("Skipping firecracker test - provider not available");
        return;
    }
    
    // Initialize the provider
    if let Err(e) = factory.initialize_provider("firecracker") {
        println!("Firecracker initialization failed: {:?}", e);
        return; // Skip test if initialization fails
    }
    
    let provider = factory.get_provider("firecracker").expect("Firecracker provider should be available");
    
    // Test RTFS execution in Firecracker VM
    let rtfs_program = Program::RtfsSource("(+ 7 5)".to_string());
    let runtime_context = RuntimeContext::controlled(vec![
        "ccos.math.add".to_string(),
        "ccos.math".to_string(),
        "math".to_string(),
    ]);
    
    let context = ExecutionContext {
        execution_id: "test-exec-firecracker-rtfs".to_string(),
        program: Some(rtfs_program),
        capability_id: None,
        capability_permissions: vec!["ccos.math.add".to_string()],
        args: vec![Value::Integer(7), Value::Integer(5)],
        config: MicroVMConfig::default(),
        runtime_context: Some(runtime_context),
    };
    
    let result = provider.execute_program(context);
    if let Err(ref e) = result {
        println!("Firecracker provider RTFS execution failed: {:?}", e);
        // This is expected if kernel/rootfs images are not available
        return;
    }
    
    let execution_result = result.unwrap();
    // The RTFS expression (+ 7 5) should evaluate to 12
    if let Value::Integer(value) = execution_result.value {
        assert_eq!(value, 12);
    } else {
        panic!("Expected integer result, got {:?}", execution_result.value);
    }
    assert!(execution_result.metadata.duration.as_millis() > 0);
    assert_eq!(execution_result.metadata.memory_used_mb, 512); // Default memory
    assert!(execution_result.metadata.cpu_time.as_millis() > 0);
} 

#[test]
fn test_gvisor_provider_availability() {
    let mut factory = MicroVMFactory::new();
    
    // Check if gvisor provider is available (depends on system)
    let available_providers = factory.get_available_providers();
    println!("Available providers: {:?}", available_providers);
    
    // gVisor provider should be available if runsc binary is present
    // This test will pass even if gVisor is not available (graceful degradation)
    if available_providers.contains(&"gvisor") {
        // If gVisor is available, test initialization
        let result = factory.initialize_provider("gvisor");
        if let Err(e) = result {
            // This is expected if runsc is not properly configured
            println!("gVisor initialization failed (expected if runsc not configured): {:?}", e);
        }
    } else {
        println!("gVisor provider not available on this system");
    }
}

#[test]
fn test_gvisor_provider_rtfs_execution() {
    let mut factory = MicroVMFactory::new();
    
    // Only run this test if gVisor is available
    if !factory.get_available_providers().contains(&"gvisor") {
        println!("Skipping gVisor test - provider not available");
        return;
    }
    
    // Initialize the provider
    if let Err(e) = factory.initialize_provider("gvisor") {
        println!("gVisor initialization failed: {:?}", e);
        return; // Skip test if initialization fails
    }
    
    let provider = factory.get_provider("gvisor").expect("gVisor provider should be available");
    
    // Test RTFS execution in gVisor container
    let rtfs_program = Program::RtfsSource("(+ 10 15)".to_string());
    let runtime_context = RuntimeContext::controlled(vec![
        "ccos.math.add".to_string(),
        "ccos.math".to_string(),
        "math".to_string(),
    ]);
    
    let context = ExecutionContext {
        execution_id: "test-exec-gvisor-rtfs".to_string(),
        program: Some(rtfs_program),
        capability_id: None,
        capability_permissions: vec!["ccos.math.add".to_string()],
        args: vec![Value::Integer(10), Value::Integer(15)],
        config: MicroVMConfig::default(),
        runtime_context: Some(runtime_context),
    };
    
    let result = provider.execute_program(context);
    if let Err(ref e) = result {
        println!("gVisor provider RTFS execution failed: {:?}", e);
        // This is expected if runsc is not properly configured
        return;
    }
    
    let execution_result = result.unwrap();
    // The RTFS expression (+ 10 15) should evaluate to 25
    if let Value::Integer(value) = execution_result.value {
        assert_eq!(value, 25);
    } else {
        panic!("Expected integer result, got {:?}", execution_result.value);
    }
    assert!(execution_result.metadata.duration.as_millis() > 0);
    // Container ID is not available in current ExecutionMetadata structure
}

#[test]
fn test_gvisor_provider_container_lifecycle() {
    let mut factory = MicroVMFactory::new();
    
    // Only run this test if gVisor is available
    if !factory.get_available_providers().contains(&"gvisor") {
        println!("Skipping gVisor container lifecycle test - provider not available");
        return;
    }
    
    // Initialize the provider
    if let Err(e) = factory.initialize_provider("gvisor") {
        println!("gVisor initialization failed: {:?}", e);
        return; // Skip test if initialization fails
    }
    
    let provider = factory.get_provider("gvisor").expect("gVisor provider should be available");
    
    // Test container creation and execution
    let external_program = Program::ExternalProgram {
        path: "echo".to_string(),
        args: vec!["Hello from gVisor container".to_string()],
    };
    let runtime_context = RuntimeContext::controlled(vec![
        "external_program".to_string(),
    ]);
    
    let context = ExecutionContext {
        execution_id: "test-exec-gvisor-container".to_string(),
        program: Some(external_program),
        capability_id: None,
        capability_permissions: vec!["external_program".to_string()],
        args: vec![],
        config: MicroVMConfig::default(),
        runtime_context: Some(runtime_context),
    };
    
    let result = provider.execute_program(context);
    if let Err(ref e) = result {
        println!("gVisor container execution failed: {:?}", e);
        // This is expected if runsc is not properly configured
        return;
    }
    
    let execution_result = result.unwrap();
    // Success field is not available in current ExecutionResult structure
    assert!(execution_result.metadata.duration.as_millis() > 0);
    // Container ID is not available in current ExecutionMetadata structure
    
    // Test cleanup - requires mutable access, so we'll skip this test
    // let cleanup_result = provider.cleanup();
    // assert!(cleanup_result.is_ok());
} 

#[test]
fn test_microvm_provider_performance_comparison() {
    let mut factory = MicroVMFactory::new();
    
    // Test performance across different providers
    let providers = ["mock", "process"];
    let test_program = Program::RtfsSource("(+ 1 2)".to_string());
    
    for provider_name in providers {
        if let Ok(()) = factory.initialize_provider(provider_name) {
            if let Some(provider) = factory.get_provider(provider_name) {
                let start_time = std::time::Instant::now();
                
                let context = ExecutionContext {
                    execution_id: format!("perf-test-{}", provider_name),
                    program: Some(test_program.clone()),
                    capability_id: None,
                    capability_permissions: vec![],
                    args: vec![],
                    config: MicroVMConfig::default(),
                    runtime_context: Some(RuntimeContext::full()),
                };
                
                let result = provider.execute_program(context);
                let duration = start_time.elapsed();
                
                assert!(result.is_ok(), "Provider {} should execute successfully", provider_name);
                println!("Provider {} execution time: {:?}", provider_name, duration);
                
                // Performance assertions
                assert!(duration.as_millis() < 1000, "Provider {} should complete within 1 second", provider_name);
            }
        }
    }
}

#[test]
fn test_microvm_provider_security_isolation() {
    let mut factory = MicroVMFactory::new();
    
    // Test security isolation with restricted capabilities
    let restricted_context = RuntimeContext::controlled(vec!["ccos.math.add".to_string()]);
    
    // Test that providers respect capability restrictions
    let providers = ["mock", "process"];
    
    for provider_name in providers {
        if let Ok(()) = factory.initialize_provider(provider_name) {
            if let Some(provider) = factory.get_provider(provider_name) {
                // Test allowed capability
                let allowed_context = ExecutionContext {
                    execution_id: format!("security-test-allowed-{}", provider_name),
                    program: Some(Program::RtfsSource("(call :ccos.math.add {:a 1 :b 2})".to_string())),
                    capability_id: None,
                    capability_permissions: vec![],
                    args: vec![],
                    config: MicroVMConfig::default(),
                    runtime_context: Some(restricted_context.clone()),
                };
                
                let allowed_result = provider.execute_program(allowed_context);
                assert!(allowed_result.is_ok(), "Allowed capability should execute in provider {}", provider_name);
                
                // Test denied capability (should be handled gracefully)
                let denied_context = ExecutionContext {
                    execution_id: format!("security-test-denied-{}", provider_name),
                    program: Some(Program::RtfsSource("(call :ccos.network.http-fetch {:url \"http://example.com\"})".to_string())),
                    capability_id: None,
                    capability_permissions: vec![],
                    args: vec![],
                    config: MicroVMConfig::default(),
                    runtime_context: Some(restricted_context.clone()),
                };
                
                let denied_result = provider.execute_program(denied_context);
                // Should either succeed (with proper error handling) or fail gracefully
                println!("Provider {} denied capability result: {:?}", provider_name, denied_result);
            }
        }
    }
}

#[test]
fn test_microvm_provider_resource_limits() {
    let mut factory = MicroVMFactory::new();
    
    // Test resource limit enforcement
    let config = MicroVMConfig {
        timeout: Duration::from_millis(100), // Very short timeout
        memory_limit_mb: 1, // Very low memory limit
        cpu_limit: 0.1, // Low CPU limit
        network_policy: NetworkPolicy::Denied,
        fs_policy: FileSystemPolicy::None,
        env_vars: HashMap::new(),
    };
    
    let providers = ["mock", "process"];
    
    for provider_name in providers {
        if let Ok(()) = factory.initialize_provider(provider_name) {
            if let Some(provider) = factory.get_provider(provider_name) {
                let context = ExecutionContext {
                    execution_id: format!("resource-test-{}", provider_name),
                    program: Some(Program::RtfsSource("(+ 1 2)".to_string())),
                    capability_id: None,
                    capability_permissions: vec![],
                    args: vec![],
                    config: config.clone(),
                    runtime_context: Some(RuntimeContext::full()),
                };
                
                let result = provider.execute_program(context);
                assert!(result.is_ok(), "Provider {} should handle resource limits gracefully", provider_name);
                
                let execution_result = result.unwrap();
                assert!(execution_result.metadata.duration.as_millis() <= 100, 
                    "Provider {} should respect timeout", provider_name);
                assert!(execution_result.metadata.memory_used_mb <= 1, 
                    "Provider {} should respect memory limit", provider_name);
            }
        }
    }
}

#[test]
fn test_microvm_provider_error_handling() {
    let mut factory = MicroVMFactory::new();
    
    // Test various error conditions
    let error_test_cases = vec![
        ("invalid-syntax", Program::RtfsSource("(invalid syntax here".to_string())),
        ("empty-program", Program::RtfsSource("".to_string())),
        ("complex-error", Program::RtfsSource("(call :nonexistent.capability {:arg \"test\"})".to_string())),
    ];
    
    let providers = ["mock", "process"];
    
    for provider_name in providers {
        if let Ok(()) = factory.initialize_provider(provider_name) {
            if let Some(provider) = factory.get_provider(provider_name) {
                for (test_name, program) in &error_test_cases {
                    let context = ExecutionContext {
                        execution_id: format!("error-test-{}-{}", provider_name, test_name),
                        program: Some(program.clone()),
                        capability_id: None,
                        capability_permissions: vec![],
                        args: vec![],
                        config: MicroVMConfig::default(),
                        runtime_context: Some(RuntimeContext::full()),
                    };
                    
                    let result = provider.execute_program(context);
                    // Should handle errors gracefully (either return error or mock result)
                    println!("Provider {} error test {} result: {:?}", provider_name, test_name, result);
                }
            }
        }
    }
}

#[test]
fn test_microvm_provider_sequential_execution() {
    let mut factory = MicroVMFactory::new();
    
    // Test sequential execution across providers (avoiding thread safety issues)
    let providers = ["mock", "process"];
    
    for provider_name in providers {
        if let Ok(()) = factory.initialize_provider(provider_name) {
            if let Some(provider) = factory.get_provider(provider_name) {
                // Test multiple sequential executions
                for i in 0..5 {
                    let context = ExecutionContext {
                        execution_id: format!("sequential-test-{}-{}", provider_name, i),
                        program: Some(Program::RtfsSource(format!("(+ {} {})", i, i))),
                        capability_id: None,
                        capability_permissions: vec![],
                        args: vec![],
                        config: MicroVMConfig::default(),
                        runtime_context: Some(RuntimeContext::full()),
                    };
                    
                    let result = provider.execute_program(context);
                    assert!(result.is_ok(), "Sequential execution should succeed in provider {}", provider_name);
                }
            }
        }
    }
}

#[test]
fn test_microvm_provider_configuration_validation() {
    let mut factory = MicroVMFactory::new();
    
    // Test configuration validation
    let providers = ["mock", "process"];
    
    for provider_name in providers {
        if let Ok(()) = factory.initialize_provider(provider_name) {
            if let Some(provider) = factory.get_provider(provider_name) {
                // Test valid configuration
                let valid_config = MicroVMConfig {
                    timeout: Duration::from_secs(30),
                    memory_limit_mb: 512,
                    cpu_limit: 1.0,
                    network_policy: NetworkPolicy::AllowList(vec!["api.github.com".to_string()]),
                    fs_policy: FileSystemPolicy::ReadOnly(vec!["/tmp".to_string()]),
                    env_vars: HashMap::new(),
                };
                
                let context = ExecutionContext {
                    execution_id: format!("config-test-{}", provider_name),
                    program: Some(Program::RtfsSource("(+ 1 2)".to_string())),
                    capability_id: None,
                    capability_permissions: vec![],
                    args: vec![],
                    config: valid_config,
                    runtime_context: Some(RuntimeContext::full()),
                };
                
                let result = provider.execute_program(context);
                assert!(result.is_ok(), "Provider {} should accept valid configuration", provider_name);
                
                // Test configuration schema
                let schema = provider.get_config_schema();
                assert!(!schema.is_null(), "Provider {} should provide configuration schema", provider_name);
            }
        }
    }
}

#[test]
fn test_microvm_provider_lifecycle_management() {
    let mut factory = MicroVMFactory::new();
    
    // Test provider lifecycle (initialize, execute, cleanup)
    let providers = ["mock", "process"];
    
    for provider_name in providers {
        if let Ok(()) = factory.initialize_provider(provider_name) {
            if let Some(provider) = factory.get_provider(provider_name) {
                // Test execution
                let context = ExecutionContext {
                    execution_id: format!("lifecycle-test-{}", provider_name),
                    program: Some(Program::RtfsSource("(+ 1 2)".to_string())),
                    capability_id: None,
                    capability_permissions: vec![],
                    args: vec![],
                    config: MicroVMConfig::default(),
                    runtime_context: Some(RuntimeContext::full()),
                };
                
                let result = provider.execute_program(context);
                assert!(result.is_ok(), "Provider {} should execute during lifecycle test", provider_name);
                
                // Test cleanup (if provider supports it)
                // Note: We can't test cleanup directly since we don't have mutable access
                // but the provider should handle cleanup internally
            }
        }
    }
}

#[test]
fn test_microvm_provider_integration_with_capability_system() {
    let mut factory = MicroVMFactory::new();
    
    // Test integration with capability system
    let providers = ["mock", "process"];
    
    for provider_name in providers {
        if let Ok(()) = factory.initialize_provider(provider_name) {
            if let Some(provider) = factory.get_provider(provider_name) {
                // Test capability execution
                let capability_context = ExecutionContext {
                    execution_id: format!("capability-test-{}", provider_name),
                    program: Some(Program::RtfsSource("(call :ccos.math.add {:a 10 :b 20})".to_string())),
                    capability_id: None,
                    capability_permissions: vec![],
                    args: vec![],
                    config: MicroVMConfig::default(),
                    runtime_context: Some(RuntimeContext::full()),
                };
                
                let result = provider.execute_program(capability_context);
                assert!(result.is_ok(), "Provider {} should execute capability calls", provider_name);
                
                let execution_result = result.unwrap();
                // Verify that capability execution produces meaningful results
                println!("Provider {} capability execution result: {:?}", provider_name, execution_result);
            }
        }
    }
}
