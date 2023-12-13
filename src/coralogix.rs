use crate::config::Config;
use crate::process::Metadata;
use crate::*;
use cx_sdk_rest_logs::auth::AuthData;
use cx_sdk_rest_logs::model::{LogSinglesEntry, LogSinglesRequest, Severity};
use cx_sdk_rest_logs::DynLogExporter;
use futures::stream::{StreamExt, TryStreamExt};
use itertools::Itertools;
use std::iter::IntoIterator;
use std::time::Instant;
use std::vec::Vec;
use time::OffsetDateTime;
use tracing::{error, info};

pub async fn process_batches(
    logs: Vec<String>,
    configured_app_name: &str,
    configured_sub_name: &str,
    config: &Config,
    metadata_instance: &process::Metadata,
    exporter: DynLogExporter,
) -> Result<(), Error> {
    let logs: Vec<String> = logs.into_iter().filter(|log| !log.trim().is_empty()).collect();
    let number_of_logs = logs.len();
    if number_of_logs == 0 {
        info!("No logs to send");
        return Ok(());
    }

    let batches = into_batches_of_estimated_size(logs);

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

fn into_batches_of_estimated_size(logs: Vec<String>) -> Vec<Vec<String>> {
    // The hard limit is 10MB. We're aiming for 2MB, so we don't need to be precise with the size estimation.
    let target_batch_size = 2 * 1024 * 1024; // 2MB
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

fn convert_to_log_entry(
    log: String,
    configured_app_name: &str,
    configured_sub_name: &str,
    metadata_instance: &Metadata,
) -> LogSinglesEntry<String> {
    let now = OffsetDateTime::now_utc();
    let application_name = dynamic_metadata_for_log(configured_app_name, &log);
    tracing::debug!("App Name: {}", &application_name);
    let subsystem_name = dynamic_metadata_for_log(configured_sub_name, &log);
    tracing::debug!("Sub Name: {}", &subsystem_name);
    let severity = get_severity_level(&log);
    let stream_name = metadata_instance.stream_name.clone();
    tracing::debug!("Severity: {:?}", severity);
    LogSinglesEntry {
        application_name,
        subsystem_name,
        computer_name: None,
        severity,
        body: log,
        timestamp: now,
        class_name: None,
        method_name: None,
        thread_id: Some(stream_name),
        category: None,
    }
}

async fn send_logs(
    exporter: DynLogExporter,
    resource_logs: Vec<LogSinglesEntry<String>>,
    auth_data: &AuthData,
) -> Result<(), Error> {
    let number_of_logs = resource_logs.len();
    let start_time = Instant::now();
    let request = LogSinglesRequest {
        entries: resource_logs,
    };
    exporter
        .as_ref()
        .export_singles_strings(request, auth_data)
        .await?;
    tracing::info!(
        "Delivered {} log records to Coralogix in {}ms.",
        number_of_logs,
        start_time.elapsed().as_millis()
    );
    Ok(())
}

fn dynamic_metadata_for_log(app_name: &str, log: &str) -> String {
    dynamic_metadata(app_name, log).unwrap_or_else(|| app_name.to_owned())
}

fn dynamic_metadata(app_name: &str, log: &str) -> Option<String> {
    if app_name.starts_with("$.") {
        let mut json_data = serde_json::from_str::<serde_json::Value>(log).ok()?;
        for segment in app_name[2..].split('.') {
            json_data = json_data
                .as_object_mut()
                .and_then(|obj| obj.remove(segment))?;
        }
        json_data.as_str().map(|str| str.to_owned())
    } else {
        //tracing::warn!("Application or Subsystem Name Parsing Failed {}", app_name);
        None
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
        let app_name = "my-app";
        let log_file_contents = r#"{
            "timestamp": "09-24 16:09:07.042",
            "message": "java.lang.NullPointerException",
        }"#;
        let dapp = dynamic_metadata_for_log(app_name, log_file_contents);
        assert_eq!(dapp, "my-app");
    }

    #[test]
    fn test_fall_back_to_app_name_when_dynamic_app_name_is_missing() {
        let app_name = "$.app_name";
        let log_file_contents = r#"{
            "timestamp": "09-24 16:09:07.042",
            "message": "java.lang.NullPointerException",
        }"#;
        let dapp = dynamic_metadata_for_log(app_name, log_file_contents);
        assert_eq!(dapp, "$.app_name");
    }

    #[test]
    fn test_simple_dynamic_app_name() {
        let app_name = "$.app_name";
        let log_file_contents = r#"{
            "timestamp": "09-24 16:09:07.042",
            "message": "java.lang.NullPointerException",
            "app_name": "my-awesome-app"
        }"#;
        let dapp = dynamic_metadata_for_log(app_name, log_file_contents);
        assert_eq!(dapp, "my-awesome-app");
    }

    #[test]
    fn test_nested_dynamic_app_name() {
        let app_name = "$.metadata.app_name";
        let log_file_contents = r#"{
            "timestamp": "09-24 16:09:07.042",
            "message": "java.lang.NullPointerException",
            "metadata": {
                "app_name": "my-awesome-app2"
            }
        }"#;
        let dapp = dynamic_metadata_for_log(app_name, log_file_contents);
        assert_eq!(dapp, "my-awesome-app2");
    }
}
