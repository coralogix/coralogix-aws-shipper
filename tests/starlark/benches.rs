use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};
use coralogix_aws_shipper::logs::transform::StarlarkTransformer;

fn benchmark_script_compilation(c: &mut Criterion) {
    let script = include_str!("../fixtures/scripts/bench_compile.star");
    c.bench_function("compile_script", |b| {
        b.iter(|| StarlarkTransformer::new(black_box(script)).unwrap())
    });
}

fn benchmark_single_transform(c: &mut Criterion) {
    let script = include_str!("../fixtures/scripts/bench_transform.star");
    let transformer = StarlarkTransformer::new(script).unwrap();
    let log = r#"{"msg": "hello", "level": "INFO", "timestamp": "2024-01-01T00:00:00Z"}"#;
    
    c.bench_function("transform_single", |b| {
        b.iter(|| transformer.transform(black_box(log)).unwrap())
    });
}

fn benchmark_batch_transform(c: &mut Criterion) {
    let script = include_str!("../fixtures/scripts/bench_batch.star");
    let transformer = StarlarkTransformer::new(script).unwrap();
    
    let mut group = c.benchmark_group("batch_transform");
    for size in [10, 100, 1000, 10000].iter() {
        let logs: Vec<String> = (0..*size)
            .map(|i| format!(r#"{{"id": {}, "msg": "log entry"}}"#, i))
            .collect();
        
        group.bench_with_input(BenchmarkId::from_parameter(size), &logs, |b, logs| {
            b.iter(|| transformer.transform_batch(black_box(logs.clone())).unwrap())
        });
    }
    group.finish();
}

fn benchmark_unnest_transform(c: &mut Criterion) {
    let script = include_str!("../fixtures/scripts/bench_unnest.star");
    let transformer = StarlarkTransformer::new(script).unwrap();
    
    let mut group = c.benchmark_group("unnest_transform");
    for nested_count in [10, 100, 500].iter() {
        let items: Vec<_> = (0..*nested_count)
            .map(|i| format!(r#"{{"id": {}}}"#, i))
            .collect();
        let input = format!(r#"{{"logs": [{}]}}"#, items.join(","));
        
        group.bench_with_input(BenchmarkId::from_parameter(nested_count), &input, |b, input| {
            b.iter(|| transformer.transform(black_box(input)).unwrap())
        });
    }
    group.finish();
}

criterion_group!(benches, 
    benchmark_script_compilation,
    benchmark_single_transform,
    benchmark_batch_transform,
    benchmark_unnest_transform
);
criterion_main!(benches);
