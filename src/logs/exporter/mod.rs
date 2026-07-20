use std::sync::Arc;

use async_trait::async_trait;
use thiserror::Error;

use super::model::ProcessedLog;

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
