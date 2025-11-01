use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};
use rtfs::parser::parse_expression;
use rtfs::runtime::evaluator::Evaluator;
use rtfs::runtime::host_interface::HostInterface;
use rtfs::runtime::pure_host::PureHost;
use rtfs::runtime::security::RuntimeContext;
use rtfs::runtime::ModuleRegistry;
use std::sync::Arc;

/// Benchmark RTFS parsing performance
fn benchmark_parsing(c: &mut Criterion) {
    let mut group = c.benchmark_group("parsing");
    
    let test_cases = vec![
        ("simple_int", "42"),
        ("simple_string", "\"hello world\""),
        ("arithmetic", "(+ 1 2 3 4 5)"),
        ("nested_arithmetic", "(+ (* 2 3) (- 10 5))"),
        ("let_binding", "(let [x 42 y 10] (+ x y))"),
        ("function_def", "(fn [x y] (+ x y))"),
        ("conditional", "(if (> 5 3) \"yes\" \"no\")"),
        ("map_literal", "{:name \"Alice\" :age 30}"),
        ("vector", "[1 2 3 4 5]"),
    ];

    for (name, expr) in test_cases {
        group.bench_with_input(BenchmarkId::from_parameter(name), &expr, |b, &expr| {
            b.iter(|| {
                parse_expression(black_box(expr))
            });
        });
    }
    
    group.finish();
}

/// Benchmark RTFS evaluation performance
fn benchmark_evaluation(c: &mut Criterion) {
    let mut group = c.benchmark_group("evaluation");
    
    let host: Arc<dyn HostInterface> = Arc::new(PureHost::new());
    let module_registry = Arc::new(ModuleRegistry::new());
    let context = RuntimeContext::full();
    
    let test_cases = vec![
        ("literal_int", "42"),
        ("literal_string", "\"hello\""),
        ("arithmetic_simple", "(+ 1 2)"),
        ("arithmetic_complex", "(+ (* 2 3) (- 10 5) (/ 20 4))"),
        ("let_binding", "(let [x 10 y 20] (+ x y))"),
        ("conditional", "(if true 42 0)"),
        ("map_access", "(get {:a 1 :b 2} :a)"),
        ("vector_creation", "[1 2 3 4 5 6 7 8 9 10]"),
    ];

    for (name, expr) in test_cases {
        let parsed = parse_expression(expr).unwrap();
        group.bench_with_input(BenchmarkId::from_parameter(name), &parsed, |b, parsed| {
            b.iter(|| {
                let evaluator = Evaluator::new(
                    module_registry.clone(),
                    context.clone(),
                    host.clone(),
                );
                evaluator.evaluate(black_box(parsed))
            });
        });
    }
    
    group.finish();
}

/// Benchmark pattern matching performance
fn benchmark_pattern_matching(c: &mut Criterion) {
    let mut group = c.benchmark_group("pattern_matching");
    
    let host: Arc<dyn HostInterface> = Arc::new(PureHost::new());
    let module_registry = Arc::new(ModuleRegistry::new());
    let context = RuntimeContext::full();
    
    let test_cases = vec![
        ("match_int", "(match 42 [42 \"found\"] [_ \"not found\"])"),
        ("match_string", "(match \"hello\" [\"hello\" :greeting] [_ :unknown])"),
        ("match_vector", "(match [1 2 3] [[1 2 3] :exact] [[1 & rest] :partial] [_ :none])"),
        ("match_map", "(match {:a 1} [{:a 1} :match] [_ :nomatch])"),
    ];

    for (name, expr) in test_cases {
        let parsed = parse_expression(expr).unwrap();
        group.bench_with_input(BenchmarkId::from_parameter(name), &parsed, |b, parsed| {
            b.iter(|| {
                let evaluator = Evaluator::new(
                    module_registry.clone(),
                    context.clone(),
                    host.clone(),
                );
                evaluator.evaluate(black_box(parsed))
            });
        });
    }
    
    group.finish();
}

/// Benchmark stdlib function performance
fn benchmark_stdlib(c: &mut Criterion) {
    let mut group = c.benchmark_group("stdlib");
    
    let host: Arc<dyn HostInterface> = Arc::new(PureHost::new());
    let module_registry = Arc::new(ModuleRegistry::new());
    let context = RuntimeContext::full();
    
    let test_cases = vec![
        ("map", "(map inc [1 2 3 4 5])"),
        ("filter", "(filter even? [1 2 3 4 5 6 7 8 9 10])"),
        ("reduce", "(reduce + 0 [1 2 3 4 5])"),
        ("range", "(range 1 100)"),
        ("string_ops", "(str \"hello\" \" \" \"world\")"),
    ];

    for (name, expr) in test_cases {
        let parsed = parse_expression(expr).unwrap();
        group.bench_with_input(BenchmarkId::from_parameter(name), &parsed, |b, parsed| {
            b.iter(|| {
                let evaluator = Evaluator::new(
                    module_registry.clone(),
                    context.clone(),
                    host.clone(),
                );
                evaluator.evaluate(black_box(parsed))
            });
        });
    }
    
    group.finish();
}

criterion_group!(
    benches,
    benchmark_parsing,
    benchmark_evaluation,
    benchmark_pattern_matching,
    benchmark_stdlib
);
criterion_main!(benches);

