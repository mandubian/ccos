//! MicroVM Performance Benchmarks
//!
//! This benchmark suite measures the performance characteristics of MicroVM providers:
//! - Execution time for different program types
//! - Memory usage patterns
//! - Startup and teardown overhead
//! - Concurrent execution performance
//! - Resource cleanup efficiency

use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};
use rtfs_compiler::runtime::microvm::*;
use rtfs_compiler::runtime::security::RuntimeContext;
use rtfs_compiler::runtime::values::Value;
use rtfs_compiler::runtime::microvm::Program;
use std::time::Instant;
use std::sync::Arc;
use tokio::runtime::Runtime;

// Simple arithmetic program for basic performance testing
fn create_simple_arithmetic_program() -> Program {
    Program::RtfsSource("(+ 1 2)".to_string())
}

// Complex nested program for stress testing
fn create_complex_nested_program() -> Program {
    Program::RtfsSource(r#"
        (let [x 10
              y 20
              z (+ x y)]
          (* z (+ x y)))
    "#.to_string())
}

// Program with multiple capability calls
fn create_capability_heavy_program() -> Program {
    Program::RtfsSource(r#"
        (do
          (call :math.add {:a 1 :b 2})
          (call :math.multiply {:a 3 :b 4})
          (call :string.concat {:a "hello" :b "world"}))
    "#.to_string())
}

fn benchmark_provider_initialization(c: &mut Criterion) {
    let mut group = c.benchmark_group("provider_initialization");
    
    group.bench_function("mock_provider_new", |b| {
        b.iter(|| {
            black_box(providers::mock::MockMicroVMProvider::new());
        });
    });
    
    group.bench_function("mock_provider_init", |b| {
        b.iter(|| {
            let mut provider = providers::mock::MockMicroVMProvider::new();
            black_box(provider.initialize());
        });
    });
    
    group.finish();
}

fn benchmark_program_execution(c: &mut Criterion) {
    let mut group = c.benchmark_group("program_execution");
    
    let simple_program = create_simple_arithmetic_program();
    let complex_program = create_complex_nested_program();
    let capability_program = create_capability_heavy_program();
    
    group.bench_function("simple_arithmetic", |b| {
        b.iter(|| {
            let mut provider = providers::mock::MockMicroVMProvider::new();
            provider.initialize().unwrap();
            black_box(provider.execute_program(&simple_program, &RuntimeContext::default()));
        });
    });
    
    group.bench_function("complex_nested", |b| {
        b.iter(|| {
            let mut provider = providers::mock::MockMicroVMProvider::new();
            provider.initialize().unwrap();
            black_box(provider.execute_program(&complex_program, &RuntimeContext::default()));
        });
    });
    
    group.bench_function("capability_heavy", |b| {
        b.iter(|| {
            let mut provider = providers::mock::MockMicroVMProvider::new();
            provider.initialize().unwrap();
            black_box(provider.execute_program(&capability_program, &RuntimeContext::default()));
        });
    });
    
    group.finish();
}

fn benchmark_concurrent_execution(c: &mut Criterion) {
    let mut group = c.benchmark_group("concurrent_execution");
    
    let program = create_simple_arithmetic_program();
    
    group.bench_function("single_thread_10_executions", |b| {
        b.iter(|| {
            let mut provider = providers::mock::MockMicroVMProvider::new();
            provider.initialize().unwrap();
            
            for _ in 0..10 {
                black_box(provider.execute_program(&program, &RuntimeContext::default()));
            }
        });
    });
    
    group.bench_function("async_concurrent_10_executions", |b| {
        let rt = Runtime::new().unwrap();
        b.iter(|| {
            rt.block_on(async {
                let mut handles = Vec::new();
                
                for _ in 0..10 {
                    let program = program.clone();
                    let handle = tokio::spawn(async move {
                        let mut provider = providers::mock::MockMicroVMProvider::new();
                        provider.initialize().unwrap();
                        provider.execute_program(&program, &RuntimeContext::default())
                    });
                    handles.push(handle);
                }
                
                for handle in handles {
                    black_box(handle.await.unwrap());
                }
            });
        });
    });
    
    group.finish();
}

fn benchmark_memory_usage(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory_usage");
    
    group.bench_function("provider_creation_memory", |b| {
        b.iter(|| {
            let start_memory = get_memory_usage();
            let provider = black_box(providers::mock::MockMicroVMProvider::new());
            let end_memory = get_memory_usage();
            black_box(end_memory - start_memory);
        });
    });
    
    group.bench_function("program_execution_memory", |b| {
        b.iter(|| {
            let program = create_complex_nested_program();
            let start_memory = get_memory_usage();
            
            let mut provider = providers::mock::MockMicroVMProvider::new();
            provider.initialize().unwrap();
            black_box(provider.execute_program(&program, &RuntimeContext::default()));
            
            let end_memory = get_memory_usage();
            black_box(end_memory - start_memory);
        });
    });
    
    group.finish();
}

fn benchmark_provider_lifecycle(c: &mut Criterion) {
    let mut group = c.benchmark_group("provider_lifecycle");
    
    group.bench_function("full_lifecycle", |b| {
        b.iter(|| {
            let mut provider = providers::mock::MockMicroVMProvider::new();
            provider.initialize().unwrap();
            
            let program = create_simple_arithmetic_program();
            black_box(provider.execute_program(&program, &RuntimeContext::default()));
            
            provider.cleanup().unwrap();
        });
    });
    
    group.bench_function("reinitialize_cycle", |b| {
        b.iter(|| {
            let mut provider = providers::mock::MockMicroVMProvider::new();
            
            for _ in 0..5 {
                provider.initialize().unwrap();
                provider.cleanup().unwrap();
            }
        });
    });
    
    group.finish();
}

// Helper function to get current memory usage (approximate)
fn get_memory_usage() -> usize {
    // This is a simplified implementation
    // In a real benchmark, you might want to use more sophisticated memory tracking
    std::mem::size_of::<providers::mock::MockMicroVMProvider>()
}

fn benchmark_error_handling_performance(c: &mut Criterion) {
    let mut group = c.benchmark_group("error_handling");
    
    let invalid_program = Program::RtfsSource("(invalid-function 1 2 3)".to_string());
    
    group.bench_function("error_program_execution", |b| {
        b.iter(|| {
            let mut provider = providers::mock::MockMicroVMProvider::new();
            provider.initialize().unwrap();
            black_box(provider.execute_program(&invalid_program, &RuntimeContext::default()));
        });
    });
    
    group.finish();
}

criterion_group!(
    benches,
    benchmark_provider_initialization,
    benchmark_program_execution,
    benchmark_concurrent_execution,
    benchmark_memory_usage,
    benchmark_provider_lifecycle,
    benchmark_error_handling_performance
);

criterion_main!(benches);
