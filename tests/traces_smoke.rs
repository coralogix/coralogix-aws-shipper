//! On-demand smoke test: ships real `aws/spans` records to a live Coralogix account.
//!
//! Ignored by default because it needs credentials and sends real data. Run with:
//!
//! ```sh
//! CORALOGIX_API_KEY=... CORALOGIX_ENDPOINT=https://ingress.eu2.coralogix.com:443 \
//!   cargo test --test traces_smoke -- --ignored --nocapture
//! ```
//!
//! Unit tests prove the mapping is structurally correct; only this proves the backend
//! accepts it and APM renders the waterfall.

use std::time::Duration;

use coralogix_aws_shipper::traces::to_resource_spans;
use cx_sdk_otlp::auth::AuthData;
use cx_sdk_otlp::config::{BackoffConfig, ChannelConfig};
use cx_sdk_otlp::otlp::proto::collector::trace::v1::ExportTraceServiceRequest;
use cx_sdk_otlp::traces::AuthorizedOtlpTraceExporterGrpc;
use cx_sdk_otlp::{ApiKey, AuthorizedOtlpExporter};
use serde_json::Value;

const FIXTURE: &str = include_str!("fixtures/aws_spans.json");

#[tokio::test]
#[ignore = "requires live Coralogix credentials"]
async fn ships_spans_to_coralogix() {
    let api_key: ApiKey = std::env::var("CORALOGIX_API_KEY")
        .expect("CORALOGIX_API_KEY must be set")
        .into();
    let endpoint = std::env::var("CORALOGIX_ENDPOINT")
        .unwrap_or_else(|_| "https://ingress.eu2.coralogix.com:443".to_string());

    let records: Vec<Value> = serde_json::from_str(FIXTURE).unwrap();
    let mut resource_spans = to_resource_spans(&records, "aws-shipper-test", "aws-spans");

    // The fixture is a frozen capture, so its timestamps age. Shift the whole batch so the
    // newest span lands at "now", keeping relative offsets intact — otherwise the spans fall
    // outside APM's default time window and look like they never arrived.
    shift_to_now(&mut resource_spans);

    let span_count: usize = resource_spans
        .iter()
        .flat_map(|rs| rs.scope_spans.iter())
        .map(|ss| ss.spans.len())
        .sum();
    assert!(span_count > 0, "nothing to send");

    let exporter = AuthorizedOtlpTraceExporterGrpc::builder()
        .with_backoff_config(BackoffConfig {
            initial_delay: Duration::from_millis(200),
            max_delay: Duration::from_secs(2),
            max_elapsed_time: Duration::from_secs(15),
        })
        .with_channel_config(ChannelConfig::new(endpoint).with_webpki_roots())
        .with_auth_data(AuthData::from(&api_key))
        .try_build()
        .expect("failed to build trace exporter");

    let response = exporter
        .export(ExportTraceServiceRequest {
            resource_spans: resource_spans.clone(),
        })
        .await
        .expect("export failed");

    let (rejected, message) = response
        .partial_success
        .as_ref()
        .map(|p| (p.rejected_spans, p.error_message.clone()))
        .unwrap_or((0, String::new()));

    println!(
        "sent {span_count} spans in {} resource group(s)",
        resource_spans.len()
    );
    for rs in &resource_spans {
        let service = rs
            .resource
            .as_ref()
            .and_then(|r| r.attributes.iter().find(|kv| kv.key == "service.name"))
            .and_then(|kv| kv.value.clone());
        println!(
            "  service.name={service:?} spans={}",
            rs.scope_spans[0].spans.len()
        );
    }
    // Trace ids are stable across runs, so this is what to search for in APM.
    if let Some(first) = resource_spans
        .first()
        .and_then(|rs| rs.scope_spans.first())
        .and_then(|ss| ss.spans.first())
    {
        println!("  sample traceId={}", hex(&first.trace_id));
    }

    assert_eq!(rejected, 0, "backend rejected spans: {message}");
}

/// Sends each span of a trace in its own OTLP request, to confirm Coralogix still assembles
/// the trace. The handler splits oversized batches across requests, so a trace's spans can
/// legitimately arrive separately - this proves that does not break trace assembly.
///
/// ```sh
/// CORALOGIX_API_KEY=... cargo test --test traces_smoke -- --ignored --nocapture \
///   assembles_a_trace_split_across_requests
/// ```
#[tokio::test]
#[ignore = "requires live Coralogix credentials"]
async fn assembles_a_trace_split_across_requests() {
    let api_key: ApiKey = std::env::var("CORALOGIX_API_KEY")
        .expect("CORALOGIX_API_KEY must be set")
        .into();
    let endpoint = std::env::var("CORALOGIX_ENDPOINT")
        .unwrap_or_else(|_| "https://ingress.eu2.coralogix.com:443".to_string());

    let records: Vec<Value> = serde_json::from_str(FIXTURE).unwrap();
    let mut resource_spans = to_resource_spans(&records, "aws-shipper-split", "aws-spans");
    shift_to_now(&mut resource_spans);

    // Maximum fragmentation: one request per span, each keeping its own resource.
    let mut requests = Vec::new();
    for rs in &resource_spans {
        for span in rs.scope_spans.iter().flat_map(|ss| ss.spans.iter()) {
            requests.push(ExportTraceServiceRequest {
                resource_spans: vec![cx_sdk_otlp::otlp::proto::trace::v1::ResourceSpans {
                    resource: rs.resource.clone(),
                    scope_spans: vec![cx_sdk_otlp::otlp::proto::trace::v1::ScopeSpans {
                        scope: None,
                        spans: vec![span.clone()],
                        schema_url: String::new(),
                    }],
                    schema_url: String::new(),
                }],
            });
        }
    }

    let exporter = AuthorizedOtlpTraceExporterGrpc::builder()
        .with_backoff_config(BackoffConfig {
            initial_delay: Duration::from_millis(200),
            max_delay: Duration::from_secs(2),
            max_elapsed_time: Duration::from_secs(15),
        })
        .with_channel_config(ChannelConfig::new(endpoint).with_webpki_roots())
        .with_auth_data(AuthData::from(&api_key))
        .try_build()
        .expect("failed to build trace exporter");

    println!(
        "sending {} spans as {} separate requests",
        requests.len(),
        requests.len()
    );
    let mut traces = std::collections::BTreeMap::new();
    for request in requests {
        for rs in &request.resource_spans {
            for s in rs.scope_spans.iter().flat_map(|ss| ss.spans.iter()) {
                *traces.entry(hex(&s.trace_id)).or_insert(0) += 1;
            }
        }
        let response = exporter.export(request).await.expect("export failed");
        if let Some(p) = response.partial_success {
            assert_eq!(
                p.rejected_spans, 0,
                "backend rejected spans: {}",
                p.error_message
            );
        }
    }

    println!("expected after assembly - app aws-shipper-split / subsystem aws-spans:");
    for (t, n) in &traces {
        println!("  trace {t} -> {n} spans");
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Slide every timestamp forward so the batch ends at the current time, preserving the
/// relative shape of the waterfall.
fn shift_to_now(resource_spans: &mut [cx_sdk_otlp::otlp::proto::trace::v1::ResourceSpans]) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64;

    let latest = resource_spans
        .iter()
        .flat_map(|rs| rs.scope_spans.iter())
        .flat_map(|ss| ss.spans.iter())
        .map(|s| s.end_time_unix_nano)
        .max()
        .unwrap_or(now);

    let offset = now.saturating_sub(latest);
    if offset == 0 {
        return;
    }

    for span in resource_spans
        .iter_mut()
        .flat_map(|rs| rs.scope_spans.iter_mut())
        .flat_map(|ss| ss.spans.iter_mut())
    {
        span.start_time_unix_nano += offset;
        span.end_time_unix_nano += offset;
        for event in &mut span.events {
            event.time_unix_nano += offset;
        }
    }
    println!("shifted timestamps forward by {}s", offset / 1_000_000_000);
}
