//! MicroVM Performance Tests
//!
//! Simple performance tests for MicroVM providers without requiring criterion.
//! These tests measure execution times and provide basic performance metrics.

use rtfs_compiler::runtime::microvm::*;
use std::time::Instant;

// Simple arithmetic program for basic performance testing
fn create_simple_arithmetic_program() -> Program {
    Program::RtfsSource("(+ 1 2)".to_string())
}

// Complex nested program for stress testing
fn create_complex_nested_program() -> Program {
    Program::RtfsSource(
        r#"
        (let [x 10
              y 20
              z (+ x y)]
          (* z (+ x y)))
    "#
        .to_string(),
    )
}

// Program with multiple capability calls
fn create_capability_heavy_program() -> Program {
    Program::RtfsSource(
        r#"
        (do
          (call :math.add {:a 1 :b 2})
          (call :math.multiply {:a 3 :b 4})
          (call :string.concat {:a "hello" :b "world"}))
    "#
        .to_string(),
    )
}

#[test]
fn test_provider_initialization_performance() {
    println!("Testing provider initialization performance...");

    let start = Instant::now();
    for _ in 0..1000 {
        let _provider = providers::mock::MockMicroVMProvider::new();
    }
    let duration = start.elapsed();

    println!("Created 1000 providers in {:?}", duration);
    println!("Average time per provider: {:?}", duration / 1000);

    // Assert that initialization is reasonably fast (< 1ms per provider)
    assert!(duration.as_micros() < 1000 * 1000); // 1 second for 1000 providers
}

#[test]
fn test_program_execution_performance() {
    println!("Testing program execution performance...");

    let mut provider = providers::mock::MockMicroVMProvider::new();
    provider.initialize().unwrap();

    let simple_program = create_simple_arithmetic_program();
    let complex_program = create_complex_nested_program();
    let capability_program = create_capability_heavy_program();

    // Test simple arithmetic
    let start = Instant::now();
    for i in 0..100 {
        let context = ExecutionContext {
            execution_id: format!("simple-{}", i),
            program: Some(simple_program.clone()),
            capability_id: None,
            capability_permissions: vec![],
            args: vec![],
            config: config::MicroVMConfig::default(),
            runtime_context: None,
        };
        let _result = provider.execute_program(context);
    }
    let simple_duration = start.elapsed();
    println!("Simple arithmetic (100x): {:?}", simple_duration);
    println!("Average per execution: {:?}", simple_duration / 100);

    // Test complex nested
    let start = Instant::now();
    for i in 0..100 {
        let context = ExecutionContext {
            execution_id: format!("complex-{}", i),
            program: Some(complex_program.clone()),
            capability_id: None,
            capability_permissions: vec![],
            args: vec![],
            config: config::MicroVMConfig::default(),
            runtime_context: None,
        };
        let _result = provider.execute_program(context);
    }
    let complex_duration = start.elapsed();
    println!("Complex nested (100x): {:?}", complex_duration);
    println!("Average per execution: {:?}", complex_duration / 100);

    // Test capability heavy
    let start = Instant::now();
    for i in 0..100 {
        let context = ExecutionContext {
            execution_id: format!("capability-{}", i),
            program: Some(capability_program.clone()),
            capability_id: None,
            capability_permissions: vec![],
            args: vec![],
            config: config::MicroVMConfig::default(),
            runtime_context: None,
        };
        let _result = provider.execute_program(context);
    }
    let capability_duration = start.elapsed();
    println!("Capability heavy (100x): {:?}", capability_duration);
    println!("Average per execution: {:?}", capability_duration / 100);

    // Assert that executions complete in reasonable time
    assert!(simple_duration.as_millis() < 1000); // 1 second for 100 executions
    assert!(complex_duration.as_millis() < 1000);
    assert!(capability_duration.as_millis() < 1000);
}

#[test]
fn test_concurrent_execution_performance() {
    println!("Testing concurrent execution performance...");

    let program = create_simple_arithmetic_program();

    // Single-threaded sequential execution
    let start = Instant::now();
    for i in 0..10 {
        let mut provider = providers::mock::MockMicroVMProvider::new();
        provider.initialize().unwrap();
        let context = ExecutionContext {
            execution_id: format!("sequential-{}", i),
            program: Some(program.clone()),
            capability_id: None,
            capability_permissions: vec![],
            args: vec![],
            config: config::MicroVMConfig::default(),
            runtime_context: None,
        };
        let _result = provider.execute_program(context);
    }
    let sequential_duration = start.elapsed();
    println!("Sequential execution (10x): {:?}", sequential_duration);

    // Single provider, multiple executions
    let start = Instant::now();
    let mut provider = providers::mock::MockMicroVMProvider::new();
    provider.initialize().unwrap();
    for i in 0..10 {
        let context = ExecutionContext {
            execution_id: format!("single-{}", i),
            program: Some(program.clone()),
            capability_id: None,
            capability_permissions: vec![],
            args: vec![],
            config: config::MicroVMConfig::default(),
            runtime_context: None,
        };
        let _result = provider.execute_program(context);
    }
    let single_provider_duration = start.elapsed();
    println!("Single provider (10x): {:?}", single_provider_duration);

    // Log the performance comparison
    println!(
        "Single provider is {}x faster than sequential",
        sequential_duration.as_micros() as f64 / single_provider_duration.as_micros() as f64
    );

    // Both approaches should be reasonably fast
    assert!(sequential_duration.as_millis() < 100); // Less than 100ms for 10 executions
    assert!(single_provider_duration.as_millis() < 100); // Less than 100ms for 10 executions
}

#[test]
fn test_memory_usage_patterns() {
    println!("Testing memory usage patterns...");

    // Measure memory usage of provider creation
    let provider_size = std::mem::size_of::<providers::mock::MockMicroVMProvider>();
    println!("Provider struct size: {} bytes", provider_size);

    // Create multiple providers and measure
    let mut providers = Vec::new();
    for i in 0..100 {
        providers.push(providers::mock::MockMicroVMProvider::new());
        if i % 20 == 0 {
            println!("Created {} providers", i + 1);
        }
    }

    let total_memory = providers.len() * provider_size;
    println!("Total memory for 100 providers: {} bytes", total_memory);

    // Assert reasonable memory usage
    assert!(total_memory < 1024 * 1024); // Less than 1MB for 100 providers
}

#[test]
fn test_provider_lifecycle_performance() {
    println!("Testing provider lifecycle performance...");

    let program = create_simple_arithmetic_program();

    // Full lifecycle: create -> initialize -> execute -> cleanup
    let start = Instant::now();
    for i in 0..50 {
        let mut provider = providers::mock::MockMicroVMProvider::new();
        provider.initialize().unwrap();
        let context = ExecutionContext {
            execution_id: format!("lifecycle-{}", i),
            program: Some(program.clone()),
            capability_id: None,
            capability_permissions: vec![],
            args: vec![],
            config: config::MicroVMConfig::default(),
            runtime_context: None,
        };
        let _result = provider.execute_program(context);
        provider.cleanup().unwrap();
    }
    let lifecycle_duration = start.elapsed();
    println!("Full lifecycle (50x): {:?}", lifecycle_duration);
    println!("Average per lifecycle: {:?}", lifecycle_duration / 50);

    // Reinitialize cycle
    let start = Instant::now();
    let mut provider = providers::mock::MockMicroVMProvider::new();
    for i in 0..50 {
        provider.initialize().unwrap();
        provider.cleanup().unwrap();
    }
    let reinit_duration = start.elapsed();
    println!("Reinitialize cycle (50x): {:?}", reinit_duration);
    println!("Average per cycle: {:?}", reinit_duration / 50);

    // Assert reasonable performance
    assert!(lifecycle_duration.as_millis() < 5000); // 5 seconds for 50 lifecycles
    assert!(reinit_duration.as_millis() < 5000);
}

#[test]
fn test_error_handling_performance() {
    println!("Testing error handling performance...");

    let invalid_program = Program::RtfsSource("(invalid-function 1 2 3)".to_string());

    let mut provider = providers::mock::MockMicroVMProvider::new();
    provider.initialize().unwrap();

    let start = Instant::now();
    for i in 0..100 {
        let context = ExecutionContext {
            execution_id: format!("error-{}", i),
            program: Some(invalid_program.clone()),
            capability_id: None,
            capability_permissions: vec![],
            args: vec![],
            config: config::MicroVMConfig::default(),
            runtime_context: None,
        };
        let _result = provider.execute_program(context);
    }
    let error_duration = start.elapsed();
    println!("Error handling (100x): {:?}", error_duration);
    println!("Average per error: {:?}", error_duration / 100);

    // Assert that error handling is reasonably fast
    assert!(error_duration.as_millis() < 1000); // 1 second for 100 errors
}

#[test]
fn test_large_program_performance() {
    println!("Testing large program performance...");

    // Create a larger program with more operations
    let large_program = Program::RtfsSource(
        r#"
        (let [x 1
              y 2
              z 3
              w 4
              v 5]
          (do
            (+ x y)
            (* z w)
            (- v x)
            (+ y z)
            (* w v)))
    "#
        .to_string(),
    );

    let mut provider = providers::mock::MockMicroVMProvider::new();
    provider.initialize().unwrap();

    let start = Instant::now();
    for i in 0..50 {
        let context = ExecutionContext {
            execution_id: format!("large-{}", i),
            program: Some(large_program.clone()),
            capability_id: None,
            capability_permissions: vec![],
            args: vec![],
            config: config::MicroVMConfig::default(),
            runtime_context: None,
        };
        let _result = provider.execute_program(context);
    }
    let large_duration = start.elapsed();
    println!("Large program (50x): {:?}", large_duration);
    println!("Average per execution: {:?}", large_duration / 50);

    // Assert that larger programs still execute in reasonable time
    assert!(large_duration.as_millis() < 1000); // 1 second for 50 executions
}

#[test]
fn test_provider_metadata_performance() {
    println!("Testing provider metadata performance...");

    let provider = providers::mock::MockMicroVMProvider::new();

    let start = Instant::now();
    for _ in 0..1000 {
        let _name = provider.name();
        let _available = provider.is_available();
        let _config_schema = provider.get_config_schema();
    }
    let metadata_duration = start.elapsed();
    println!("Metadata access (1000x): {:?}", metadata_duration);
    println!("Average per access: {:?}", metadata_duration / 1000);

    // Assert that metadata access is reasonably fast
    assert!(metadata_duration.as_millis() < 10); // 10ms for 1000 accesses
}
