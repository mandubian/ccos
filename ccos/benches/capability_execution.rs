use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};
use ccos::capabilities::{CapabilityRegistry, CapabilityExecutionPolicy};
use rtfs::runtime::security::RuntimeContext;
use rtfs::runtime::values::Value;

/// Benchmark capability registration and lookup
fn benchmark_capability_registration(c: &mut Criterion) {
    let mut group = c.benchmark_group("capability_registration");
    
    group.bench_function("create_registry", |b| {
        b.iter(|| {
            CapabilityRegistry::new()
        });
    });
    
    group.bench_function("register_providers", |b| {
        b.iter(|| {
            let mut registry = CapabilityRegistry::new();
            registry.set_execution_policy(CapabilityExecutionPolicy::InlineDev);
            registry
        });
    });
    
    group.finish();
}

/// Benchmark capability execution for different providers
fn benchmark_capability_execution(c: &mut Criterion) {
    let mut group = c.benchmark_group("capability_execution");
    
    let mut registry = CapabilityRegistry::new();
    registry.set_execution_policy(CapabilityExecutionPolicy::InlineDev);
    
    let context = RuntimeContext::controlled(vec![
        "ccos.io.file-exists".to_string(),
        "ccos.json.parse".to_string(),
        "ccos.json.stringify".to_string(),
    ]);

    // Benchmark JSON parsing
    let json_input = Value::String(r#"{"name":"test","count":42,"enabled":true}"#.to_string());
    group.bench_function("json_parse", |b| {
        b.iter(|| {
            registry.execute_capability_with_microvm(
                "ccos.json.parse",
                vec![black_box(json_input.clone())],
                Some(&context),
            )
        });
    });

    // Benchmark JSON stringify
    let map_value = Value::Map(vec![
        (rtfs::ast::MapKey::String("key".to_string()), Value::String("value".to_string())),
        (rtfs::ast::MapKey::String("number".to_string()), Value::Integer(42)),
    ].into_iter().collect());
    
    group.bench_function("json_stringify", |b| {
        b.iter(|| {
            registry.execute_capability_with_microvm(
                "ccos.json.stringify",
                vec![black_box(map_value.clone())],
                Some(&context),
            )
        });
    });
    
    group.finish();
}

/// Benchmark security validation overhead
fn benchmark_security_validation(c: &mut Criterion) {
    let mut group = c.benchmark_group("security_validation");
    
    let contexts = vec![
        ("full_access", RuntimeContext::full()),
        ("controlled_1_cap", RuntimeContext::controlled(vec!["ccos.json.parse".to_string()])),
        ("controlled_10_caps", RuntimeContext::controlled(
            (0..10).map(|i| format!("ccos.test.cap{}", i)).collect()
        )),
    ];
    
    for (name, context) in contexts {
        group.bench_with_input(BenchmarkId::from_parameter(name), &context, |b, ctx| {
            b.iter(|| {
                ctx.is_capability_allowed(black_box("ccos.json.parse"))
            });
        });
    }
    
    group.finish();
}

/// Benchmark value serialization/deserialization
fn benchmark_value_serialization(c: &mut Criterion) {
    let mut group = c.benchmark_group("value_serialization");
    
    let test_values = vec![
        ("simple_int", Value::Integer(42)),
        ("simple_string", Value::String("hello world".to_string())),
        ("small_vector", Value::Vector(vec![Value::Integer(1), Value::Integer(2), Value::Integer(3)])),
        ("small_map", Value::Map(vec![
            (rtfs::ast::MapKey::String("a".to_string()), Value::Integer(1)),
            (rtfs::ast::MapKey::String("b".to_string()), Value::Integer(2)),
        ].into_iter().collect())),
        ("large_vector", Value::Vector((0..100).map(Value::Integer).collect())),
    ];

    for (name, value) in test_values {
        group.bench_with_input(BenchmarkId::from_parameter(name), &value, |b, val| {
            b.iter(|| {
                // Benchmark the cost of cloning values
                black_box(val.clone())
            });
        });
    }
    
    group.finish();
}

criterion_group!(
    benches,
    benchmark_capability_registration,
    benchmark_capability_execution,
    benchmark_security_validation,
    benchmark_value_serialization,
);
criterion_main!(benches);

