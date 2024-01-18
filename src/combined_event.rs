use aws_lambda_events::event::cloudwatch_logs::AwsLogs;
use aws_lambda_events::event::s3::S3Event;
use aws_lambda_events::event::sns::SnsEvent;
use aws_lambda_events::event::sqs::SqsEvent;
use aws_lambda_events::event::kinesis::KinesisEvent;
use aws_lambda_events::ecr_scan::EcrScanEvent;
use serde::de::{self, Deserialize, Deserializer};
use serde_json::Value;

pub enum CombinedEvent {
    S3(S3Event),
    Sns(SnsEvent),
    CloudWatchLogs(AwsLogs),
    Sqs(SqsEvent),
    Kinesis(KinesisEvent),
    EcrScan(EcrScanEvent),
}

impl<'de> Deserialize<'de> for CombinedEvent {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw_value: Value = Deserialize::deserialize(deserializer)?;

        // Attempt to match against known event structures event_source = "aws:sqs"
        tracing::debug!("raw_value: {:?}", raw_value);
        if let Some(records) = raw_value.get("Records") {
            if records[0].get("s3").is_some() {
                Ok(CombinedEvent::S3(
                    S3Event::deserialize(raw_value).map_err(de::Error::custom)?,
                ))
            } else if records[0].get("Sns").is_some() {
                Ok(CombinedEvent::Sns(
                    SnsEvent::deserialize(raw_value).map_err(de::Error::custom)?,
                ))
            } else if records[0].get("eventSource").map_or(false, |v| v == "aws:sqs") {
                Ok(CombinedEvent::Sqs(
                    SqsEvent::deserialize(raw_value).map_err(de::Error::custom)?,
                ))
            } else if records[0].get("eventSource").map_or(false, |v| v == "aws:kinesis") {
                Ok(CombinedEvent::Kinesis(
                KinesisEvent::deserialize(raw_value).map_err(de::Error::custom)?,
                ))
            }
            else {
                Err(de::Error::custom("Unknown Records event type"))
            }
        } else if let Some(records) = raw_value.get("awslogs") {
            tracing::debug!("records: {:?}", records);
            Ok(CombinedEvent::CloudWatchLogs(
                AwsLogs::deserialize(records).map_err(de::Error::custom)?,
            ))
        } else if let Some(record) = raw_value.get("detail-type") {
            tracing::debug!("raw_value: {:?}", raw_value);
            
            tracing::debug!("record: {:?}", record);
            if record == "ECR Image Scan" {
                Ok(CombinedEvent::EcrScan(
                    EcrScanEvent::deserialize(raw_value).map_err(de::Error::custom)?,
                ))
            } else {
                Err(de::Error::custom("Unknown event type"))
            }
        }
        else {
            Err(de::Error::custom("Unknown event type"))
        }
    }
}
