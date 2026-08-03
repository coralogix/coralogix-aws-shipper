use std::sync::Arc;

use async_trait::async_trait;
use cx_sdk_otlp::auth::AuthData;
use thiserror::Error;

use super::config::{Config, LogExportConfig};
use super::model::ProcessedLog;
use otlp::{OtlpCompression, OtlpGrpcExporter};
use rest::CoralogixRestExporter;

pub mod otlp;
pub mod rest;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OtlpFailureClassification {
    Server,
    Client,
    Blocked,
    Unknown,
    Unclassified,
}

impl std::fmt::Display for OtlpFailureClassification {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Server => "server_error",
            Self::Client => "client_error",
            Self::Blocked => "blocked",
            Self::Unknown => "unknown_error",
            Self::Unclassified => "unclassified_error",
        })
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct OtlpResponseError {
    classification: OtlpFailureClassification,
    grpc_status: &'static str,
}

impl OtlpResponseError {
    pub(crate) fn new(
        classification: OtlpFailureClassification,
        grpc_status: &'static str,
    ) -> Self {
        Self {
            classification,
            grpc_status,
        }
    }

    pub(crate) fn classification(&self) -> OtlpFailureClassification {
        self.classification
    }

    pub(crate) fn grpc_status(&self) -> &'static str {
        self.grpc_status
    }
}

impl std::fmt::Debug for OtlpResponseError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OtlpResponseError")
            .field("classification", &self.classification.to_string())
            .field("grpc_status", &self.grpc_status)
            .finish()
    }
}

impl std::fmt::Display for OtlpResponseError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "OTLP log export failed (classification={}, grpc_status={})",
            self.classification, self.grpc_status
        )
    }
}

impl std::error::Error for OtlpResponseError {}

#[derive(Debug, Error)]
pub enum LogExportError {
    #[error("REST exporter initialization failed: {0}")]
    RestInitialization(#[source] Box<dyn std::error::Error + Send + Sync + 'static>),
    #[error("REST log export failed: {0}")]
    Rest(#[from] cx_sdk_rest_logs::Error),
    #[error("OTLP initialization failed: {0}")]
    OtlpInitialization(String),
    #[error("{0}")]
    OtlpResponse(OtlpResponseError),
    #[error("OTLP log export failed: server rejected {rejected_log_records} log record(s)")]
    PartialRejection { rejected_log_records: i64 },
    #[error("one encoded OTLP log record exceeds the configured request limit")]
    OversizedRecord,
}

#[async_trait]
pub trait LogExporter: Send + Sync {
    async fn export(&self, logs: Vec<ProcessedLog>) -> Result<(), LogExportError>;
}

pub type DynLogExporter = Arc<dyn LogExporter>;

struct StartupDestination {
    protocol: &'static str,
    destination_type: &'static str,
    endpoint_authority: String,
}

fn startup_destination(export: &LogExportConfig) -> StartupDestination {
    let (protocol, destination_type, endpoint) = match export {
        LogExportConfig::CoralogixRest { endpoint, .. } => {
            ("coralogix_rest", "coralogix", endpoint)
        }
        LogExportConfig::CollectorOtlpGrpc { endpoint } => ("otlp_grpc", "collector", endpoint),
        LogExportConfig::CoralogixOtlpGrpc { endpoint, .. } => ("otlp_grpc", "coralogix", endpoint),
    };
    let endpoint_authority = endpoint
        .parse::<http::Uri>()
        .ok()
        .and_then(|uri| {
            uri.authority()
                .map(|authority| authority.as_str().to_string())
        })
        .map(|authority| {
            authority
                .rsplit_once('@')
                .map_or(authority.clone(), |(_, sanitized)| sanitized.to_string())
        })
        .unwrap_or_else(|| "<unavailable>".to_string());

    StartupDestination {
        protocol,
        destination_type,
        endpoint_authority,
    }
}

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
    let destination = startup_destination(export);
    let exporter: DynLogExporter = match export {
        LogExportConfig::CoralogixRest { endpoint, api_key } => Arc::new(
            CoralogixRestExporter::new(endpoint.clone(), api_key.clone(), max_elapsed_time)?,
        ),
        LogExportConfig::CollectorOtlpGrpc { endpoint } => {
            Arc::new(OtlpGrpcExporter::new_with_compression(
                endpoint.clone(),
                AuthData::default(),
                max_elapsed_time,
                max_request_bytes,
                OtlpCompression::None,
            )?)
        }
        LogExportConfig::CoralogixOtlpGrpc { endpoint, api_key } => {
            Arc::new(OtlpGrpcExporter::new(
                endpoint.clone(),
                AuthData::from(api_key),
                max_elapsed_time,
                max_request_bytes,
            )?)
        }
    };

    tracing::info!(
        protocol = destination.protocol,
        destination_type = destination.destination_type,
        endpoint_authority = destination.endpoint_authority,
        "Configured log export destination"
    );

    Ok(exporter)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logs::config::LogExportConfig;

    #[test]
    fn startup_destination_reports_only_sanitized_authority() {
        let fields = startup_destination(&LogExportConfig::CoralogixRest {
            endpoint: "https://user:password@ingress.example.com:443/v1/logs?api_key=secret"
                .to_string(),
            api_key: "secret".to_string().into(),
        });

        assert_eq!(fields.protocol, "coralogix_rest");
        assert_eq!(fields.destination_type, "coralogix");
        assert_eq!(fields.endpoint_authority, "ingress.example.com:443");
    }

    #[test]
    fn startup_destination_distinguishes_otlp_routes() {
        let collector = startup_destination(&LogExportConfig::CollectorOtlpGrpc {
            endpoint: "https://collector.internal".to_string(),
        });
        assert_eq!(collector.protocol, "otlp_grpc");
        assert_eq!(collector.destination_type, "collector");
        assert_eq!(collector.endpoint_authority, "collector.internal");

        let direct = startup_destination(&LogExportConfig::CoralogixOtlpGrpc {
            endpoint: "https://ingress.eu2.coralogix.com:443".to_string(),
            api_key: "secret".to_string().into(),
        });
        assert_eq!(direct.protocol, "otlp_grpc");
        assert_eq!(direct.destination_type, "coralogix");
        assert_eq!(direct.endpoint_authority, "ingress.eu2.coralogix.com:443");
    }

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
