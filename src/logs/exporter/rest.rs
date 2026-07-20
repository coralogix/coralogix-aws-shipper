use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use cx_sdk_rest_logs::{
    auth::{ApiKey, AuthData},
    config::{BackoffConfig, LogExporterConfig},
    model::{LogSinglesEntry, LogSinglesRequest, Severity},
    LogExporterObj, RestLogExporter,
};

use super::{LogExportError, LogExporter};
use crate::logs::model::{LogSeverity, ProcessedLog};

pub struct CoralogixRestExporter {
    exporter: Arc<dyn LogExporterObj + Send + Sync>,
    auth_data: AuthData,
}

impl CoralogixRestExporter {
    pub fn new(
        endpoint: String,
        api_key: ApiKey,
        max_elapsed_time: u64,
    ) -> Result<Self, LogExportError> {
        let exporter = RestLogExporter::builder()
            .with_config(LogExporterConfig {
                url: endpoint,
                request_timeout: Duration::from_secs(30),
                backoff_config: BackoffConfig {
                    initial_delay: Duration::from_millis(10_000),
                    max_delay: Duration::from_millis(60_000),
                    max_elapsed_time: Duration::from_secs(max_elapsed_time),
                },
                user_agent: Some(
                    concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION")).to_string(),
                ),
                linger: None,
                strict_mode: None,
                processing_flow: None,
                request_body_size_limit: None,
            })
            .build()
            .map_err(|error| LogExportError::RestInitialization(Box::new(error)))?;

        Ok(Self {
            exporter: Arc::new(exporter),
            auth_data: AuthData::from(&api_key),
        })
    }
}

fn rest_severity(severity: LogSeverity) -> Severity {
    match severity {
        LogSeverity::Verbose => Severity::Verbose,
        LogSeverity::Debug => Severity::Debug,
        LogSeverity::Info => Severity::Info,
        LogSeverity::Warn => Severity::Warn,
        LogSeverity::Error => Severity::Error,
        LogSeverity::Critical => Severity::Critical,
    }
}

#[async_trait]
impl LogExporter for CoralogixRestExporter {
    async fn export(&self, logs: Vec<ProcessedLog>) -> Result<(), LogExportError> {
        let request = LogSinglesRequest {
            entries: logs
                .into_iter()
                .map(|log| LogSinglesEntry {
                    application_name: log.application_name,
                    subsystem_name: log.subsystem_name,
                    computer_name: None,
                    severity: rest_severity(log.severity),
                    body: log.body,
                    timestamp: log.timestamp,
                    class_name: None,
                    method_name: None,
                    thread_id: None,
                    category: None,
                })
                .collect(),
        };
        self.exporter
            .export_singles_jsons(request, &self.auth_data)
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::error::Error as _;

    use super::*;

    #[test]
    fn initialization_error_preserves_sdk_source() {
        let error = CoralogixRestExporter::new(
            "https://ingress-service.invalid:443".to_string(),
            ApiKey::from("test"),
            1,
        )
        .err()
        .expect("deprecated ingress-service endpoint must fail initialization");

        assert!(
            matches!(error, LogExportError::RestInitialization(_)),
            "expected REST initialization error, got {error:?}"
        );
        assert!(
            error.source().is_some(),
            "REST initialization error must retain the SDK builder error as its source"
        );
    }
}
