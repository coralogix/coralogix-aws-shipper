use aws_lambda_events::ecr_scan::EcrScanEvent;
use aws_lambda_events::event::cloudwatch_logs::LogsEvent;
use aws_lambda_events::event::kafka::KafkaEvent;
use aws_lambda_events::event::kinesis::KinesisEvent;
use aws_lambda_events::event::s3::S3Event;
use aws_lambda_events::event::sns::SnsEvent;
use aws_lambda_events::event::sqs::SqsEvent;
use aws_lambda_events::event::firehose::KinesisFirehoseEvent;

use serde::de::{self, Deserialize, Deserializer};
use serde_json::Value;
use tracing::debug;

#[derive(Debug)]
pub enum Combined {
    S3(S3Event),
    Sns(SnsEvent),
    CloudWatchLogs(LogsEvent),
    Sqs(SqsEvent),
    Kinesis(KinesisEvent),
    Kafka(KafkaEvent),
    EcrScan(EcrScanEvent),
    Firehose(KinesisFirehoseEvent),
}

impl<'de> Deserialize<'de> for Combined {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw_value: Value = Deserialize::deserialize(deserializer)?;
        debug!("raw_value: {:?}", raw_value);
        if let Ok(event) = S3Event::deserialize(&raw_value) {
            tracing::info!("s3 event detected");
            return Ok(Combined::S3(event));
        }

        if let Ok(event) = SnsEvent::deserialize(&raw_value) {
            tracing::info!("sns event detected");
            return Ok(Combined::Sns(event));
        }
        if let Ok(event) = EcrScanEvent::deserialize(&raw_value) {
            tracing::info!("ecr scan event detected");
            return Ok(Combined::EcrScan(event));
        }

        if let Ok(event) = LogsEvent::deserialize(&raw_value) {
            tracing::info!("cloudwatch event detected");
            return Ok(Combined::CloudWatchLogs(event));
        }

        if let Ok(event) = KinesisEvent::deserialize(&raw_value) {
            tracing::info!("kinesis event detected");
            return Ok(Combined::Kinesis(event));
        }

        if let Ok(event) = SqsEvent::deserialize(&raw_value) {
            tracing::info!("sqs event detected");
            return Ok(Combined::Sqs(event));
        }

        if let Ok(event) = KinesisFirehoseEvent::deserialize(&raw_value) {
            tracing::info!("firehose event detected");
            return Ok(Combined::Firehose(event));
        }

        // IMPORTANT: kafka must be evaluated last as it uses an arbitrary map to evaluate records.
        // Since all other fields are optional, this map could potentially match any arbitrary JSON
        // and result in empty values.
        if let Ok(event) = KafkaEvent::deserialize(&raw_value) {
            tracing::info!("kafka event detected");

            // kafka events triggering a lambda function should always have at least one record
            // if not, it is likely an unsupport or bad event
            if event.records.is_empty() {
                return Err(de::Error::custom(format!(
                    "unsupported or bad event type: {raw_value}"
                )));
            }
            return Ok(Combined::Kafka(event));
        }

        Err(de::Error::custom(format!(
            "unsupported event type: {raw_value}"
        )))
    }
}
