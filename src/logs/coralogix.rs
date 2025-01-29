use crate::logs::config::Config;
use crate::logs::*;
use cx_sdk_rest_logs::auth::AuthData;
use cx_sdk_rest_logs::model::{LogSinglesEntry, LogSinglesRequest, Severity};
use cx_sdk_rest_logs::DynLogExporter;
use futures::stream::{StreamExt, TryStreamExt};
use itertools::Itertools;
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
          
            // to be deprecated
            || self.stream_name.is_some()
            || self.loggroup_name.is_some()
            || self.bucket_name.is_some()
            || self.key_name.is_some()
            || self.topic_name.is_some()
        
            || !self.custom_metadata.is_empty()
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

    let application_name = mctx
        .evaluate(configured_app_name.to_string())
        .unwrap_or_else(|e| {
            tracing::warn!("application name dynamic parsing failed, using: {}", e);
            configured_app_name.to_owned()
        });

    tracing::debug!("App Name: {}", &application_name);
    let subsystem_name = mctx
        .evaluate(configured_sub_name.to_string())
        .unwrap_or_else(|e| {
            tracing::warn!("subsystem name dynamic parsing failed, using: {}", e);
            configured_sub_name.to_owned()
        });

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
    let body = if message.has_metadata() {
        serde_json::to_value(&message).unwrap_or(message.message)
    } else {
        message.message
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

// #[cfg(test)]
// mod test {

//     // use crate::logs::coralogix::dynamic_metadata_for_log;

//     #[test]
//     fn test_nondynamic_app_name() {
//         let key_name = "AwsLogs/folder1/folder2";
//         let app_name = "my-app";
//         let log_file_contents = r#"{
//             "timestamp": "09-24 16:09:07.042",
//             "message": "java.lang.NullPointerException",
//         }"#;
//         let dapp = dynamic_metadata_for_log(app_name, log_file_contents, key_name.to_string());
//         assert_eq!(dapp, "my-app");
//     }

//     #[test]
//     fn test_fall_back_to_app_name_when_dynamic_app_name_is_missing() {
//         let key_name = "AwsLogs/folder1/folder2";
//         let app_name = "$.app_name";
//         let log_file_contents = r#"{
//             "timestamp": "09-24 16:09:07.042",
//             "message": "java.lang.NullPointerException",
//         }"#;
//         let dapp = dynamic_metadata_for_log(app_name, log_file_contents, key_name.to_string());
//         assert_eq!(dapp, "$.app_name");
//     }

//     #[test]
//     fn test_simple_dynamic_app_name() {
//         let key_name = "AwsLogs/folder1/folder2";
//         let app_name = "$.app_name";
//         let log_file_contents = r#"{
//             "timestamp": "09-24 16:09:07.042",
//             "message": "java.lang.NullPointerException",
//             "app_name": "my-awesome-app"
//         }"#;
//         let dapp = dynamic_metadata_for_log(app_name, log_file_contents, key_name.to_string());
//         assert_eq!(dapp, "my-awesome-app");
//     }

//     #[test]
//     fn test_nested_dynamic_app_name() {
//         let key_name = "AwsLogs/folder1/folder2";
//         let app_name = "$.metadata.app_name";
//         let log_file_contents = r#"{
//             "timestamp": "09-24 16:09:07.042",
//             "message": "java.lang.NullPointerException",
//             "metadata": {
//                 "app_name": "my-awesome-app2"
//             }
//         }"#;
//         let dapp = dynamic_metadata_for_log(app_name, log_file_contents, key_name.to_string());
//         assert_eq!(dapp, "my-awesome-app2");
//     }
//     #[test]
//     fn test_dynamic_folder_app_name() {
//         let key_name = "AwsLogs/folder1/folder2";
//         let app_name = "{{s3_key.2}}";
//         let log_file_contents = r#"{
//             "timestamp": "09-24 16:09:07.042",
//             "message": "java.lang.NullPointerException",
//             "metadata": {
//                 "app_name": "my-awesome-app2"
//             }
//         }"#;
//         let dapp = dynamic_metadata_for_log(app_name, log_file_contents, key_name.to_string());
//         assert_eq!(dapp, "folder1");
//     }
//     #[test]
//     fn test_dynamic_folder_app_name_fails() {
//         let key_name = "AwsLogs/folder1/folder2";
//         let app_name = "{{s3_key.8}}";
//         let log_file_contents = r#"{
//             "timestamp": "09-24 16:09:07.042",
//             "message": "java.lang.NullPointerException",
//             "metadata": {
//                 "app_name": "my-awesome-app2"
//             }
//         }"#;
//         let dapp = dynamic_metadata_for_log(app_name, log_file_contents, key_name.to_string());
//         assert_eq!(dapp, "default");
//     }
// }
