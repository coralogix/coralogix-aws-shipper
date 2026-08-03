use std::{
    cell::Cell,
    collections::{BTreeMap, VecDeque},
    sync::Arc,
    time::{Duration, Instant},
};

use async_trait::async_trait;
use cx_sdk_otlp::otlp::proto::{
    common::v1::{any_value, AnyValue, ArrayValue, InstrumentationScope, KeyValue, KeyValueList},
    logs::v1::{LogRecord, ResourceLogs, ScopeLogs, SeverityNumber},
    resource::v1::Resource,
};
use cx_sdk_otlp::{
    auth::AuthData,
    config::{BackoffConfig, ChannelConfig},
    logs::OtlpLogExporterGrpc,
    otlp::proto::collector::logs::v1::{ExportLogsServiceRequest, ExportLogsServiceResponse},
    OtlpExporter, RequestListener, RequestOutcome, ResponseError,
};
use prost_014::Message;

use crate::logs::{
    exporter::{LogExportError, LogExporter, OtlpFailureClassification, OtlpResponseError},
    model::{LogSeverity, ProcessedLog},
};

tokio::task_local! {
    static OTLP_ATTEMPT_COUNT: Cell<u64>;
}

struct AttemptListener;

impl RequestListener for AttemptListener {
    fn on_request_completed(&self, _outcome: RequestOutcome, _duration: Duration) {
        let _ = OTLP_ATTEMPT_COUNT.try_with(|count| count.set(count.get() + 1));
    }
}

struct OtlpTransportResponse {
    response: ExportLogsServiceResponse,
    attempt_count: u64,
}

#[async_trait]
trait OtlpTransport: Send + Sync {
    async fn send(
        &self,
        request: ExportLogsServiceRequest,
    ) -> Result<OtlpTransportResponse, LogExportError>;
}

struct SdkOtlpTransport {
    exporter: OtlpLogExporterGrpc,
    auth: AuthData,
}

#[async_trait]
impl OtlpTransport for SdkOtlpTransport {
    async fn send(
        &self,
        request: ExportLogsServiceRequest,
    ) -> Result<OtlpTransportResponse, LogExportError> {
        let started = Instant::now();
        let (result, attempt_count) = OTLP_ATTEMPT_COUNT
            .scope(Cell::new(0), async {
                let result = self.exporter.export(request, &self.auth).await;
                (result, OTLP_ATTEMPT_COUNT.with(Cell::get))
            })
            .await;

        match result {
            Ok(response) => Ok(OtlpTransportResponse {
                response,
                attempt_count,
            }),
            Err(error) => {
                let error = sanitized_otlp_error(&error);
                tracing::error!(
                    sdk_status = %error.classification(),
                    grpc_status = error.grpc_status(),
                    attempt_count,
                    elapsed_ms = started.elapsed().as_millis(),
                    "OTLP/gRPC log export failed"
                );
                Err(LogExportError::OtlpResponse(error))
            }
        }
    }
}

fn sanitized_otlp_error(error: &ResponseError) -> OtlpResponseError {
    let (classification, grpc_status) = match error {
        ResponseError::Server(error) => (
            OtlpFailureClassification::Server,
            grpc_status_name(&format!("{:?}", error.status.code())),
        ),
        ResponseError::Client(error) => (
            OtlpFailureClassification::Client,
            grpc_status_name(&format!("{:?}", error.status.code())),
        ),
        ResponseError::Blocked => (OtlpFailureClassification::Blocked, "resource_exhausted"),
        ResponseError::Unknown(error) => (
            OtlpFailureClassification::Unknown,
            error
                .status
                .as_ref()
                .map(|status| grpc_status_name(&format!("{:?}", status.code())))
                .unwrap_or("unavailable"),
        ),
        _ => (OtlpFailureClassification::Unclassified, "unavailable"),
    };
    OtlpResponseError::new(classification, grpc_status)
}

fn grpc_status_name(code: &str) -> &'static str {
    match code {
        "Ok" => "ok",
        "Cancelled" => "cancelled",
        "Unknown" => "unknown",
        "InvalidArgument" => "invalid_argument",
        "DeadlineExceeded" => "deadline_exceeded",
        "NotFound" => "not_found",
        "AlreadyExists" => "already_exists",
        "PermissionDenied" => "permission_denied",
        "ResourceExhausted" => "resource_exhausted",
        "FailedPrecondition" => "failed_precondition",
        "Aborted" => "aborted",
        "OutOfRange" => "out_of_range",
        "Unimplemented" => "unimplemented",
        "Internal" => "internal",
        "Unavailable" => "unavailable",
        "DataLoss" => "data_loss",
        "Unauthenticated" => "unauthenticated",
        _ => "unavailable",
    }
}

fn otlp_channel_config(endpoint: String) -> ChannelConfig {
    ChannelConfig::new(endpoint).with_webpki_roots()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OtlpCompression {
    None,
    Gzip,
}

impl OtlpCompression {
    fn gzip_enabled(self) -> bool {
        matches!(self, Self::Gzip)
    }
}

pub struct OtlpGrpcExporter {
    transport: Arc<dyn OtlpTransport>,
    max_request_bytes: usize,
}

impl OtlpGrpcExporter {
    pub fn new(
        endpoint: String,
        auth: AuthData,
        max_elapsed_time: u64,
        max_request_bytes: usize,
    ) -> Result<Self, LogExportError> {
        Self::new_with_compression(
            endpoint,
            auth,
            max_elapsed_time,
            max_request_bytes,
            OtlpCompression::Gzip,
        )
    }

    pub(crate) fn new_with_compression(
        endpoint: String,
        auth: AuthData,
        max_elapsed_time: u64,
        max_request_bytes: usize,
        compression: OtlpCompression,
    ) -> Result<Self, LogExportError> {
        let exporter = OtlpLogExporterGrpc::builder()
            .with_channel_config(otlp_channel_config(endpoint))
            .with_backoff_config(BackoffConfig {
                initial_delay: Duration::from_millis(100),
                max_delay: Duration::from_secs(10),
                max_elapsed_time: Duration::from_secs(max_elapsed_time),
            })
            .with_gzip_compression(compression.gzip_enabled())
            .listener(AttemptListener)
            .try_build()
            .map_err(|error| LogExportError::OtlpInitialization(error.to_string()))?;

        Ok(Self {
            transport: Arc::new(SdkOtlpTransport { exporter, auth }),
            max_request_bytes,
        })
    }

    #[cfg(test)]
    fn with_transport(transport: Arc<dyn OtlpTransport>, max_request_bytes: usize) -> Self {
        Self {
            transport,
            max_request_bytes,
        }
    }
}

#[async_trait]
impl LogExporter for OtlpGrpcExporter {
    async fn export(&self, logs: Vec<ProcessedLog>) -> Result<(), LogExportError> {
        if logs.is_empty() {
            return Ok(());
        }

        let mut pending = VecDeque::from([logs]);
        let mut requests = Vec::new();
        while let Some(mut batch) = pending.pop_front() {
            let request = build_export_request(&batch);
            let encoded_bytes = request.encoded_len();
            if encoded_bytes > self.max_request_bytes {
                if batch.len() == 1 {
                    return Err(LogExportError::OversizedRecord);
                }

                let right = batch.split_off(batch.len() / 2);
                pending.push_front(right);
                pending.push_front(batch);
                continue;
            }

            let resource_count = request.resource_logs.len();
            requests.push((request, batch.len(), resource_count, encoded_bytes));
        }

        for (request, record_count, resource_count, encoded_bytes) in requests {
            let started = Instant::now();
            let transport_response = self.transport.send(request).await?;
            let response = transport_response.response;

            if let Some(partial) = response.partial_success {
                let error_message_present = !partial.error_message.is_empty();
                if partial.rejected_log_records > 0 {
                    tracing::warn!(
                        rejected_log_records = partial.rejected_log_records,
                        record_count,
                        resource_count,
                        encoded_bytes,
                        error_message_present,
                        "OTLP collector rejected log records"
                    );
                    return Err(LogExportError::PartialRejection {
                        rejected_log_records: partial.rejected_log_records,
                    });
                }

                if error_message_present {
                    tracing::warn!(
                        record_count,
                        resource_count,
                        encoded_bytes,
                        error_message_present,
                        "OTLP collector accepted all log records with a warning"
                    );
                }
            }

            tracing::info!(
                record_count,
                resource_count,
                encoded_bytes,
                attempt_count = transport_response.attempt_count,
                elapsed_ms = started.elapsed().as_millis(),
                "Delivered log records through OTLP/gRPC"
            );
        }

        Ok(())
    }
}

fn json_to_any_value(value: serde_json::Value) -> AnyValue {
    let value = match value {
        serde_json::Value::Null => any_value::Value::StringValue("null".to_string()),
        serde_json::Value::Bool(value) => any_value::Value::BoolValue(value),
        serde_json::Value::String(value) => any_value::Value::StringValue(value),
        serde_json::Value::Number(value) => {
            if let Some(value) = value.as_i64() {
                any_value::Value::IntValue(value)
            } else if let Some(value) = value.as_u64() {
                match i64::try_from(value) {
                    Ok(value) => any_value::Value::IntValue(value),
                    Err(_) => any_value::Value::StringValue(value.to_string()),
                }
            } else {
                any_value::Value::DoubleValue(value.as_f64().expect("valid JSON number"))
            }
        }
        serde_json::Value::Array(values) => any_value::Value::ArrayValue(ArrayValue {
            values: values.into_iter().map(json_to_any_value).collect(),
        }),
        serde_json::Value::Object(values) => any_value::Value::KvlistValue(KeyValueList {
            values: values
                .into_iter()
                .map(|(key, value)| KeyValue {
                    key,
                    value: Some(json_to_any_value(value)),
                })
                .collect(),
        }),
    };

    AnyValue { value: Some(value) }
}

fn otlp_severity(severity: LogSeverity) -> (SeverityNumber, &'static str) {
    match severity {
        LogSeverity::Verbose => (SeverityNumber::Trace, "Verbose"),
        LogSeverity::Debug => (SeverityNumber::Debug, "Debug"),
        LogSeverity::Info => (SeverityNumber::Info, "Info"),
        LogSeverity::Warn => (SeverityNumber::Warn, "Warn"),
        LogSeverity::Error => (SeverityNumber::Error, "Error"),
        LogSeverity::Critical => (SeverityNumber::Fatal, "Critical"),
    }
}

/// Builds an OTLP export request for exporter delivery, request-building tests,
/// and Criterion mapping benchmarks.
pub fn build_export_request(logs: &[ProcessedLog]) -> ExportLogsServiceRequest {
    let mut groups: BTreeMap<(&str, &str), Vec<&ProcessedLog>> = BTreeMap::new();
    for log in logs {
        groups
            .entry((&log.application_name, &log.subsystem_name))
            .or_default()
            .push(log);
    }

    let resource_logs = groups
        .into_iter()
        .map(|((application, subsystem), logs)| {
            let attributes = vec![
                KeyValue {
                    key: "cx.application.name".to_string(),
                    value: Some(AnyValue {
                        value: Some(any_value::Value::StringValue(application.to_string())),
                    }),
                },
                KeyValue {
                    key: "cx.subsystem.name".to_string(),
                    value: Some(AnyValue {
                        value: Some(any_value::Value::StringValue(subsystem.to_string())),
                    }),
                },
            ];
            let log_records = logs
                .into_iter()
                .map(|log| {
                    let (severity_number, severity_text) = otlp_severity(log.severity);
                    let timestamp = log.timestamp.unix_timestamp_nanos() as u64;
                    LogRecord {
                        time_unix_nano: timestamp,
                        observed_time_unix_nano: timestamp,
                        severity_number: severity_number.into(),
                        severity_text: severity_text.to_string(),
                        body: Some(json_to_any_value(log.body.clone())),
                        attributes: Vec::new(),
                        dropped_attributes_count: 0,
                        flags: 0,
                        trace_id: Vec::new(),
                        span_id: Vec::new(),
                    }
                })
                .collect();

            ResourceLogs {
                resource: Some(Resource {
                    attributes,
                    dropped_attributes_count: 0,
                }),
                scope_logs: vec![ScopeLogs {
                    scope: Some(InstrumentationScope {
                        name: env!("CARGO_PKG_NAME").to_string(),
                        version: env!("CARGO_PKG_VERSION").to_string(),
                        attributes: Vec::new(),
                        dropped_attributes_count: 0,
                    }),
                    log_records,
                    schema_url: String::new(),
                }],
                schema_url: String::new(),
            }
        })
        .collect();

    ExportLogsServiceRequest { resource_logs }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logs::{
        exporter::{LogExportError, LogExporter},
        model::{LogSeverity, ProcessedLog},
    };
    use async_trait::async_trait;
    use cx_sdk_otlp::otlp::proto::{
        collector::logs::v1::{ExportLogsPartialSuccess, ExportLogsServiceResponse},
        common::v1::any_value,
    };
    use std::{
        collections::VecDeque,
        sync::{Arc, Mutex},
    };
    use time::OffsetDateTime;

    struct RecordingTransport {
        requests: Mutex<Vec<ExportLogsServiceRequest>>,
        response: ExportLogsServiceResponse,
    }

    impl Default for RecordingTransport {
        fn default() -> Self {
            Self {
                requests: Mutex::new(Vec::new()),
                response: ExportLogsServiceResponse::default(),
            }
        }
    }

    #[async_trait]
    impl OtlpTransport for RecordingTransport {
        async fn send(
            &self,
            request: ExportLogsServiceRequest,
        ) -> Result<OtlpTransportResponse, LogExportError> {
            self.requests.lock().unwrap().push(request);
            Ok(OtlpTransportResponse {
                response: self.response.clone(),
                attempt_count: 1,
            })
        }
    }

    enum ScriptedOutcome {
        Success(ExportLogsServiceResponse),
        TransportFailure,
    }

    struct ScriptedTransport {
        requests: Mutex<Vec<ExportLogsServiceRequest>>,
        outcomes: Mutex<VecDeque<ScriptedOutcome>>,
    }

    impl ScriptedTransport {
        fn new(outcomes: impl IntoIterator<Item = ScriptedOutcome>) -> Self {
            Self {
                requests: Mutex::new(Vec::new()),
                outcomes: Mutex::new(outcomes.into_iter().collect()),
            }
        }
    }

    #[async_trait]
    impl OtlpTransport for ScriptedTransport {
        async fn send(
            &self,
            request: ExportLogsServiceRequest,
        ) -> Result<OtlpTransportResponse, LogExportError> {
            self.requests.lock().unwrap().push(request);
            match self
                .outcomes
                .lock()
                .unwrap()
                .pop_front()
                .expect("scripted transport outcome")
            {
                ScriptedOutcome::Success(response) => Ok(OtlpTransportResponse {
                    response,
                    attempt_count: 1,
                }),
                ScriptedOutcome::TransportFailure => Err(LogExportError::OtlpResponse(
                    OtlpResponseError::new(OtlpFailureClassification::Unclassified, "unavailable"),
                )),
            }
        }
    }

    struct FailingTransport;

    #[async_trait]
    impl OtlpTransport for FailingTransport {
        async fn send(
            &self,
            _request: ExportLogsServiceRequest,
        ) -> Result<OtlpTransportResponse, LogExportError> {
            Err(LogExportError::OtlpResponse(OtlpResponseError::new(
                OtlpFailureClassification::Unclassified,
                "unavailable",
            )))
        }
    }

    fn log(app: &str, sub: &str, body: serde_json::Value) -> ProcessedLog {
        ProcessedLog {
            application_name: app.to_string(),
            subsystem_name: sub.to_string(),
            body,
            severity: LogSeverity::Info,
            timestamp: OffsetDateTime::UNIX_EPOCH,
        }
    }

    #[test]
    fn otlp_channel_config_uses_webpki_roots() {
        let config = otlp_channel_config("https://collector.example.com:443".to_string());

        assert_eq!(
            config.tls_roots,
            Some(cx_sdk_core::channel::TlsRoots::WebPki)
        );
    }

    #[test]
    fn groups_resources_without_splitting_requests() {
        let request = build_export_request(&[
            log("a", "one", serde_json::json!({"message": "first"})),
            log("a", "one", serde_json::json!({"message": "second"})),
            log("b", "two", serde_json::json!({"message": "third"})),
        ]);

        assert_eq!(request.resource_logs.len(), 2);
        let record_count: usize = request
            .resource_logs
            .iter()
            .flat_map(|resource| &resource.scope_logs)
            .map(|scope| scope.log_records.len())
            .sum();
        assert_eq!(record_count, 3);
    }

    #[test]
    fn preserves_structured_json_body() {
        let value = json_to_any_value(serde_json::json!({
            "message": "hello",
            "attempt": 2,
            "ok": true,
            "missing": null
        }));
        assert!(matches!(
            value.value,
            Some(any_value::Value::KvlistValue(_))
        ));
    }

    #[test]
    fn preserves_nested_arrays_and_objects() {
        let value = json_to_any_value(serde_json::json!({
            "items": [
                {"enabled": true, "labels": ["one", null]},
                [1, {"nested": "value"}]
            ]
        }));
        let Some(any_value::Value::KvlistValue(root)) = value.value else {
            panic!("root must be an OTLP key-value list");
        };
        let Some(any_value::Value::ArrayValue(items)) = root.values[0]
            .value
            .as_ref()
            .and_then(|value| value.value.as_ref())
        else {
            panic!("items must be an OTLP array");
        };
        let Some(any_value::Value::KvlistValue(first)) = items.values[0].value.as_ref() else {
            panic!("first item must be an OTLP key-value list");
        };
        let Some(any_value::Value::ArrayValue(labels)) = first.values[1]
            .value
            .as_ref()
            .and_then(|value| value.value.as_ref())
        else {
            panic!("labels must be an OTLP array");
        };
        assert!(matches!(
            labels.values[1].value.as_ref(),
            Some(any_value::Value::StringValue(value)) if value == "null"
        ));
    }

    #[test]
    fn maps_numeric_boundaries_without_loss() {
        let values = [
            (
                serde_json::json!(i64::MIN),
                any_value::Value::IntValue(i64::MIN),
            ),
            (
                serde_json::json!(i64::MAX),
                any_value::Value::IntValue(i64::MAX),
            ),
            (
                serde_json::json!(u64::MAX),
                any_value::Value::StringValue(u64::MAX.to_string()),
            ),
        ];

        for (input, expected) in values {
            assert_eq!(json_to_any_value(input).value, Some(expected));
        }
        assert!(matches!(
            json_to_any_value(serde_json::json!(1.5)).value,
            Some(any_value::Value::DoubleValue(value)) if value == 1.5
        ));
    }

    #[test]
    fn maps_severity_and_timestamps() {
        let timestamp = OffsetDateTime::from_unix_timestamp_nanos(1_234_567_890).unwrap();
        let cases = [
            (LogSeverity::Verbose, SeverityNumber::Trace, "Verbose"),
            (LogSeverity::Debug, SeverityNumber::Debug, "Debug"),
            (LogSeverity::Info, SeverityNumber::Info, "Info"),
            (LogSeverity::Warn, SeverityNumber::Warn, "Warn"),
            (LogSeverity::Error, SeverityNumber::Error, "Error"),
            (LogSeverity::Critical, SeverityNumber::Fatal, "Critical"),
        ];

        for (severity, expected_number, expected_text) in cases {
            let mut input = log("app", "sub", serde_json::json!("body"));
            input.severity = severity;
            input.timestamp = timestamp;
            let request = build_export_request(&[input]);
            let record = &request.resource_logs[0].scope_logs[0].log_records[0];
            assert_eq!(record.severity_number, expected_number as i32);
            assert_eq!(record.severity_text, expected_text);
            assert_eq!(record.time_unix_nano, 1_234_567_890);
            assert_eq!(record.observed_time_unix_nano, 1_234_567_890);
        }
    }

    #[tokio::test]
    async fn sdk_attempt_listener_counts_attempts_in_the_current_export() {
        OTLP_ATTEMPT_COUNT
            .scope(std::cell::Cell::new(0), async {
                let listener = AttemptListener;
                listener.on_request_completed(
                    cx_sdk_otlp::RequestOutcome::Failure,
                    Duration::from_millis(1),
                );
                listener.on_request_completed(
                    cx_sdk_otlp::RequestOutcome::Success,
                    Duration::from_millis(1),
                );

                assert_eq!(OTLP_ATTEMPT_COUNT.with(Cell::get), 2);
            })
            .await;
    }

    #[tokio::test]
    async fn splits_oversized_batches_without_splitting_by_resource() {
        let transport = Arc::new(RecordingTransport::default());
        let logs = vec![
            log(
                "app",
                "sub",
                serde_json::json!({"message": "a".repeat(180)}),
            ),
            log(
                "app",
                "sub",
                serde_json::json!({"message": "b".repeat(180)}),
            ),
        ];
        let exporter = OtlpGrpcExporter::with_transport(
            transport.clone(),
            build_export_request(&logs[..1]).encoded_len(),
        );

        exporter.export(logs).await.unwrap();

        let requests = transport.requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        assert!(requests
            .iter()
            .all(|request| request.resource_logs.len() == 1));
    }

    #[tokio::test]
    async fn later_split_transport_failure_fails_the_logical_batch() {
        let transport = Arc::new(ScriptedTransport::new([
            ScriptedOutcome::Success(ExportLogsServiceResponse::default()),
            ScriptedOutcome::TransportFailure,
            ScriptedOutcome::Success(ExportLogsServiceResponse::default()),
        ]));
        let logs = vec![
            log(
                "app",
                "sub",
                serde_json::json!({"message": "a".repeat(180)}),
            ),
            log(
                "app",
                "sub",
                serde_json::json!({"message": "b".repeat(180)}),
            ),
            log(
                "app",
                "sub",
                serde_json::json!({"message": "c".repeat(180)}),
            ),
        ];
        let max_request_bytes = build_export_request(&logs[..1]).encoded_len();
        let exporter = OtlpGrpcExporter::with_transport(transport.clone(), max_request_bytes);

        let error = exporter.export(logs).await.unwrap_err();

        assert!(matches!(error, LogExportError::OtlpResponse(_)));
        let requests = transport.requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        assert_ne!(requests[0], requests[1]);
    }

    #[tokio::test]
    async fn rejects_one_record_larger_than_the_limit() {
        let transport = Arc::new(RecordingTransport::default());
        let exporter = OtlpGrpcExporter::with_transport(transport, 32);

        let error = exporter
            .export(vec![log(
                "app",
                "sub",
                serde_json::json!({"message": "too large"}),
            )])
            .await
            .unwrap_err();

        assert!(matches!(error, LogExportError::OversizedRecord));
    }

    #[tokio::test]
    async fn validates_all_partitions_before_sending_any_request() {
        let transport = Arc::new(RecordingTransport::default());
        let valid = log("app", "sub", serde_json::json!({"message": "valid"}));
        let oversized = log(
            "app",
            "sub",
            serde_json::json!({"message": "x".repeat(1_024)}),
        );
        let max_request_bytes = build_export_request(std::slice::from_ref(&valid)).encoded_len();
        let exporter = OtlpGrpcExporter::with_transport(transport.clone(), max_request_bytes);

        let error = exporter.export(vec![valid, oversized]).await.unwrap_err();

        assert!(matches!(error, LogExportError::OversizedRecord));
        assert!(transport.requests.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn sends_multiple_resources_in_one_request_when_under_limit() {
        let transport = Arc::new(RecordingTransport::default());
        let logs = vec![
            log("app-a", "sub-a", serde_json::json!({"message": "first"})),
            log("app-b", "sub-b", serde_json::json!({"message": "second"})),
        ];
        let max_request_bytes = build_export_request(&logs).encoded_len();
        let exporter = OtlpGrpcExporter::with_transport(transport.clone(), max_request_bytes);

        exporter.export(logs).await.unwrap();

        let requests = transport.requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].resource_logs.len(), 2);
    }

    #[tokio::test]
    async fn accepts_a_request_exactly_at_the_encoded_size_limit() {
        let transport = Arc::new(RecordingTransport::default());
        let logs = vec![log("app", "sub", serde_json::json!({"message": "hello"}))];
        let exporter = OtlpGrpcExporter::with_transport(
            transport.clone(),
            build_export_request(&logs).encoded_len(),
        );

        exporter.export(logs).await.unwrap();

        assert_eq!(transport.requests.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn does_not_send_empty_batches() {
        let transport = Arc::new(RecordingTransport::default());
        let exporter = OtlpGrpcExporter::with_transport(transport.clone(), 256);

        exporter.export(Vec::new()).await.unwrap();

        assert!(transport.requests.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn partial_rejection_fails_the_logical_batch_without_exposing_server_message() {
        const SENTINEL_MESSAGE: &str = "server-controlled-sensitive-detail";
        let transport = Arc::new(RecordingTransport {
            requests: Mutex::new(Vec::new()),
            response: ExportLogsServiceResponse {
                partial_success: Some(ExportLogsPartialSuccess {
                    rejected_log_records: 1,
                    error_message: SENTINEL_MESSAGE.to_string(),
                }),
            },
        });
        let exporter = OtlpGrpcExporter::with_transport(transport.clone(), 4 * 1024 * 1024);

        let error = exporter
            .export(vec![log(
                "app",
                "sub",
                serde_json::json!({"message": "hello"}),
            )])
            .await
            .unwrap_err();
        let display = error.to_string();
        let debug = format!("{error:?}");

        assert!(matches!(
            error,
            LogExportError::PartialRejection {
                rejected_log_records: 1
            }
        ));
        assert!(!display.contains(SENTINEL_MESSAGE));
        assert!(!debug.contains(SENTINEL_MESSAGE));
        assert_eq!(transport.requests.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn warning_only_partial_success_succeeds() {
        let transport = Arc::new(RecordingTransport {
            requests: Mutex::new(Vec::new()),
            response: ExportLogsServiceResponse {
                partial_success: Some(ExportLogsPartialSuccess {
                    rejected_log_records: 0,
                    error_message: "server warning".to_string(),
                }),
            },
        });
        let exporter = OtlpGrpcExporter::with_transport(transport.clone(), 4 * 1024 * 1024);

        exporter
            .export(vec![log(
                "app",
                "sub",
                serde_json::json!({"message": "hello"}),
            )])
            .await
            .unwrap();

        assert_eq!(transport.requests.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn empty_partial_success_succeeds() {
        let transport = Arc::new(RecordingTransport {
            requests: Mutex::new(Vec::new()),
            response: ExportLogsServiceResponse {
                partial_success: Some(ExportLogsPartialSuccess {
                    rejected_log_records: 0,
                    error_message: String::new(),
                }),
            },
        });
        let exporter = OtlpGrpcExporter::with_transport(transport.clone(), 4 * 1024 * 1024);

        exporter
            .export(vec![log(
                "app",
                "sub",
                serde_json::json!({"message": "hello"}),
            )])
            .await
            .unwrap();

        assert_eq!(transport.requests.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn later_split_partial_rejection_fails_the_logical_batch() {
        let transport = Arc::new(ScriptedTransport::new([
            ScriptedOutcome::Success(ExportLogsServiceResponse::default()),
            ScriptedOutcome::Success(ExportLogsServiceResponse {
                partial_success: Some(ExportLogsPartialSuccess {
                    rejected_log_records: 1,
                    error_message: "one record rejected".to_string(),
                }),
            }),
            ScriptedOutcome::Success(ExportLogsServiceResponse::default()),
        ]));
        let logs = vec![
            log(
                "app",
                "sub",
                serde_json::json!({"message": "a".repeat(180)}),
            ),
            log(
                "app",
                "sub",
                serde_json::json!({"message": "b".repeat(180)}),
            ),
            log(
                "app",
                "sub",
                serde_json::json!({"message": "c".repeat(180)}),
            ),
        ];
        let max_request_bytes = build_export_request(&logs[..1]).encoded_len();
        let exporter = OtlpGrpcExporter::with_transport(transport.clone(), max_request_bytes);

        let error = exporter.export(logs).await.unwrap_err();

        assert!(matches!(
            error,
            LogExportError::PartialRejection {
                rejected_log_records: 1
            }
        ));
        assert_eq!(transport.requests.lock().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn propagates_transport_errors() {
        let exporter =
            OtlpGrpcExporter::with_transport(Arc::new(FailingTransport), 4 * 1024 * 1024);

        let error = exporter
            .export(vec![log(
                "app",
                "sub",
                serde_json::json!({"message": "hello"}),
            )])
            .await
            .unwrap_err();

        assert!(matches!(error, LogExportError::OtlpResponse(_)));
        assert_eq!(
            error.to_string(),
            "OTLP log export failed (classification=unclassified_error, grpc_status=unavailable)"
        );
    }
}
