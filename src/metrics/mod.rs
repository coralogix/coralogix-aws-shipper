use crate::events;
use crate::metrics::config::Config;
use aws_lambda_events::firehose::KinesisFirehoseResponse;
use lambda_runtime::{Error, LambdaEvent};
use tracing::error;

pub mod config;
pub mod process;

// metric telemetry handler
pub async fn handler(
    config: &Config,
    event: LambdaEvent<events::Combined>,
) -> Result<KinesisFirehoseResponse, Error> {
    match event.payload {
        events::Combined::Firehose(firehose_event) => process::transform_firehose_event(config, firehose_event).await,
        _ => {
            error!("incompatible event type for metrics telemetry mode");
            Err("incompatible event type for metrics telemetry mode".to_string().into())
        }
    }
}
