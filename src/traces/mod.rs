//! Conversion of AWS Transaction Search spans into OTLP.
//!
//! With Transaction Search enabled, AWS-managed services (Step Functions, API Gateway,
//! AppSync, Bedrock AgentCore) write 100% of their spans to the `aws/spans` CloudWatch
//! log group. Each log event is one span, encoded as AWS's own flat JSON — *not* OTLP/JSON:
//!
//! ```json
//! {
//!   "resource": { "attributes": { "service.name": "..." } },
//!   "traceId": "<32 hex>", "spanId": "<16 hex>", "parentSpanId": "<16 hex>",
//!   "name": "S3.ListBuckets", "kind": "CLIENT",
//!   "startTimeUnixNano": 0, "endTimeUnixNano": 0,
//!   "status": { "code": "UNSET" },
//!   "attributes": { "http.status_code": 200 },
//!   "events": [ { "timeUnixNano": 0, "name": "exception", "attributes": {} } ],
//!   "_aws": { "xray": { "type": "subsegment" } }
//! }
//! ```
//!
//! Attributes are a flat map with dotted semconv keys, where OTLP wants a
//! `[{key, value:{stringValue}}]` array. Bridging that is this module's job — no OTel
//! Collector component does it: the `awsfirehose` receiver is metrics and logs only, and no
//! processor turns a log record into a span.

use std::collections::BTreeMap;
use std::time::Duration;

use crate::events::Combined;
use cx_sdk_otlp::auth::AuthData;
use cx_sdk_otlp::config::{BackoffConfig, ChannelConfig};
use cx_sdk_otlp::otlp::proto::collector::trace::v1::ExportTraceServiceRequest;
use cx_sdk_otlp::traces::AuthorizedOtlpTraceExporterGrpc;
use cx_sdk_otlp::{ApiKey, AuthorizedOtlpExporter};
use lambda_runtime::{Error, LambdaEvent};
use prost_014::Message;
use tracing::{error, info, warn};

use cx_sdk_otlp::otlp::proto::common::v1::{any_value, AnyValue, KeyValue};
use cx_sdk_otlp::otlp::proto::resource::v1::Resource;
use cx_sdk_otlp::otlp::proto::trace::v1::{
    span::{Event, SpanKind},
    status::StatusCode,
    ResourceSpans, ScopeSpans, Span, Status,
};
use serde_json::Value;

/// Decode a lowercase-hex id into `len` bytes, rejecting anything malformed.
///
/// Transaction Search already emits W3C-format ids, so there is no X-Ray `1-<hex>-<hex>`
/// unwrapping to do. If that changes it belongs here: an id that fails to normalize detaches
/// the span from the customer's own trace.
fn hex_id(s: &str, len: usize) -> Option<Vec<u8>> {
    if s.len() != len * 2 {
        return None;
    }
    (0..len)
        .map(|i| u8::from_str_radix(s.get(i * 2..i * 2 + 2)?, 16).ok())
        .collect()
}

fn any_value(v: &Value) -> Option<AnyValue> {
    let value = match v {
        Value::String(s) => any_value::Value::StringValue(s.clone()),
        Value::Bool(b) => any_value::Value::BoolValue(*b),
        Value::Number(n) => match n.as_i64() {
            Some(i) => any_value::Value::IntValue(i),
            None => any_value::Value::DoubleValue(n.as_f64()?),
        },
        Value::Null => return None,
        // ponytail: arrays/objects flattened to JSON text. AWS only emits scalars today;
        // upgrade to ArrayValue/KvlistValue if a service starts nesting.
        other => any_value::Value::StringValue(other.to_string()),
    };
    Some(AnyValue { value: Some(value) })
}

/// Flat JSON map -> OTLP attribute list, keeping each JSON value's type. AWS mixes them —
/// real bools, ints, and strings that merely look boolean — so the type comes from the JSON,
/// never from the key.
fn attributes(v: Option<&Value>) -> Vec<KeyValue> {
    let Some(map) = v.and_then(Value::as_object) else {
        return Vec::new();
    };
    map.iter()
        .filter_map(|(k, v)| {
            Some(KeyValue {
                key: k.clone(),
                value: Some(any_value(v)?),
            })
        })
        .collect()
}

/// Span events, where AWS puts exception detail on failed spans. Already OTel semantic
/// convention, so this is a structural copy; dropping it leaves failed spans with no reason.
fn events(v: Option<&Value>) -> Vec<Event> {
    let Some(arr) = v.and_then(Value::as_array) else {
        return Vec::new();
    };
    arr.iter()
        .filter_map(|e| {
            Some(Event {
                time_unix_nano: e.get("timeUnixNano")?.as_u64()?,
                name: e
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                attributes: attributes(e.get("attributes")),
                dropped_attributes_count: 0,
            })
        })
        .collect()
}

fn span_kind(kind: Option<&str>) -> SpanKind {
    match kind.unwrap_or_default() {
        "SERVER" => SpanKind::Server,
        "CLIENT" => SpanKind::Client,
        "INTERNAL" => SpanKind::Internal,
        "PRODUCER" => SpanKind::Producer,
        "CONSUMER" => SpanKind::Consumer,
        _ => SpanKind::Unspecified,
    }
}

fn status(v: Option<&Value>) -> Option<Status> {
    let status = v?;
    let code = match status
        .get("code")
        .and_then(Value::as_str)
        .unwrap_or("UNSET")
    {
        "OK" => StatusCode::Ok,
        "ERROR" => StatusCode::Error,
        _ => StatusCode::Unset,
    };
    Some(Status {
        code: code as i32,
        message: status
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
    })
}

/// Convert one `aws/spans` record, or `None` if it should not become a span.
///
/// Transaction Search emits each span twice: once at start (`endTimeUnixNano: null`,
/// `aws.xray.inprogress: true`) and again on completion. Mapping both would duplicate span
/// ids and draw zero-duration phantoms, so the missing end time doubles as the dedup key.
fn to_span(record: &Value) -> Option<Span> {
    // as_u64, never as_f64: these values exceed 2^53, so a float round-trip corrupts them.
    let start_time_unix_nano = record.get("startTimeUnixNano")?.as_u64()?;
    let end_time_unix_nano = record.get("endTimeUnixNano")?.as_u64()?;

    let trace_id = hex_id(record.get("traceId")?.as_str()?, 16)?;
    let span_id = hex_id(record.get("spanId")?.as_str()?, 8)?;
    let parent_span_id = record
        .get("parentSpanId")
        .and_then(Value::as_str)
        .and_then(|s| hex_id(s, 8))
        .unwrap_or_default();

    Some(Span {
        trace_id,
        span_id,
        parent_span_id,
        trace_state: String::new(),
        name: record
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        kind: span_kind(record.get("kind").and_then(Value::as_str)) as i32,
        start_time_unix_nano,
        end_time_unix_nano,
        attributes: attributes(record.get("attributes")),
        status: status(record.get("status")),
        events: events(record.get("events")),
        // ponytail: no `links` seen in any AWS-emitted record; add if a service starts using them.
        links: Vec::new(),
        dropped_attributes_count: 0,
        dropped_events_count: 0,
        dropped_links_count: 0,
    })
}

fn record_service_name(record: &Value) -> Option<&str> {
    record
        .get("resource")
        .and_then(|r| r.get("attributes"))
        .and_then(|a| a.get("service.name"))
        .and_then(Value::as_str)
}

/// `service.name` per trace id, taken from whichever record in the batch carries one — in
/// practice the root segment. Spans without one land under an unknown service in APM.
fn service_names_by_trace(records: &[Value]) -> BTreeMap<&str, &str> {
    let mut by_trace = BTreeMap::new();
    for record in records {
        if let (Some(trace_id), Some(service)) = (
            record.get("traceId").and_then(Value::as_str),
            record_service_name(record),
        ) {
            by_trace.entry(trace_id).or_insert(service);
        }
    }
    by_trace
}

/// Resource attributes for a record, resolving `service.name` in order: the span's own
/// resource, a real `aws.local.service`, then the root segment of the same trace in this batch.
///
/// Only root segments carry `service.name`. Subscription filters push near-real-time in
/// batches of roughly one record, so a child rarely shares a batch with its root, and children
/// hold no owner identifier of their own. Spans that resolve to nothing are left unnamed on
/// purpose: a placeholder would only look like a real service. Resolving it accurately needs
/// the whole trace, so it belongs in ingestion.
fn resource_attributes(
    record: &Value,
    service_by_trace: &BTreeMap<&str, &str>,
) -> BTreeMap<String, Value> {
    let mut attrs: BTreeMap<String, Value> = record
        .get("resource")
        .and_then(|r| r.get("attributes"))
        .and_then(Value::as_object)
        .map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
        .unwrap_or_default();

    if !attrs.contains_key("service.name") {
        let inherited = record
            .get("attributes")
            .and_then(|a| a.get("aws.local.service"))
            .and_then(Value::as_str)
            .filter(|n| *n != "UnknownService")
            .or_else(|| {
                record
                    .get("traceId")
                    .and_then(Value::as_str)
                    .and_then(|t| service_by_trace.get(t).copied())
            });

        if let Some(name) = inherited {
            attrs.insert("service.name".to_string(), Value::String(name.to_string()));
        }
    }
    attrs
}

/// Convert `aws/spans` log records into OTLP `ResourceSpans`, grouped by resource.
///
/// Grouping matters: APM derives the service map from distinct resources, so spans from
/// different AWS services must not be flattened into one `ResourceSpans`.
pub fn to_resource_spans(records: &[Value], app_name: &str, sub_name: &str) -> Vec<ResourceSpans> {
    let service_by_trace = service_names_by_trace(records);
    let mut groups: BTreeMap<String, (BTreeMap<String, Value>, Vec<Span>)> = BTreeMap::new();

    for record in records {
        let Some(span) = to_span(record) else {
            continue;
        };
        let attrs = resource_attributes(record, &service_by_trace);
        // BTreeMap is already sorted, so this key is stable regardless of JSON field order.
        let key = format!("{attrs:?}");
        groups
            .entry(key)
            .or_insert((attrs, Vec::new()))
            .1
            .push(span);
    }

    groups
        .into_values()
        .map(|(attrs, spans)| {
            let mut resource = Resource {
                attributes: attrs
                    .iter()
                    .filter_map(|(k, v)| {
                        Some(KeyValue {
                            key: k.clone(),
                            value: Some(any_value(v)?),
                        })
                    })
                    .collect(),
                dropped_attributes_count: 0,
            };
            resource
                .add_metadata_to_resource(app_name.to_string().into(), sub_name.to_string().into());

            ResourceSpans {
                resource: Some(resource),
                scope_spans: vec![ScopeSpans {
                    scope: None,
                    spans,
                    schema_url: String::new(),
                }],
                schema_url: String::new(),
            }
        })
        .collect()
}

pub struct Config {
    pub api_key: ApiKey,
    pub endpoint: String,
    pub app_name: String,
    pub sub_name: String,
}

impl Config {
    pub fn load_from_env() -> Result<Config, String> {
        Ok(Config {
            app_name: std::env::var("APP_NAME").unwrap_or_else(|_| "aws".to_string()),
            api_key: std::env::var("CORALOGIX_API_KEY")
                .map_err(|e| format!("CORALOGIX_API_KEY is not set: {e}"))?
                .into(),
            // An explicit endpoint wins (collectors, tests); otherwise derive it from the
            // domain, matching how the OTLP log path resolves `ingress.<domain>`.
            endpoint: match std::env::var("CORALOGIX_ENDPOINT") {
                Ok(endpoint) => endpoint,
                Err(_) => {
                    let domain = std::env::var("CORALOGIX_DOMAIN").map_err(|e| {
                        format!("neither CORALOGIX_ENDPOINT nor CORALOGIX_DOMAIN is set: {e}")
                    })?;
                    format!("https://ingress.{domain}:443")
                }
            },
            sub_name: std::env::var("SUB_NAME").unwrap_or_else(|_| "traces".to_string()),
        })
    }
}

pub fn build_exporter(config: &Config) -> Result<AuthorizedOtlpTraceExporterGrpc, String> {
    AuthorizedOtlpTraceExporterGrpc::builder()
        // ponytail: fixed backoff. Lift to env vars if customers need to tune it, as the
        // logs path eventually did.
        .with_backoff_config(BackoffConfig {
            initial_delay: Duration::from_millis(200),
            max_delay: Duration::from_secs(2),
            max_elapsed_time: Duration::from_secs(20),
        })
        .with_channel_config(ChannelConfig::new(config.endpoint.clone()).with_webpki_roots())
        .with_auth_data(AuthData::from(&config.api_key))
        .try_build()
        .map_err(|e| format!("failed to build OTLP trace exporter: {e}"))
}

/// Largest OTLP request we will send. Mirrors the default the logs exporter uses; gRPC
/// receivers commonly cap decoding at 4 MiB and reject anything larger outright.
const MAX_REQUEST_BYTES: usize = 4 * 1024 * 1024;

fn span_count_of(resource_spans: &[ResourceSpans]) -> usize {
    resource_spans
        .iter()
        .flat_map(|rs| rs.scope_spans.iter())
        .map(|ss| ss.spans.len())
        .sum()
}

/// Split at `at` spans, preserving each part's resource so no span loses its service name.
fn split_at(
    resource_spans: Vec<ResourceSpans>,
    at: usize,
) -> (Vec<ResourceSpans>, Vec<ResourceSpans>) {
    let (mut head, mut tail) = (Vec::new(), Vec::new());
    let mut taken = 0usize;

    for rs in resource_spans {
        let spans: Vec<Span> = rs.scope_spans.into_iter().flat_map(|ss| ss.spans).collect();
        let take = at.saturating_sub(taken).min(spans.len());
        let (h, t) = spans.split_at(take);
        taken += take;

        let part = |spans: &[Span], out: &mut Vec<ResourceSpans>| {
            if !spans.is_empty() {
                out.push(ResourceSpans {
                    resource: rs.resource.clone(),
                    scope_spans: vec![ScopeSpans {
                        scope: None,
                        spans: spans.to_vec(),
                        schema_url: String::new(),
                    }],
                    schema_url: String::new(),
                });
            }
        };
        part(h, &mut head);
        part(t, &mut tail);
    }
    (head, tail)
}

/// Split spans across as many requests as needed to stay under `max_bytes`.
///
/// A CloudWatch batch is normally a single record, but delivery size is not something we
/// control, so a burst could otherwise produce a request the receiver rejects — and that
/// would fail identically on every retry until the batch is lost.
///
/// Halves recursively and measures the real encoded length each time, rather than
/// estimating from span sizes: protobuf framing makes an estimate under-count, and a
/// request that is over by a few bytes is rejected just the same. A single span larger than
/// the limit is still sent alone, since there is nothing left to split.
fn split_requests(
    resource_spans: Vec<ResourceSpans>,
    max_bytes: usize,
) -> Vec<ExportTraceServiceRequest> {
    let spans = span_count_of(&resource_spans);
    let request = ExportTraceServiceRequest { resource_spans };

    if spans <= 1 || request.encoded_len() <= max_bytes {
        return vec![request];
    }

    let (head, tail) = split_at(request.resource_spans, spans / 2);
    let mut requests = split_requests(head, max_bytes);
    requests.extend(split_requests(tail, max_bytes));
    requests
}

/// Ship one CloudWatch Logs batch from the `aws/spans` log group as OTLP spans.
pub async fn handler(
    config: &Config,
    exporter: &AuthorizedOtlpTraceExporterGrpc,
    event: LambdaEvent<Combined>,
) -> Result<(), Error> {
    let Combined::CloudWatchLogs(logs_event) = event.payload else {
        error!("incompatible event type for traces telemetry mode: expected CloudWatch Logs from the aws/spans log group");
        return Err("incompatible event type for traces telemetry mode"
            .to_string()
            .into());
    };

    let data = logs_event.aws_logs.data;
    let received = data.log_events.len();

    let records: Vec<Value> = data
        .log_events
        .iter()
        .filter_map(|entry| match serde_json::from_str(&entry.message) {
            Ok(record) => Some(record),
            Err(e) => {
                // A non-span record in aws/spans is not fatal: skip it and ship the rest
                // rather than failing the batch and having CloudWatch retry it forever.
                warn!("skipping unparsable record in {}: {e}", data.log_group);
                None
            }
        })
        .collect();

    let resource_spans = to_resource_spans(&records, &config.app_name, &config.sub_name);
    let span_count: usize = resource_spans
        .iter()
        .flat_map(|rs| rs.scope_spans.iter())
        .map(|ss| ss.spans.len())
        .sum();

    // Records that produced no span are either AWS's in-progress duplicates (expected, we
    // drop them deliberately) or records we could not map (unexpected — a format change
    // would look exactly like this, so it must be visible rather than silent).
    let in_progress = records
        .iter()
        .filter(|r| r.get("endTimeUnixNano").and_then(Value::as_u64).is_none())
        .count();
    let unmappable = records.len().saturating_sub(in_progress + span_count);
    if unmappable > 0 {
        warn!(
            "{unmappable} record(s) in {} could not be mapped to spans",
            data.log_group
        );
    }

    if span_count == 0 {
        info!(
            "no spans to ship from {} ({received} record(s) received, {in_progress} in-progress)",
            data.log_group
        );
        return Ok(());
    }

    info!(
        "shipping {span_count} span(s) from {received} record(s) in {}",
        data.log_group
    );

    for request in split_requests(resource_spans, MAX_REQUEST_BYTES) {
        let response = exporter
            .export(request)
            .await
            .map_err(|e| Error::from(e.to_string()))?;

        // Match the logs path: a partial rejection fails the batch so the existing Lambda
        // retry/DLQ handling runs. Silently succeeding would lose the rejected spans.
        if let Some(partial) = response.partial_success {
            if partial.rejected_spans > 0 {
                warn!(
                    rejected_spans = partial.rejected_spans,
                    error_message_present = !partial.error_message.is_empty(),
                    "Coralogix rejected spans"
                );
                return Err(Error::from(format!(
                    "OTLP trace export failed: server rejected {} span(s)",
                    partial.rejected_spans
                )));
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Real `aws/spans` records from Step Functions executions, successful and failed, so
    /// error spans and exception events are covered.
    const FIXTURE: &str = include_str!("../../tests/fixtures/aws_spans.json");

    fn fixture() -> Vec<Value> {
        serde_json::from_str(FIXTURE).unwrap()
    }

    /// No default service name, so these exercise the inheritance chain on its own.
    fn all_spans(records: &[Value]) -> Vec<Span> {
        to_resource_spans(records, "aws", "traces")
            .into_iter()
            .flat_map(|rs| rs.scope_spans.into_iter().flat_map(|ss| ss.spans))
            .collect()
    }

    fn service_name_of(rs: &ResourceSpans) -> Option<String> {
        rs.resource
            .as_ref()?
            .attributes
            .iter()
            .find(|kv| kv.key == "service.name")
            .and_then(|kv| kv.value.clone())
            .and_then(|v| match v.value {
                Some(any_value::Value::StringValue(s)) => Some(s),
                _ => None,
            })
    }

    #[test]
    fn drops_in_progress_duplicates() {
        let records = fixture();
        assert_eq!(records.len(), 42, "fixture changed");

        let spans = all_spans(&records);

        assert_eq!(spans.len(), 24, "in-progress records must be dropped");

        let mut ids: Vec<&Vec<u8>> = spans.iter().map(|s| &s.span_id).collect();
        ids.sort();
        let unique = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), unique, "duplicate span ids leaked through");
    }

    #[test]
    fn maps_ids_times_and_kinds() {
        let records = fixture();
        let spans = all_spans(&records);

        for span in &spans {
            assert_eq!(span.trace_id.len(), 16);
            assert_eq!(span.span_id.len(), 8);
            assert!(span.end_time_unix_nano >= span.start_time_unix_nano);
            assert!(span.start_time_unix_nano > 0);
        }

        // The completed root segment: no parent, SERVER kind, and timestamps preserved
        // exactly — this value is past 2^53 and would round if parsed as f64.
        let root = spans
            .iter()
            .find(|s| s.span_id == hex_id("b03258e2e1abc353", 8).unwrap())
            .expect("root span missing");
        assert!(root.parent_span_id.is_empty());
        assert_eq!(root.kind, SpanKind::Server as i32);
        assert_eq!(root.start_time_unix_nano, 1785830304824000000);
        assert_eq!(root.end_time_unix_nano, 1785830306082000128);

        // Child spans keep their parent link, so the waterfall reconstructs.
        let client = spans
            .iter()
            .find(|s| s.span_id == hex_id("29d03b80232a080a", 8).unwrap())
            .expect("client span missing");
        assert_eq!(client.kind, SpanKind::Client as i32);
        assert_eq!(
            client.parent_span_id,
            hex_id("3b65215f9ddfbb8f", 8).unwrap()
        );
    }

    #[test]
    fn preserves_attribute_types() {
        let records = fixture();
        let spans = all_spans(&records);

        let client = spans
            .iter()
            .find(|s| s.span_id == hex_id("29d03b80232a080a", 8).unwrap())
            .unwrap();
        let attr = |key: &str| {
            client
                .attributes
                .iter()
                .find(|kv| kv.key == key)
                .and_then(|kv| kv.value.clone())
                .and_then(|v| v.value)
        };

        assert_eq!(
            attr("http.status_code"),
            Some(any_value::Value::IntValue(200)),
            "ints must not become strings"
        );
        assert_eq!(
            attr("telemetry.extended"),
            Some(any_value::Value::StringValue("true".to_string())),
            "the string \"true\" must not become a bool"
        );
        assert_eq!(
            attr("aws.region"),
            Some(any_value::Value::StringValue("eu-west-1".to_string()))
        );
    }

    #[test]
    fn groups_by_resource_and_tags_with_coralogix_metadata() {
        let records = fixture();
        let grouped = to_resource_spans(&records, "my-app", "my-sub");

        assert!(grouped.len() > 1, "distinct resources must stay separate");

        for rs in &grouped {
            let attrs = &rs.resource.as_ref().unwrap().attributes;
            let has = |k: &str| attrs.iter().any(|kv| kv.key == k);
            assert!(has("cx.application.name") && has("cx.subsystem.name"));
        }

        // The state machine's own resource keeps service.name for the APM service map.
        assert!(grouped.iter().any(|rs| {
            rs.resource.as_ref().unwrap().attributes.iter().any(|kv| {
                kv.key == "service.name"
                    && kv.value.as_ref().and_then(|v| v.value.clone())
                        == Some(any_value::Value::StringValue("cx-span-sample".to_string()))
            })
        }));
    }

    /// Failed spans must arrive as ERROR *with* their exception event intact. Without the
    /// event, APM shows a red span and no reason; without the ERROR status, a failed Step
    /// Functions run renders as a success.
    #[test]
    fn maps_error_status_and_exception_events() {
        let records = fixture();
        let spans = all_spans(&records);

        let errors: Vec<&Span> = spans
            .iter()
            .filter(|s| s.status.as_ref().map(|st| st.code) == Some(StatusCode::Error as i32))
            .collect();
        assert_eq!(errors.len(), 9, "error spans lost");

        let failed = spans
            .iter()
            .find(|s| s.span_id == hex_id("7df80a2dabf4fdf3", 8).unwrap())
            .expect("known failed span missing");

        assert_eq!(
            failed.status.as_ref().unwrap().code,
            StatusCode::Error as i32
        );
        assert_eq!(failed.events.len(), 1);

        let event = &failed.events[0];
        assert_eq!(event.name, "exception");
        assert_eq!(event.time_unix_nano, 1785830814611000064);

        let attr = |key: &str| {
            event
                .attributes
                .iter()
                .find(|kv| kv.key == key)
                .and_then(|kv| kv.value.clone())
                .and_then(|v| v.value)
        };
        assert_eq!(
            attr("exception.type"),
            Some(any_value::Value::StringValue(
                "software.amazon.awssdk.services.s3.model.AccessDeniedException".to_string()
            ))
        );
        assert!(
            matches!(attr("exception.message"), Some(any_value::Value::StringValue(m)) if m.contains("s3:ListAllMyBuckets")),
            "exception message must survive"
        );
        assert!(attr("exception.stacktrace").is_some());

        // Successful spans stay clean — no phantom events, no ERROR bleed.
        let ok = spans
            .iter()
            .find(|s| s.span_id == hex_id("29d03b80232a080a", 8).unwrap())
            .unwrap();
        assert!(ok.events.is_empty());
        assert_eq!(ok.status.as_ref().unwrap().code, StatusCode::Unset as i32);
    }

    /// Every span must carry a service.name, or it lands under an unknown service in APM
    /// and vanishes from the service map. Only root segments have one in the raw data.
    #[test]
    fn every_span_inherits_a_service_name() {
        let records = fixture();
        let grouped = to_resource_spans(&records, "aws", "traces");

        let mut with_service = 0;
        let mut total = 0;
        for rs in &grouped {
            let service = rs
                .resource
                .as_ref()
                .unwrap()
                .attributes
                .iter()
                .find(|kv| kv.key == "service.name")
                .and_then(|kv| kv.value.as_ref())
                .and_then(|v| v.value.clone());

            let spans: usize = rs.scope_spans.iter().map(|ss| ss.spans.len()).sum();
            total += spans;
            if service.is_some() {
                with_service += spans;
                assert_eq!(
                    service,
                    Some(any_value::Value::StringValue("cx-span-sample".to_string())),
                    "inherited the wrong service name"
                );
            }
        }

        assert_eq!(total, 24);
        assert_eq!(
            with_service, total,
            "child spans must inherit service.name from their trace's root segment"
        );
    }

    /// The handler's input is a gzipped, base64-encoded CloudWatch subscription batch, and
    /// `Combined` sniffs the event type by trial deserialization. This checks an `aws/spans`
    /// batch is recognised as CloudWatch Logs and survives the round trip.
    #[test]
    fn decodes_a_cloudwatch_subscription_batch() {
        use base64::Engine;
        use flate2::write::GzEncoder;
        use flate2::Compression;
        use std::io::Write;

        let records = fixture();
        let log_events: Vec<Value> = records
            .iter()
            .enumerate()
            .map(|(i, record)| {
                serde_json::json!({
                    "id": i.to_string(),
                    "timestamp": 1785830304824i64,
                    "message": record.to_string(),
                })
            })
            .collect();

        let payload = serde_json::json!({
            "owner": "123456789012",
            "logGroup": "aws/spans",
            "logStream": "default",
            "subscriptionFilters": ["coralogix-traces"],
            "messageType": "DATA_MESSAGE",
            "logEvents": log_events,
        });

        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(payload.to_string().as_bytes()).unwrap();
        let data = base64::engine::general_purpose::STANDARD.encode(encoder.finish().unwrap());

        let event: Combined =
            serde_json::from_value(serde_json::json!({"awslogs": {"data": data}})).unwrap();
        let Combined::CloudWatchLogs(logs_event) = event else {
            panic!("aws/spans batch was not detected as a CloudWatch Logs event");
        };

        assert_eq!(logs_event.aws_logs.data.log_group, "aws/spans");
        assert_eq!(logs_event.aws_logs.data.log_events.len(), 42);

        let parsed: Vec<Value> = logs_event
            .aws_logs
            .data
            .log_events
            .iter()
            .filter_map(|entry| serde_json::from_str(&entry.message).ok())
            .collect();
        assert_eq!(parsed.len(), 42, "records must survive the gzip round trip");
        assert_eq!(
            all_spans(&parsed).len(),
            24,
            "a real batch must map to the same spans as the raw fixture"
        );
    }

    /// A batch of children without their root — what CloudWatch actually delivers — must be
    /// left unnamed rather than given a made-up name. Pins the known gap: a placeholder would
    /// be indistinguishable from a real service, so resolution belongs in ingestion.
    #[test]
    fn children_without_a_root_are_left_unnamed() {
        let children: Vec<Value> = fixture()
            .into_iter()
            .filter(|r| r.get("parentSpanId").is_some())
            .collect();
        assert!(!children.is_empty());

        let grouped = to_resource_spans(&children, "aws", "traces");
        assert!(!grouped.is_empty());
        assert!(
            grouped.iter().all(|rs| service_name_of(rs).is_none()),
            "no service name should be invented when the root is absent"
        );
    }

    /// A batch that fits stays a single request - splitting must not fragment normal traffic.
    #[test]
    fn small_batches_are_sent_as_one_request() {
        let grouped = to_resource_spans(&fixture(), "aws", "traces");
        let requests = split_requests(grouped, MAX_REQUEST_BYTES);
        assert_eq!(requests.len(), 1);
        assert_eq!(
            requests[0]
                .resource_spans
                .iter()
                .flat_map(|rs| rs.scope_spans.iter())
                .map(|ss| ss.spans.len())
                .sum::<usize>(),
            24
        );
    }

    /// With a tiny limit every span must still be sent, exactly once, under the cap.
    #[test]
    fn oversized_batches_split_without_losing_spans() {
        let grouped = to_resource_spans(&fixture(), "aws", "traces");
        let limit = 1024;
        let requests = split_requests(grouped, limit);

        assert!(requests.len() > 1, "expected the batch to be split");

        let mut ids: Vec<Vec<u8>> = requests
            .iter()
            .flat_map(|r| r.resource_spans.iter())
            .flat_map(|rs| rs.scope_spans.iter())
            .flat_map(|ss| ss.spans.iter())
            .map(|s| s.span_id.clone())
            .collect();
        assert_eq!(ids.len(), 24, "every span must survive the split");
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), 24, "no span may be duplicated across requests");

        // Every request holding more than one span must respect the cap.
        for r in &requests {
            let spans: usize = r
                .resource_spans
                .iter()
                .flat_map(|rs| rs.scope_spans.iter())
                .map(|ss| ss.spans.len())
                .sum();
            if spans > 1 {
                assert!(r.encoded_len() <= limit, "request exceeded the limit");
            }
        }

        // Resource attributes must be carried onto each split part, or spans lose their
        // service name and Coralogix metadata.
        for r in &requests {
            for rs in &r.resource_spans {
                assert!(rs.resource.is_some());
            }
        }
    }

    #[test]
    fn rejects_malformed_ids() {
        assert!(hex_id("abc", 8).is_none(), "wrong length");
        assert!(hex_id("zzzzzzzzzzzzzzzz", 8).is_none(), "not hex");
        assert_eq!(hex_id("00ff", 2), Some(vec![0x00, 0xff]));

        // A record with a bad trace id is dropped rather than shipped detached.
        let bad = serde_json::json!({
            "traceId": "nothex", "spanId": "b03258e2e1abc353",
            "startTimeUnixNano": 1u64, "endTimeUnixNano": 2u64,
        });
        assert!(to_span(&bad).is_none());
    }
}
