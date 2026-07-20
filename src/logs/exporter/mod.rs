use std::sync::Arc;

use async_trait::async_trait;
use cx_sdk_otlp::auth::AuthData;
use thiserror::Error;

use super::config::{Config, LogExportConfig};
use super::model::ProcessedLog;
use otlp::OtlpGrpcExporter;
use rest::CoralogixRestExporter;

pub mod otlp;
pub mod rest;

#[derive(Debug, Error)]
pub enum LogExportError {
    #[error("REST exporter initialization failed: {0}")]
    RestInitialization(#[source] Box<dyn std::error::Error + Send + Sync + 'static>),
    #[error("REST log export failed: {0}")]
    Rest(#[from] cx_sdk_rest_logs::Error),
    #[error("OTLP initialization failed: {0}")]
    OtlpInitialization(String),
    #[error("OTLP log export failed: {0}")]
    OtlpResponse(String),
    #[error("one encoded OTLP log record exceeds the configured request limit")]
    OversizedRecord,
}

#[async_trait]
pub trait LogExporter: Send + Sync {
    async fn export(&self, logs: Vec<ProcessedLog>) -> Result<(), LogExportError>;
}

pub type DynLogExporter = Arc<dyn LogExporter>;

pub fn build_exporter(config: &Config) -> Result<DynLogExporter, LogExportError> {
    build_exporter_from(
        &config.export,
        config.max_elapsed_time,
        config.batches_max_size * 1024 * 1024,
    )
}

fn build_exporter_from(
    export: &LogExportConfig,
    max_elapsed_time: u64,
    max_request_bytes: usize,
) -> Result<DynLogExporter, LogExportError> {
    match export {
        LogExportConfig::CoralogixRest { endpoint, api_key } => Ok(Arc::new(
            CoralogixRestExporter::new(endpoint.clone(), api_key.clone(), max_elapsed_time)?,
        )),
        LogExportConfig::CollectorOtlpGrpc { endpoint } => Ok(Arc::new(OtlpGrpcExporter::new(
            endpoint.clone(),
            AuthData::default(),
            max_elapsed_time,
            max_request_bytes,
        )?)),
        LogExportConfig::CoralogixOtlpGrpc { endpoint, api_key } => {
            Ok(Arc::new(OtlpGrpcExporter::new(
                endpoint.clone(),
                AuthData::from(api_key),
                max_elapsed_time,
                max_request_bytes,
            )?))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logs::config::LogExportConfig;

    #[tokio::test]
    async fn builds_collector_otlp_exporter_without_api_key() {
        let exporter = build_exporter_from(
            &LogExportConfig::CollectorOtlpGrpc {
                endpoint: "http://127.0.0.1:4317".to_string(),
            },
            250,
            4 * 1024 * 1024,
        );
        assert!(exporter.is_ok());
    }

    #[tokio::test]
    async fn builds_direct_coralogix_otlp_exporter_with_api_key() {
        let exporter = build_exporter_from(
            &LogExportConfig::CoralogixOtlpGrpc {
                endpoint: "https://ingress.eu2.coralogix.com:443".to_string(),
                api_key: "secret".to_string().into(),
            },
            250,
            4 * 1024 * 1024,
        );
        assert!(exporter.is_ok());
    }
}
