use std::{
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
    OtlpExporter,
};
use prost_014::Message;

use crate::logs::{
    exporter::{LogExportError, LogExporter},
    model::{LogSeverity, ProcessedLog},
};

#[async_trait]
trait OtlpTransport: Send + Sync {
    async fn send(
        &self,
        request: ExportLogsServiceRequest,
    ) -> Result<ExportLogsServiceResponse, LogExportError>;
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
    ) -> Result<ExportLogsServiceResponse, LogExportError> {
        self.exporter
            .export(request, &self.auth)
            .await
            .map_err(|error| LogExportError::OtlpResponse(error.to_string()))
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
        let exporter = OtlpLogExporterGrpc::builder()
            .with_channel_config(ChannelConfig::new(endpoint))
            .with_backoff_config(BackoffConfig {
                initial_delay: Duration::from_millis(100),
                max_delay: Duration::from_secs(10),
                max_elapsed_time: Duration::from_secs(max_elapsed_time),
            })
            .with_gzip_compression(true)
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
            let response = self.transport.send(request).await?;

            if let Some(partial) = response.partial_success {
                if partial.rejected_log_records > 0 || !partial.error_message.is_empty() {
                    tracing::warn!(
                        rejected_log_records = partial.rejected_log_records,
                        error_message_present = !partial.error_message.is_empty(),
                        "OTLP collector partially accepted a log request"
                    );
                }
            }

            tracing::info!(
                record_count,
                resource_count,
                encoded_bytes,
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
    use std::sync::{Arc, Mutex};
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
        ) -> Result<ExportLogsServiceResponse, LogExportError> {
            self.requests.lock().unwrap().push(request);
            Ok(self.response.clone())
        }
    }

    struct FailingTransport;

    #[async_trait]
    impl OtlpTransport for FailingTransport {
        async fn send(
            &self,
            _request: ExportLogsServiceRequest,
        ) -> Result<ExportLogsServiceResponse, LogExportError> {
            Err(LogExportError::OtlpResponse(
                "transport failure".to_string(),
            ))
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
    async fn partial_success_does_not_resend_the_complete_request() {
        let transport = Arc::new(RecordingTransport {
            requests: Mutex::new(Vec::new()),
            response: ExportLogsServiceResponse {
                partial_success: Some(ExportLogsPartialSuccess {
                    rejected_log_records: 1,
                    error_message: "one record rejected".to_string(),
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

        assert!(
            matches!(error, LogExportError::OtlpResponse(message) if message == "transport failure")
        );
    }
}
