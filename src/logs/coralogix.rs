use crate::logs::config::Config;
use crate::logs::*;
use cx_sdk_rest_logs::auth::AuthData;
use cx_sdk_rest_logs::model::{LogSinglesEntry, LogSinglesRequest, Severity};
use cx_sdk_rest_logs::DynLogExporter;
use fancy_regex::Regex;
use futures::stream::{StreamExt, TryStreamExt};
use itertools::Itertools;
use once_cell::sync::Lazy;
use process;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::env;
use std::iter::IntoIterator;
use std::time::Instant;
use std::vec::Vec;
use time::OffsetDateTime;
use tracing::{debug, error, info};

// Add these static regex patterns at the top of the file with the other statics
static JSON_EVALUATION_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"\{\{\s*\$\.([a-zA-Z0-9._]+)\s*\}\}"#)
        .expect("Failed to create JSON evaluation regex")
});

pub async fn process_batches(
    logs: Vec<String>,
    configured_app_name: &str,
    configured_sub_name: &str,
    config: &Config,
    mctx: &process::MetadataContext,
    exporter: DynLogExporter,
) -> Result<(), Error> {
    let logs: Vec<String> = logs
        .into_iter()
        .filter(|log| !log.trim().is_empty())
        .collect();
    let number_of_logs = logs.len();
    if number_of_logs == 0 {
        info!("No logs to send");
        return Ok(());
    }

    let batches = into_batches_of_estimated_size(logs, config);

    info!(
        "Will send {} logs in {} batches. (On average {} logs per batch)",
        number_of_logs,
        batches.len(),
        number_of_logs / batches.len()
    );

    let auth_data = AuthData::from(&config.api_key);
    // Process and send batches concurrently, but not more than 5 simultaneously.
    let results = futures::stream::iter(batches)
        .map(|batch| {
            let batch = batch
                .into_iter()
                .map(|log| {
                    convert_to_log_entry(
                        log,
                        configured_app_name,
                        configured_sub_name,
                        mctx,
                        config,
                    )
                })
                .collect_vec();
            send_logs(exporter.clone(), batch, &auth_data)
        })
        .buffer_unordered(5)
        .inspect_err(|error| error!(?error, "Failed to send logs"))
        .collect::<Vec<_>>()
        .await;

    // Fail if at least one operation failed
    results.into_iter().collect::<Result<Vec<()>, Error>>()?;

    Ok(())
}

fn into_batches_of_estimated_size(logs: Vec<String>, config: &Config) -> Vec<Vec<String>> {
    // The hard limit is 10MB. We're aiming for 2MB, so we don't need to be precise with the size estimation.
    let target_batch_size = config.batches_max_size * 1024 * 1024; // 4MB
    let overhead_per_log_estimation = 200;

    let (mut batches, batch, _) = logs
        .into_iter()
        .fold::<(Vec<Vec<String>>, Vec<String>, usize), _>(
            (Vec::new(), Vec::new(), 0),
            |acc, log| {
                let (mut batches, mut batch, size) = acc;

                let new_size = size + log.len() + overhead_per_log_estimation;
                if new_size <= target_batch_size {
                    batch.push(log);
                    (batches, batch, new_size)
                } else {
                    batches.push(std::mem::take(&mut batch));
                    let new_size = log.len() + overhead_per_log_estimation;
                    batch.push(log);
                    (batches, batch, new_size)
                }
            },
        );
    if !batch.is_empty() {
        batches.push(batch);
    }
    batches
}
#[derive(Serialize, Deserialize, Default)]
struct JsonMessage {
    message: Value,
    #[serde(skip_serializing_if = "Option::is_none", rename = "s3.object.key")]
    s3_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "s3.bucket")]
    s3_bucket: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "cw.log.group")]
    cw_log_group: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "cw.log.stream")]
    cw_log_stream: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "cw.owner")]
    cw_owner: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "kinesis.event.id")]
    kinesis_event_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "kinesis.event.name")]
    kinesis_event_name: Option<String>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        rename = "kinesis.event.source"
    )]
    kinesis_event_source: Option<String>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        rename = "kinesis.event.source_arn"
    )]
    kinesis_event_source_arn: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "kafka.topic")]
    kafka_topic: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "ecr.scan.id")]
    ecr_scan_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "ecr.scan.source")]
    ecr_scan_source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "sqs.event.source")]
    sqs_event_source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "sqs.event.id")]
    sqs_event_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "cw.tags")]
    cw_tags: Option<Value>,

    // to be deprecated
    #[serde(skip_serializing_if = "Option::is_none")]
    stream_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    loggroup_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    bucket_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    key_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    topic_name: Option<String>,
    #[serde(flatten)]
    custom_metadata: HashMap<String, String>,
}

impl From<&process::MetadataContext> for JsonMessage {
    fn from(mctx: &process::MetadataContext) -> Self {
        Self {
            message: Value::Null,
            s3_key: mctx.get("s3.object.key"),
            s3_bucket: mctx.get("s3.bucket"),
            cw_log_group: mctx.get("cw.log.group"),
            cw_log_stream: mctx.get("cw.log.stream"),
            cw_owner: mctx.get("cw.owner"),
            kinesis_event_id: mctx.get("kinesis.event.id"),
            kinesis_event_name: mctx.get("kinesis.event.name"),
            kinesis_event_source: mctx.get("kinesis.event.source"),
            kinesis_event_source_arn: mctx.get("kinesis.event.source_arn"),
            kafka_topic: mctx.get("kafka.topic"),
            ecr_scan_id: mctx.get("ecr.scan.id"),
            ecr_scan_source: mctx.get("ecr.scan.source"),
            sqs_event_source: mctx.get("sqs.event.source"),
            sqs_event_id: mctx.get("sqs.event.id"),
            cw_tags: mctx.get("cw.tags").and_then(|tags_json| {
                serde_json::from_str::<Value>(&tags_json).ok()
            }),

            // to be deprecated
            stream_name: mctx.get("cw.log.stream"),
            loggroup_name: mctx.get("cw.log.group"),
            bucket_name: mctx.get("s3.bucket"),
            key_name: mctx.get("s3.object.key"),
            topic_name: mctx.get("kafka.topic"),
            custom_metadata: HashMap::new(),
        }
    }
}

impl JsonMessage {
    fn new(message: Value) -> Self {
        Self {
            message,
            s3_key: None,
            s3_bucket: None,
            cw_log_group: None,
            cw_log_stream: None,
            cw_owner: None,
            kinesis_event_id: None,
            kinesis_event_name: None,
            kinesis_event_source: None,
            kinesis_event_source_arn: None,
            kafka_topic: None,
            ecr_scan_id: None,
            ecr_scan_source: None,
            sqs_event_source: None,
            sqs_event_id: None,
            cw_tags: None,
            stream_name: None,
            loggroup_name: None,
            bucket_name: None,
            key_name: None,
            topic_name: None,
            custom_metadata: HashMap::new(),
        }
    }

    fn with_selected_metadata(
        mut self,
        mctx: &process::MetadataContext,
        selected_metadata_keys: Vec<&str>,
    ) -> Self {
        for key in selected_metadata_keys {
            match key {
                "s3.object.key" => self.s3_key = mctx.get("s3.object.key"),
                "s3.bucket" => self.s3_bucket = mctx.get("s3.bucket"),
                "cw.log.group" => self.cw_log_group = mctx.get("cw.log.group"),
                "cw.log.stream" => self.cw_log_stream = mctx.get("cw.log.stream"),
                "cw.owner" => self.cw_owner = mctx.get("cw.owner"),
                "kinesis.event.id" => self.kinesis_event_id = mctx.get("kinesis.event.id"),
                "kinesis.event.name" => self.kinesis_event_name = mctx.get("kinesis.event.name"),
                "kinesis.event.source" => {
                    self.kinesis_event_source = mctx.get("kinesis.event.source")
                }
                "kinesis.event.source_arn" => {
                    self.kinesis_event_source_arn = mctx.get("kinesis.event.source_arn")
                }
                "kafka.topic" => self.kafka_topic = mctx.get("kafka.topic"),
                "ecr.scan.id" => self.ecr_scan_id = mctx.get("ecr.scan.id"),
                "ecr.scan.source" => self.ecr_scan_source = mctx.get("ecr.scan.source"),
                "sqs.event.source" => self.sqs_event_source = mctx.get("sqs.event.source"),
                "sqs.event.id" => self.sqs_event_id = mctx.get("sqs.event.id"),
                "stream_name" => self.stream_name = mctx.get("cw.log.stream"),
                "loggroup_name" => self.loggroup_name = mctx.get("cw.log.group"),
                "bucket_name" => self.bucket_name = mctx.get("s3.bucket"),
                "key_name" => self.key_name = mctx.get("s3.object.key"),
                "topic_name" => self.topic_name = mctx.get("kafka.topic"),
                _ => {}
            }
        }
        self
    }

    fn has_metadata(&self) -> bool {
        self.s3_key.is_some()
            || self.s3_bucket.is_some()
            || self.cw_log_group.is_some()
            || self.cw_log_stream.is_some()
            || self.cw_owner.is_some()
            || self.kinesis_event_id.is_some()
            || self.kinesis_event_name.is_some()
            || self.kinesis_event_source.is_some()
            || self.kinesis_event_source_arn.is_some()
            || self.kafka_topic.is_some()
            || self.ecr_scan_id.is_some()
            || self.ecr_scan_source.is_some()
            || self.sqs_event_source.is_some()
            || self.sqs_event_id.is_some()
            || self.cw_tags.is_some()
          
            // to be deprecated
            || self.stream_name.is_some()
            || self.loggroup_name.is_some()
            || self.bucket_name.is_some()
            || self.key_name.is_some()
            || self.topic_name.is_some()
        
            || !self.custom_metadata.is_empty()
    }
}

fn extract_json_value(log: &str, template: &str) -> Option<String> {
    // First check if this is a template pattern
    if let Ok(Some(captures)) = JSON_EVALUATION_REGEX.captures(template) {
        if let Some(key_match) = captures.get(1) {
            let key_path = key_match.as_str();
            return extract_json_path_value(log, key_path);
        }
    }

    // If not a template, treat as direct path
    extract_json_path_value(log, template)
}

fn extract_json_path_value(log: &str, key_path: &str) -> Option<String> {
    // Try to parse the log as JSON
    if let Ok(json_value) = serde_json::from_str::<Value>(log) {
        // Split the key path by dots to handle nested objects
        let keys: Vec<&str> = key_path.split('.').collect();

        // Navigate through the JSON structure
        let mut current = &json_value;
        for key in keys {
            match current.get(key) {
                Some(value) => current = value,
                None => return None,
            }
        }

        // Convert the final value to a string
        match current {
            Value::String(s) => Some(s.clone()),
            Value::Number(n) => Some(n.to_string()),
            Value::Bool(b) => Some(b.to_string()),
            _ => None,
        }
    } else {
        None
    }
}

fn convert_to_log_entry(
    log: String,
    configured_app_name: &str,
    configured_sub_name: &str,
    mctx: &process::MetadataContext,
    config: &Config,
) -> LogSinglesEntry<Value> {
    let now = OffsetDateTime::now_utc();

    let application_name = extract_json_value(&log, configured_app_name).unwrap_or_else(|| {
        mctx.evaluate(configured_app_name.to_string())
            .unwrap_or_else(|e| {
                tracing::warn!("application name dynamic parsing failed: {}", e);
                // If it's a template that references a specific metadata key, use defaults
                // Only use fallback hierarchy for non-template values
                if configured_app_name.starts_with("{{") && configured_app_name.ends_with("}}") {
                    "unknown-application".to_string()
                } else {
                    // For non-template values, return the original input string
                    configured_app_name.to_string()
                }
            })
    });

    let subsystem_name = extract_json_value(&log, configured_sub_name).unwrap_or_else(|| {
        mctx.evaluate(configured_sub_name.to_string())
            .unwrap_or_else(|e| {
                tracing::warn!("subsystem name dynamic parsing failed: {}", e);
                // If it's a template that references a specific metadata key, use defaults
                // Only use fallback hierarchy for non-template values
                if configured_sub_name.starts_with("{{") && configured_sub_name.ends_with("}}") {
                    "unknown-subsystem".to_string()
                } else {
                    // For non-template values, return the original input string
                    configured_sub_name.to_string()
                }
            })
    });

    tracing::debug!("App Name: {}", &application_name);
    tracing::debug!("Sub Name: {}", &subsystem_name);
    let severity = get_severity_level(&log);
    // let stream_name = metadata_instance.stream_name.clone();
    // let topic_name = metadata_instance.topic_name.clone();
    // let loggroup_name = metadata_instance.log_group.clone();
    tracing::debug!("Severity: {:?}", severity);

    let msg = match serde_json::from_str(&log) {
        Ok(value) => value,
        Err(_) => Value::String(log),
    };

    let add_metadata: Vec<&str> = config.add_metadata.split(',').map(|s| s.trim()).collect();
    tracing::debug!("add_metadata: {:?}", add_metadata);
    let mut message = JsonMessage::new(msg).with_selected_metadata(mctx, add_metadata);

    if let Ok(custom_metadata_str) = env::var("CUSTOM_METADATA") {
        debug!("Custom metadata STR: {}", custom_metadata_str);
        let mut metadata = HashMap::new();
        let pairs = custom_metadata_str.split(',');

        for pair in pairs {
            let split_pair: Vec<&str> = pair.split('=').collect();
            match split_pair.as_slice() {
                [key, value] => {
                    metadata.insert(key.to_string(), value.to_string());
                }
                _ => {
                    error!("Failed to split key-value pair: {}", pair);
                    continue;
                }
            }
        }

        if !metadata.is_empty() {
            debug!("Custom metadata: {:?}", metadata);
            message.custom_metadata = metadata;
        }
    }
    debug!("Message metadata: {:?}", message.custom_metadata);
    
    // Get tags if they exist
    let tags_value = mctx.get("cw.tags").and_then(|tags_json| {
        serde_json::from_str::<Value>(&tags_json).ok()
    });
    
    // Build the body with tags at root level
    let body = if message.has_metadata() {
        // If there's metadata, use the JsonMessage structure
        let mut body_value = serde_json::to_value(&message).unwrap_or(message.message);
        
        // If tags exist, merge them at root level
        if let Some(tags) = tags_value {
            if let Value::Object(ref mut body_map) = body_value {
                body_map.insert("cw.tags".to_string(), tags);
            }
        }
        body_value
    } else {
        // No metadata - check if we need to wrap string message or add tags
        match (message.message, tags_value) {
            (Value::String(text), Some(tags)) => {
                // String message with tags - wrap in object with "text" key and add tags at root
                let mut obj = serde_json::Map::new();
                obj.insert("text".to_string(), Value::String(text));
                obj.insert("cw.tags".to_string(), tags);
                Value::Object(obj)
            }
            (msg, Some(tags)) => {
                // JSON message with tags - merge tags into the object at root level
                if let Value::Object(mut obj) = msg {
                    obj.insert("cw.tags".to_string(), tags);
                    Value::Object(obj)
                } else {
                    msg
                }
            }
            (msg, None) => {
                // No tags - return message as-is (backward compatibility)
                msg
            }
        }
    };

    LogSinglesEntry {
        application_name,
        subsystem_name,
        computer_name: None,
        severity,
        body,
        timestamp: now,
        class_name: None,
        method_name: None,
        thread_id: None,
        category: None,
    }
}

async fn send_logs(
    exporter: DynLogExporter,
    resource_logs: Vec<LogSinglesEntry<Value>>,
    auth_data: &AuthData,
) -> Result<(), Error> {
    let number_of_logs = resource_logs.len();
    let start_time = Instant::now();
    tracing::debug!("Logs to send: {:?}", resource_logs);
    let request = LogSinglesRequest {
        entries: resource_logs,
    };
    exporter
        .as_ref()
        .export_singles_jsons(request, auth_data)
        .await?;
    tracing::info!(
        "Delivered {} log records to Coralogix in {}ms.",
        number_of_logs,
        start_time.elapsed().as_millis()
    );
    Ok(())
}

// fn dynamic_metadata_value(mctx: process::MetadataContext, value: String) -> String {}

fn get_severity_level(message: &str) -> Severity {
    let mut severity: Severity = Severity::Info;

    if message.to_lowercase().contains("debug") {
        severity = Severity::Debug;
    }
    if message.to_lowercase().contains("verbose") || message.to_lowercase().contains("trace") {
        severity = Severity::Verbose;
    }
    if message.to_lowercase().contains("info") {
        severity = Severity::Info;
    }
    if message.to_lowercase().contains("warn") || message.to_lowercase().contains("warning") {
        severity = Severity::Warn;
    }
    if message.to_lowercase().contains("error") {
        severity = Severity::Error;
    }
    if message.to_lowercase().contains("fatal")
        || message.to_lowercase().contains("panic")
        || message.to_lowercase().contains("critical")
    {
        severity = Severity::Critical;
    }

    severity
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> Config {
        Config {
            newline_pattern: String::new(),
            blocking_pattern: String::new(),
            sampling: 1,
            logs_per_batch: 500,
            integration_type: IntegrationType::S3,
            app_name: None,
            sub_name: None,
            api_key: "test".to_string().into(),
            endpoint: "test".to_string(),
            max_elapsed_time: 250,
            csv_delimiter: ",".to_string(),
            batches_max_size: 4,
            batches_max_concurrency: 10,
            add_metadata: String::new(),
            dlq_arn: None,
            dlq_url: None,
            dlq_retry_limit: None,
            dlq_s3_bucket: None,
            lambda_assume_role: None,
            enable_log_group_tags: false,
        }
    }

    #[test]
    fn test_extract_json_value_simple() {
        let log = r#"{"eventSource": "aws:s3", "message": "test"}"#;
        let result = extract_json_value(log, "eventSource");
        assert_eq!(result, Some("aws:s3".to_string()));
    }

    #[test]
    fn test_extract_json_value_nested() {
        let log = r#"{"metadata": {"app": {"name": "test-app"}}}"#;
        let result = extract_json_value(log, "metadata.app.name");
        assert_eq!(result, Some("test-app".to_string()));
    }

    #[test]
    fn test_extract_json_value_non_string() {
        let log = r#"{"count": 42, "active": true}"#;
        assert_eq!(extract_json_value(log, "count"), Some("42".to_string()));
        assert_eq!(extract_json_value(log, "active"), Some("true".to_string()));
    }

    #[test]
    fn test_extract_json_value_missing() {
        let log = r#"{"eventSource": "aws:s3"}"#;
        let result = extract_json_value(log, "nonexistent");
        assert_eq!(result, None);
    }

    #[test]
    fn test_extract_json_value_invalid_json() {
        let log = "not a json";
        let result = extract_json_value(log, "any.path");
        assert_eq!(result, None);
    }

    #[test]
    fn test_extract_json_value_template_syntax() {
        let log = r#"{"metadata": {"app": {"name": "test-app"}}}"#;

        // Test different spacing in template
        assert_eq!(
            extract_json_value(log, "{{$.metadata.app.name}}"),
            Some("test-app".to_string())
        );
        assert_eq!(
            extract_json_value(log, "{{ $.metadata.app.name }}"),
            Some("test-app".to_string())
        );
        assert_eq!(
            extract_json_value(log, "{{    $.metadata.app.name    }}"),
            Some("test-app".to_string())
        );

        // Test direct path still works
        assert_eq!(
            extract_json_value(log, "metadata.app.name"),
            Some("test-app".to_string())
        );
    }

    #[test]
    fn test_convert_to_log_entry_with_json_extraction() {
        let log = r#"{"app": "json-app", "subsystem": "json-subsystem"}"#;
        let app_name = "{{ $.app }}";
        let sub_name = "{{ $.subsystem }}";
        let mctx = process::MetadataContext::default();
        let config = test_config();

        let entry = convert_to_log_entry(log.to_string(), app_name, sub_name, &mctx, &config);

        assert_eq!(entry.application_name, "json-app");
        assert_eq!(entry.subsystem_name, "json-subsystem");
    }

    #[test]
    fn test_convert_to_log_entry_json_fallback() {
        let log = r#"{"different_key": "value"}"#;
        let app_name = "{{ $.nonexistent }}";
        let sub_name = "{{ $.missing }}";
        let mctx = process::MetadataContext::default();
        let config = test_config();

        let entry = convert_to_log_entry(log.to_string(), app_name, sub_name, &mctx, &config);

        // With improved fallback, when JSON extraction fails and no metadata exists,
        // it should fall back to sensible defaults instead of returning template strings
        assert_eq!(entry.application_name, "unknown-application");
        assert_eq!(entry.subsystem_name, "unknown-subsystem");
    }

    #[test]
    fn test_convert_to_log_entry_nested_json() {
        let log = r#"{
            "metadata": {
                "application": {
                    "name": "nested-app",
                    "environment": "production"
                },
                "service": {
                    "type": "web",
                    "instance": "worker-1"
                }
            }
        }"#;

        let app_name = "{{ $.metadata.application.name }}";
        let sub_name = "{{ $.metadata.service.type }}";
        let mctx = process::MetadataContext::default();
        let config = test_config();

        let entry = convert_to_log_entry(log.to_string(), app_name, sub_name, &mctx, &config);

        assert_eq!(entry.application_name, "nested-app");
        assert_eq!(entry.subsystem_name, "web");
    }

    #[test]
    fn test_json_path_spacing_variations() {
        let log = r#"{
            "key": "value1",
            "nested": {
                "key": "value2"
            }
        }"#;

        // Test different spacing variations
        let variations = [
            "{{$.key}}",
            "{{ $.key }}",
            "{{$.key  }}",
            "{{  $.key}}",
            "{{    $.key    }}",
            "{{$.nested.key}}",
            "{{ $.nested.key }}",
            "{{    $.nested.key    }}",
        ];

        for template in variations {
            let entry = convert_to_log_entry(
                log.to_string(),
                template,
                "default-sub",
                &process::MetadataContext::default(),
                &test_config(),
            );

            let expected = if template.contains("nested") {
                "value2"
            } else {
                "value1"
            };

            assert_eq!(
                entry.application_name, expected,
                "Failed for template: {}",
                template
            );
        }
    }

    #[test]
    fn test_convert_to_log_entry_regex_fallback() {
        let log = r#"{"message": "test log"}"#;
        let mctx = process::MetadataContext::default();

        // Set up CloudWatch metadata
        mctx.insert(
            "cw.log.group".to_string(),
            Some("/ecs/my-service-prod_logs_abc123".to_string()),
        );
        mctx.insert("cw.log.stream".to_string(), Some("my-stream".to_string()));

        let config = test_config();

        // Test case 1: Customer's regex that should extract "my-service"
        let customer_regex =
            r#"{{ cw.log.group | r'^/ecs/([a-z0-9-]+?)(?:-[a-z]+)?_logs_[a-z0-9]+$|^(.*)$' }}"#;
        let entry = convert_to_log_entry(
            log.to_string(),
            customer_regex,
            "{{ cw.log.stream }}",
            &mctx,
            &config,
        );

        assert_eq!(entry.application_name, "my-service");
        assert_eq!(entry.subsystem_name, "my-stream");

        // Test case 2: Regex that doesn't match - should fall back to raw log group value
        let non_matching_regex = r#"{{ cw.log.group | r'^/this/will/never/match/([a-z]+)$' }}"#;
        let entry = convert_to_log_entry(
            log.to_string(),
            non_matching_regex,
            "{{ cw.log.stream }}",
            &mctx,
            &config,
        );

        assert_eq!(entry.application_name, "/ecs/my-service-prod_logs_abc123"); // Raw value fallback
        assert_eq!(entry.subsystem_name, "my-stream");

        // Test case 3: Missing metadata key - should use defaults since it's a template
        let missing_key_regex = r#"{{ nonexistent.key | r'^(.*)$' }}"#;
        let entry = convert_to_log_entry(
            log.to_string(),
            missing_key_regex,
            missing_key_regex,
            &mctx,
            &config,
        );

        // Should use defaults since templates reference non-existent metadata keys
        assert_eq!(entry.application_name, "unknown-application");
        assert_eq!(entry.subsystem_name, "unknown-subsystem");

        // Test case 4: Template references missing metadata key - should use defaults, not hierarchy
        let empty_mctx = process::MetadataContext::default();
        empty_mctx.insert("s3.bucket".to_string(), Some("test-bucket".to_string()));
        empty_mctx.insert(
            "s3.object.key".to_string(),
            Some("logs/app.log".to_string()),
        );

        // Using realistic CloudWatch template but no CloudWatch metadata available
        let cw_template =
            r#"{{ cw.log.group | r'^/ecs/([a-z0-9-]+?)(?:-[a-z]+)?_logs_[a-z0-9]+$|^(.*)$' }}"#;

        let entry = convert_to_log_entry(
            log.to_string(),
            cw_template, // App name template references cw.log.group (not available)
            "{{ cw.log.stream }}", // Subsystem template references cw.log.stream (not available)
            &empty_mctx,
            &config,
        );

        // Should use defaults since templates reference non-existent metadata keys
        assert_eq!(entry.application_name, "unknown-application");
        assert_eq!(entry.subsystem_name, "unknown-subsystem");

        // Test case 5: No metadata at all - should use default fallbacks
        let completely_empty_mctx = process::MetadataContext::default();

        let entry = convert_to_log_entry(
            log.to_string(),
            missing_key_regex,
            missing_key_regex,
            &completely_empty_mctx,
            &config,
        );

        assert_eq!(entry.application_name, "unknown-application");
        assert_eq!(entry.subsystem_name, "unknown-subsystem");
    }

    #[test]
    fn test_kinesis_no_app_name_configured() {
        let log = r#"{"message": "test log from kinesis"}"#;
        let mctx = process::MetadataContext::default();

        // Set up CloudWatch metadata that would be available from Kinesis
        mctx.insert(
            "cw.log.group".to_string(),
            Some("/aws/lambda/my-function".to_string()),
        );
        mctx.insert(
            "cw.log.stream".to_string(),
            Some("2024/01/01/my-stream".to_string()),
        );

        let config = test_config();

        // Test: When no app_name is configured, Kinesis uses "NO APPLICATION NAME"
        let entry = convert_to_log_entry(
            log.to_string(),
            "NO APPLICATION NAME", // This is what kinesis_logs passes when config.app_name is None
            "NO SUBSYSTEM NAME",   // This is what kinesis_logs passes when config.sub_name is None
            &mctx,
            &config,
        );

        // Since "NO APPLICATION NAME" is not a template, it gets processed by our improved fallback
        // The evaluate() method returns it as-is because it's not a template (no {{ }})
        // So the final result should be the literal string
        assert_eq!(entry.application_name, "NO APPLICATION NAME");
        assert_eq!(entry.subsystem_name, "NO SUBSYSTEM NAME");
    }

    #[test]
    fn test_dynamic_fallback_when_regex_fails() {
        let log = r#"{"message": "test log"}"#;
        let mctx = process::MetadataContext::default();

        // Set up metadata where the key exists but won't match the regex
        mctx.insert(
            "cw.log.group".to_string(),
            Some("/not-matching-ecs-pattern".to_string()),
        );
        mctx.insert("s3.bucket".to_string(), Some("backup-bucket".to_string()));

        let config = test_config();

        // Scenario 1: Key exists, regex fails → should return raw value of the specific key
        let failing_regex = r#"{{ cw.log.group | r'^/ecs/([a-z0-9-]+?)_logs_[a-z0-9]+$' }}"#;

        let entry = convert_to_log_entry(
            log.to_string(),
            failing_regex,
            "{{ cw.log.group }}", // Simple template without regex
            &mctx,
            &config,
        );

        // Dynamic fallback: uses raw value of cw.log.group (the key referenced in template)
        assert_eq!(entry.application_name, "/not-matching-ecs-pattern");
        assert_eq!(entry.subsystem_name, "/not-matching-ecs-pattern");

        // Scenario 2: Key doesn't exist → should use defaults (not hierarchy)
        let missing_key_template = r#"{{ nonexistent.key | r'^(.*)$' }}"#;

        let entry2 = convert_to_log_entry(
            log.to_string(),
            missing_key_template,
            missing_key_template,
            &mctx,
            &config,
        );

        // Templates with missing keys use defaults, not hierarchy
        assert_eq!(entry2.application_name, "unknown-application");
        assert_eq!(entry2.subsystem_name, "unknown-subsystem");

        // Scenario 3: Non-template values → should return as-is when not JSON paths
        let entry3 = convert_to_log_entry(
            log.to_string(),
            "some-plain-app-name", // Not a template, not a JSON path
            "some-plain-sub-name", // Not a template, not a JSON path
            &mctx,
            &config,
        );

        // Non-template values should return the original input string
        assert_eq!(entry3.application_name, "some-plain-app-name");
        assert_eq!(entry3.subsystem_name, "some-plain-sub-name");
    }

    #[test]
    fn test_all_metadata_keys_dynamic_fallback() {
        let log = r#"{"message": "test log"}"#;
        let config = test_config();

        // All metadata keys from the documentation
        let all_metadata_keys = vec![
            // S3 metadata
            "s3.bucket",
            "s3.object.key",
            // CloudWatch metadata
            "cw.log.group",
            "cw.log.stream",
            "cw.owner",
            // Kafka metadata
            "kafka.topic",
            // Kinesis metadata
            "kinesis.event.id",
            "kinesis.event.name",
            "kinesis.event.source",
            "kinesis.event.source_arn",
            // SQS metadata
            "sqs.event.source",
            "sqs.event.id",
            // ECR metadata
            "ecr.scan.id",
            "ecr.scan.source",
        ];

        for key in all_metadata_keys {
            // Test 1: When the referenced metadata key EXISTS
            let mctx_with_key = process::MetadataContext::default();
            mctx_with_key.insert(key.to_string(), Some(format!("test-value-for-{}", key)));

            let template = format!("{{{{ {} | r'^(.*)$' }}}}", key);

            let entry = convert_to_log_entry(
                log.to_string(),
                &template,
                &template,
                &mctx_with_key,
                &config,
            );

            // Should use the specific metadata key value
            assert_eq!(
                entry.application_name,
                format!("test-value-for-{}", key),
                "Failed for metadata key '{}' when key exists",
                key
            );
            assert_eq!(
                entry.subsystem_name,
                format!("test-value-for-{}", key),
                "Failed for metadata key '{}' when key exists",
                key
            );

            // Test 2: When the referenced metadata key DOESN'T EXIST
            let mctx_without_key = process::MetadataContext::default();
            // Add some other random metadata to ensure it doesn't fall back to unrelated keys
            mctx_without_key.insert(
                "some.other.key".to_string(),
                Some("should-not-be-used".to_string()),
            );

            let entry2 = convert_to_log_entry(
                log.to_string(),
                &template,
                &template,
                &mctx_without_key,
                &config,
            );

            // Should use defaults, NOT any other available metadata
            assert_eq!(
                entry2.application_name, 
                "unknown-application",
                "Failed for metadata key '{}' when key missing - should use defaults, not other metadata", 
                key
            );
            assert_eq!(
                entry2.subsystem_name, 
                "unknown-subsystem", 
                "Failed for metadata key '{}' when key missing - should use defaults, not other metadata", 
                key
            );
        }
    }
}
