use aws_lambda_events::event::cloudwatch_logs::AwsLogs;
use aws_lambda_events::event::s3::S3Event;
use aws_lambda_events::event::sns::SnsEvent;
use aws_lambda_events::event::sqs::SqsEvent;
use serde::de::{self, Deserialize, Deserializer};
use serde_json::Value;

pub enum CombinedEvent {
    S3(S3Event),
    Sns(SnsEvent),
    CloudWatchLogs(AwsLogs),
    Sqs(SqsEvent),
}

impl<'de> Deserialize<'de> for CombinedEvent {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw_value: Value = Deserialize::deserialize(deserializer)?;

        // Attempt to match against known event structures
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
            } else if records[0].get("body").is_some() {
                Ok(CombinedEvent::Sqs(
                    SqsEvent::deserialize(raw_value).map_err(de::Error::custom)?,
                ))
            } else {
                Err(de::Error::custom("Unknown Records event type"))
            }
        } else if let Some(records) = raw_value.get("awslogs") {
            tracing::info!("records: {:?}", records);
            Ok(CombinedEvent::CloudWatchLogs(
                AwsLogs::deserialize(records).map_err(de::Error::custom)?,
            ))
            //Ok(CombinedEvent::CloudWatchLogs(
            //    AwsLogs::deserialize(raw_value).map_err(de::Error::custom)?,
        } else {
            Err(de::Error::custom("Unknown event type"))
        }
    }
}
