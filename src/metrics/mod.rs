use crate::events;
use crate::metrics::config::Config;
use aws_lambda_events::firehose::KinesisFirehoseResponse;
use lambda_runtime::{Error, LambdaEvent};
use tracing::{debug, info};

pub mod config;
pub mod process;

// metric telemetry handler
pub async fn handler(
    config: &Config,
    event: LambdaEvent<events::Combined>,
) -> Result<KinesisFirehoseResponse, Error> {
    if let events::Combined::Firehose(firehose_event) = event.payload {
        let firehose_response = process::kinesis_firehose(config, firehose_event).await?;
        debug!("Firehose response: {:?}", firehose_response);

        // Check if any records failed processing
        let any_failed = firehose_response
            .records
            .iter()
            .any(|record| record.result != Some("Ok".to_string()));

        if any_failed {
            // Return an error to indicate that the batch failed
            Err(Error::from("One or more records failed processing"))
        } else {
            // All records succeeded, return the response
            info!("All records processed successfully");
            Ok(firehose_response)
        }
    } else {
        Err(Error::from("Unknown event type"))
    }
}
