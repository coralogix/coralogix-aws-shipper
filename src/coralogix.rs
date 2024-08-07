use crate::config::Config;
use crate::process::Metadata;
use crate::*;
use cx_sdk_rest_logs::auth::AuthData;
use cx_sdk_rest_logs::model::{LogSinglesEntry, LogSinglesRequest, Severity};
use cx_sdk_rest_logs::DynLogExporter;
use futures::stream::{StreamExt, TryStreamExt};
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::iter::IntoIterator;
use std::time::Instant;
use std::vec::Vec;
use time::OffsetDateTime;
use tracing::{error, info};
use std::env;

pub async fn process_batches(
    logs: Vec<String>,
    configured_app_name: &str,
    configured_sub_name: &str,
    config: &Config,
    metadata_instance: &process::Metadata,
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
                        metadata_instance,
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

fn convert_to_log_entry(
    log: String,
    configured_app_name: &str,
    configured_sub_name: &str,
    metadata_instance: &Metadata,
    config: &Config,
) -> LogSinglesEntry<Value> {
    let now = OffsetDateTime::now_utc();
    let application_name = dynamic_metadata_for_log(
        configured_app_name,
        &log,
        metadata_instance.key_name.clone(),
    );
    tracing::debug!("App Name: {}", &application_name);
    let subsystem_name = dynamic_metadata_for_log(
        configured_sub_name,
        &log,
        metadata_instance.key_name.clone(),
    );
    tracing::debug!("Sub Name: {}", &subsystem_name);
    let severity = get_severity_level(&log);
    let stream_name = metadata_instance.stream_name.clone();
    let topic_name = metadata_instance.topic_name.clone();
    // let loggroup_name = metadata_instance.log_group.clone();
    tracing::debug!("Severity: {:?}", severity);

    let message = match serde_json::from_str(&log) {
        Ok(value) => value,
        Err(_) => Value::String(log),
    };

    let mut message = JsonMessage {
        message,
        stream_name: None,
        loggroup_name: None,
        bucket_name: None,
        key_name: None,
        topic_name: None,
        custom_metadata: HashMap::new(),
    };

    let add_metadata: Vec<&str> = config.add_metadata.split(',').map(|s| s.trim()).collect();

    tracing::debug!("add_metadata: {:?}", add_metadata);

    for metadata_field in &add_metadata {
        tracing::debug!("Processing metadata field: {}", metadata_field);
        match *metadata_field {
            "stream_name" => {
                message.stream_name = Some(metadata_instance.stream_name.clone());
                tracing::debug!("Assigned stream_name: {}", metadata_instance.stream_name);
            }
            "loggroup_name" => {
                message.loggroup_name = Some(metadata_instance.log_group.clone());
                tracing::debug!("Assigned loggroup_name: {}", metadata_instance.log_group);
            }
            "bucket_name" => {
                message.bucket_name = Some(metadata_instance.bucket_name.clone());
                tracing::debug!("Assigned bucket_name: {}", metadata_instance.bucket_name);
            }
            "key_name" => {
                message.key_name = Some(metadata_instance.key_name.clone());
                tracing::debug!("Assigned key_name: {}", metadata_instance.key_name);
            }
            "topic_name" => {
                message.topic_name = Some(metadata_instance.topic_name.clone());
                tracing::debug!("Assigned topic_name: {}", metadata_instance.topic_name);
            }
            _ => {
                tracing::debug!(
                    "No matching metadata field or condition not met for: {}",
                    metadata_field
                );
            }
        }
    }
    if let Ok(custom_metadata_str) = env::var("CUSTOM_METADATA") {
        debug!("Custom metadata STR: {}", custom_metadata_str);
        let mut metadata = HashMap::new();
        let pairs = custom_metadata_str.split(',');
    
        for pair in pairs {
            let split_pair: Vec<&str> = pair.split('=').collect();
            match split_pair.as_slice() {
                [key, value] => {
                    metadata.insert(key.to_string(), value.to_string());
                },
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
    let body =  if message.stream_name.is_some() || message.loggroup_name.is_some() || message.bucket_name.is_some() || message.key_name.is_some() || message.topic_name.is_some()|| !message.custom_metadata.is_empty() {
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
        thread_id: Some(stream_name),
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

fn dynamic_metadata_for_log(app_name: &str, log: &str, key_name: String) -> String {
    dynamic_metadata(app_name, log, key_name).unwrap_or_else(|| app_name.to_owned())
}

fn dynamic_metadata(app_name: &str, log: &str, key_name: String) -> Option<String> {
    if app_name.starts_with("$.") {
        let mut json_data = serde_json::from_str::<serde_json::Value>(log).ok()?;
        for segment in app_name[2..].split('.') {
            json_data = json_data
                .as_object_mut()
                .and_then(|obj| obj.remove(segment))?;
        }
        json_data.as_str().map(|str| str.to_owned())
    } else if app_name.starts_with("{{s3_key") {
        // Check if there is an index provided
        let maybe_index_str = app_name
            .trim_start_matches("{{s3_key")
            .trim_end_matches('}');
        debug!("Getting s3 Key from {}", app_name);
        if let Some('.') = maybe_index_str.chars().next() {
            // Parse the index
            let index_str = &maybe_index_str[1..]; // Remove the leading '.'
            if let Ok(index) = index_str.parse::<usize>() {
                debug!("Getting index {} from {}", index_str, key_name);
                let segments: Vec<&str> = key_name.split('/').collect();
                debug!("Segments: {:?}", segments);
                if index > 0 && index <= segments.len() {
                    // Return the directory at the specified index (1-based)
                    debug!("Returning {}", segments[index - 1]);
                    return Some(segments[index - 1].to_string());
                }
            }
        }
        tracing::warn!("Application or Subsystem Name Parsing Failed {}", app_name);
        Some("default".to_string())
    } else {
        Some(app_name.to_string())
    }
}

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
mod test {

    use crate::coralogix::dynamic_metadata_for_log;

    #[test]
    fn test_nondynamic_app_name() {
        let key_name = "AwsLogs/folder1/folder2";
        let app_name = "my-app";
        let log_file_contents = r#"{
            "timestamp": "09-24 16:09:07.042",
            "message": "java.lang.NullPointerException",
        }"#;
        let dapp = dynamic_metadata_for_log(app_name, log_file_contents, key_name.to_string());
        assert_eq!(dapp, "my-app");
    }

    #[test]
    fn test_fall_back_to_app_name_when_dynamic_app_name_is_missing() {
        let key_name = "AwsLogs/folder1/folder2";
        let app_name = "$.app_name";
        let log_file_contents = r#"{
            "timestamp": "09-24 16:09:07.042",
            "message": "java.lang.NullPointerException",
        }"#;
        let dapp = dynamic_metadata_for_log(app_name, log_file_contents, key_name.to_string());
        assert_eq!(dapp, "$.app_name");
    }

    #[test]
    fn test_simple_dynamic_app_name() {
        let key_name = "AwsLogs/folder1/folder2";
        let app_name = "$.app_name";
        let log_file_contents = r#"{
            "timestamp": "09-24 16:09:07.042",
            "message": "java.lang.NullPointerException",
            "app_name": "my-awesome-app"
        }"#;
        let dapp = dynamic_metadata_for_log(app_name, log_file_contents, key_name.to_string());
        assert_eq!(dapp, "my-awesome-app");
    }

    #[test]
    fn test_nested_dynamic_app_name() {
        let key_name = "AwsLogs/folder1/folder2";
        let app_name = "$.metadata.app_name";
        let log_file_contents = r#"{
            "timestamp": "09-24 16:09:07.042",
            "message": "java.lang.NullPointerException",
            "metadata": {
                "app_name": "my-awesome-app2"
            }
        }"#;
        let dapp = dynamic_metadata_for_log(app_name, log_file_contents, key_name.to_string());
        assert_eq!(dapp, "my-awesome-app2");
    }
    #[test]
    fn test_dynamic_folder_app_name() {
        let key_name = "AwsLogs/folder1/folder2";
        let app_name = "{{s3_key.2}}";
        let log_file_contents = r#"{
            "timestamp": "09-24 16:09:07.042",
            "message": "java.lang.NullPointerException",
            "metadata": {
                "app_name": "my-awesome-app2"
            }
        }"#;
        let dapp = dynamic_metadata_for_log(app_name, log_file_contents, key_name.to_string());
        assert_eq!(dapp, "folder1");
    }
    #[test]
    fn test_dynamic_folder_app_name_fails() {
        let key_name = "AwsLogs/folder1/folder2";
        let app_name = "{{s3_key.8}}";
        let log_file_contents = r#"{
            "timestamp": "09-24 16:09:07.042",
            "message": "java.lang.NullPointerException",
            "metadata": {
                "app_name": "my-awesome-app2"
            }
        }"#;
        let dapp = dynamic_metadata_for_log(app_name, log_file_contents, key_name.to_string());
        assert_eq!(dapp, "default");
    }
}
