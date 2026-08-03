use coralogix_aws_shipper::logs::{
    exporter::otlp::build_export_request,
    model::{LogSeverity, ProcessedLog},
};
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use prost_014::Message;

fn logs(count: usize, unique_resources: bool) -> Vec<ProcessedLog> {
    (0..count)
        .map(|index| ProcessedLog {
            application_name: if unique_resources {
                format!("app-{index}")
            } else {
                "app".to_string()
            },
            subsystem_name: if unique_resources {
                format!("sub-{index}")
            } else {
                "sub".to_string()
            },
            body: serde_json::json!({"message": "x".repeat(4096)}),
            severity: LogSeverity::Info,
            timestamp: time::OffsetDateTime::UNIX_EPOCH,
        })
        .collect()
}

fn benchmark(c: &mut Criterion) {
    let one_resource = logs(1_000, false);
    let unique_resources = logs(1_000, true);
    println!(
        "encoded request sizes: one resource={} bytes, unique resources={} bytes",
        build_export_request(&one_resource).encoded_len(),
        build_export_request(&unique_resources).encoded_len()
    );
    c.bench_function("otlp_group_1000_one_resource", |b| {
        b.iter(|| build_export_request(black_box(&one_resource)))
    });
    c.bench_function("otlp_group_1000_unique_resources", |b| {
        b.iter(|| build_export_request(black_box(&unique_resources)))
    });
}

criterion_group!(benches, benchmark);
criterion_main!(benches);
