use aws_lambda_events::event::cloudwatch_logs::LogsEvent;
use aws_lambda_events::event::s3::S3Event;
use aws_lambda_events::event::sns::SnsEvent;
use aws_lambda_events::event::sqs::SqsEvent;
use aws_lambda_events::event::kinesis::KinesisEvent;
use aws_lambda_events::event::kafka::KafkaEvent;
use aws_lambda_events::ecr_scan::EcrScanEvent;
use serde::de::{self, Deserialize, Deserializer};
use serde_json::Value;
use tracing::debug;

#[derive(Debug)]
pub enum CombinedEvent {
    S3(S3Event),
    Sns(SnsEvent),
    CloudWatchLogs(LogsEvent),
    Sqs(SqsEvent),
    Kinesis(KinesisEvent),
    Kafka(KafkaEvent),
    EcrScan(EcrScanEvent),
}

impl<'de> Deserialize<'de> for CombinedEvent {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw_value: Value = Deserialize::deserialize(deserializer)?;
        debug!("raw_value: {:?}", raw_value);
        if let Ok(event) = S3Event::deserialize(&raw_value) {
            tracing::debug!("s3 event detected");
            return Ok(CombinedEvent::S3(event));
        }

        if let Ok(event) = SnsEvent::deserialize(&raw_value) {
            tracing::debug!("sns event detected");
            return Ok(CombinedEvent::Sns(event));
        }
        if let Ok(event) = EcrScanEvent::deserialize(&raw_value) {
            tracing::debug!("ecr scan event detected");
            return Ok(CombinedEvent::EcrScan(event));
        }
        
        if let Ok(event) = LogsEvent::deserialize(&raw_value) {
            tracing::debug!("cloudwatch event detected");
            return Ok(CombinedEvent::CloudWatchLogs(event));
        }

        if let Ok(event) = KafkaEvent::deserialize(&raw_value) {
            tracing::debug!("kafka event detected");
            return Ok(CombinedEvent::Kafka(event));
        }

        if let Ok(event) = SqsEvent::deserialize(&raw_value) {
            tracing::debug!("sqs event detected");
            return Ok(CombinedEvent::Sqs(event));
        }
        

        Err(de::Error::custom(format!(
            "unsupported event type: {raw_value}"
        )))
    }
}