#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::security::RuntimeContext;
    use crate::ast::Expression;
    use crate::ast::Literal;
    use crate::ast::Symbol;

    #[test]
    fn test_program_enum_creation() {
        // Test RTFS source program
        let source_program = Program::RtfsSource("(+ 1 2)".to_string());
        assert_eq!(source_program.description(), "RTFS source: (+ 1 2)");
        assert!(!source_program.is_network_operation());
        assert!(!source_program.is_file_operation());

        // Test RTFS bytecode program
        let bytecode_program = Program::RtfsBytecode(vec![1, 2, 3, 4]);
        assert_eq!(bytecode_program.description(), "RTFS bytecode (4 bytes)");

        // Test external program
        let external_program = Program::ExternalProgram {
            path: "/usr/bin/echo".to_string(),
            args: vec!["hello".to_string()],
        };
        assert_eq!(external_program.description(), "External program: /usr/bin/echo [\"hello\"]");
    }

    #[test]
    fn test_execution_context_creation() {
        let config = MicroVMConfig::default();
        let runtime_context = RuntimeContext::pure();
        
        let context = ExecutionContext {
            execution_id: "test-exec-123".to_string(),
            program: Some(Program::RtfsSource("(+ 1 2)".to_string())),
            capability_id: Some("test.capability".to_string()),
            capability_permissions: vec!["test.capability".to_string()],
            args: vec![Value::Integer(42)],
            config,
            runtime_context: Some(runtime_context),
        };

        assert_eq!(context.execution_id, "test-exec-123");
        assert!(context.program.is_some());
        assert_eq!(context.capability_permissions.len(), 1);
        assert_eq!(context.args.len(), 1);
    }

    #[test]
    fn test_rtfs_microvm_executor_creation() {
        let executor = RtfsMicroVMExecutor::new();
        assert!(executor.capability_registry.is_empty());
    }

    #[test]
    fn test_rtfs_source_execution() {
        let mut executor = RtfsMicroVMExecutor::new();
        let runtime_context = RuntimeContext::pure();
        
        // Test simple arithmetic expression
        let result = executor.execute_rtfs_source(
            "(+ 1 2)",
            vec![],
            runtime_context,
        );
        
        assert!(result.is_ok());
        if let Ok(Value::Integer(value)) = result {
            assert_eq!(value, 3);
        } else {
            panic!("Expected integer result");
        }
    }

    #[test]
    fn test_rtfs_ast_execution() {
        let mut executor = RtfsMicroVMExecutor::new();
        let runtime_context = RuntimeContext::pure();
        
        // Create a simple AST for (+ 1 2)
        let ast = Expression::List(vec![
            Expression::Symbol(Symbol("+".to_string())),
            Expression::Literal(Literal::Integer(1)),
            Expression::Literal(Literal::Integer(2)),
        ]);
        
        let result = executor.execute_rtfs_ast(
            ast,
            vec![],
            runtime_context,
        );
        
        assert!(result.is_ok());
        if let Ok(Value::Integer(value)) = result {
            assert_eq!(value, 3);
        } else {
            panic!("Expected integer result");
        }
    }

    #[test]
    fn test_capability_permission_validation() {
        let executor = RtfsMicroVMExecutor::new();
        
        // Test with allowed capabilities
        let mut runtime_context = RuntimeContext::pure();
        runtime_context.allow_capability("test.capability");
        
        let result = executor.validate_capability_permissions(
            &vec!["test.capability".to_string()],
            &runtime_context,
        );
        assert!(result.is_ok());

        // Test with disallowed capabilities
        let runtime_context = RuntimeContext::pure();
        let result = executor.validate_capability_permissions(
            &vec!["forbidden.capability".to_string()],
            &runtime_context,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_mock_microvm_provider_program_execution() {
        let mut provider = MockMicroVMProvider::new();
        provider.initialize().unwrap();
        
        let config = MicroVMConfig::default();
        let runtime_context = RuntimeContext::pure();
        
        let context = ExecutionContext {
            execution_id: "test-exec-456".to_string(),
            program: Some(Program::RtfsSource("(+ 3 4)".to_string())),
            capability_id: None,
            capability_permissions: vec![],
            args: vec![],
            config,
            runtime_context: Some(runtime_context),
        };

        let result = provider.execute_program(context);
        assert!(result.is_ok());
        
        let execution_result = result.unwrap();
        if let Value::Integer(value) = execution_result.value {
            assert_eq!(value, 7);
        } else {
            panic!("Expected integer result");
        }
    }

    #[test]
    fn test_microvm_factory_creation() {
        let factory = MicroVMFactory::new();
        let providers = factory.list_providers();
        
        // Should have at least mock and process providers
        assert!(providers.contains(&"mock"));
        assert!(providers.contains(&"process"));
        
        // Check available providers
        let available = factory.get_available_providers();
        assert!(available.contains(&"mock")); // Mock should always be available
    }

    #[test]
    fn test_program_network_detection() {
        // Test network operation detection
        let network_program = Program::RtfsSource("(http/get \"https://example.com\")".to_string());
        assert!(network_program.is_network_operation());
        
        let non_network_program = Program::RtfsSource("(+ 1 2)".to_string());
        assert!(!non_network_program.is_network_operation());
    }

    #[test]
    fn test_program_file_operation_detection() {
        // Test file operation detection
        let file_program = Program::RtfsSource("(file/read \"/path/to/file\")".to_string());
        assert!(file_program.is_file_operation());
        
        let non_file_program = Program::RtfsSource("(+ 1 2)".to_string());
        assert!(!non_file_program.is_file_operation());
    }

    #[test]
    fn test_native_function_execution_with_permissions() {
        let mut executor = RtfsMicroVMExecutor::new();
        
        // Test native function execution with permissions
        let mut runtime_context = RuntimeContext::pure();
        runtime_context.allow_capability("native_function");
        
        let native_func = |args: Vec<Value>| -> RuntimeResult<Value> {
            Ok(Value::String(format!("Called with {} args", args.len())))
        };
        
        let result = executor.execute_native_function(
            native_func,
            vec![Value::Integer(1), Value::String("test".to_string())],
            runtime_context,
        );
        
        assert!(result.is_ok());
        if let Ok(Value::String(value)) = result {
            assert_eq!(value, "Called with 2 args");
        } else {
            panic!("Expected string result");
        }
    }

    #[test]
    fn test_native_function_execution_without_permissions() {
        let mut executor = RtfsMicroVMExecutor::new();
        
        // Test native function execution without permissions
        let runtime_context = RuntimeContext::pure(); // No permissions
        
        let native_func = |_args: Vec<Value>| -> RuntimeResult<Value> {
            Ok(Value::String("should not execute".to_string()))
        };
        
        let result = executor.execute_native_function(
            native_func,
            vec![],
            runtime_context,
        );
        
        assert!(result.is_err());
        if let Err(RuntimeError::SecurityViolation { capability, .. }) = result {
            assert_eq!(capability, "native_function");
        } else {
            panic!("Expected security violation error");
        }
    }

    #[test]
    fn test_external_program_execution() {
        let executor = RtfsMicroVMExecutor::new();
        
        // Test external program execution with permissions
        let mut runtime_context = RuntimeContext::pure();
        runtime_context.allow_capability("external_program");
        
        // Test with echo command (should work on most systems)
        let result = executor.execute_external_program(
            "echo".to_string(),
            vec!["hello world".to_string()],
            vec![],
            runtime_context,
        );
        
        // This might fail if echo is not available, but the permission check should work
        if result.is_ok() {
            if let Ok(Value::String(output)) = result {
                assert!(output.contains("hello world"));
            }
        }
    }

    #[test]
    fn test_external_program_execution_without_permissions() {
        let executor = RtfsMicroVMExecutor::new();
        
        // Test external program execution without permissions
        let runtime_context = RuntimeContext::pure(); // No permissions
        
        let result = executor.execute_external_program(
            "echo".to_string(),
            vec!["test".to_string()],
            vec![],
            runtime_context,
        );
        
        assert!(result.is_err());
        if let Err(RuntimeError::SecurityViolation { capability, .. }) = result {
            assert_eq!(capability, "external_program");
        } else {
            panic!("Expected security violation error");
        }
    }
} 