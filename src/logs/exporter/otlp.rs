use std::collections::BTreeMap;

use cx_sdk_otlp::otlp::proto::{
    collector::logs::v1::ExportLogsServiceRequest,
    common::v1::{any_value, AnyValue, ArrayValue, InstrumentationScope, KeyValue, KeyValueList},
    logs::v1::{LogRecord, ResourceLogs, ScopeLogs, SeverityNumber},
    resource::v1::Resource,
};

use crate::logs::model::{LogSeverity, ProcessedLog};

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
    use crate::logs::model::{LogSeverity, ProcessedLog};
    use cx_sdk_otlp::otlp::proto::common::v1::any_value;
    use time::OffsetDateTime;

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
}
