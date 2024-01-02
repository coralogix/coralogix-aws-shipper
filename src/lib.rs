use aws_lambda_events::event::cloudwatch_logs::AwsLogs;
use aws_lambda_events::event::s3::S3Event;
use aws_sdk_s3::Client;
use combined_event::CombinedEvent;
use cx_sdk_rest_logs::config::{BackoffConfig, LogExporterConfig};
use cx_sdk_rest_logs::{DynLogExporter, RestLogExporter};
use http::header::USER_AGENT;
use lambda_runtime::{Error, LambdaEvent};
use std::collections::HashMap;
use std::string::String;
use std::sync::Arc;
use std::time::Duration;
use tracing::level_filters::LevelFilter;
use tracing::{debug, info};
use tracing_subscriber::EnvFilter;

use crate::config::{Config, IntegrationType};

pub mod combined_event;
pub mod config;
pub mod coralogix;
pub mod process;

pub fn set_up_logging() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::builder()
                .with_default_directive(LevelFilter::WARN.into())
                .from_env_lossy(),
        )
        .init();
}

pub fn set_up_coralogix_exporter(config: &Config) -> Result<DynLogExporter, Error> {
    let backoff = BackoffConfig {
        initial_delay: Duration::from_millis(10000),
        max_delay: Duration::from_millis(60000),
        max_elapsed_time: Duration::from_secs(config.max_elapsed_time),
    };

    let mut headers: HashMap<String, String> = HashMap::new();
    headers.insert(
        USER_AGENT.to_string(),
        concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"),).to_owned(),
    );
    headers.insert(
        "X-Coralogix-Data-Source".to_owned(),
        config.integration_type.to_string(),
    );

    let config = LogExporterConfig {
        url: config.endpoint.clone(),
        request_timeout: Duration::from_secs(30),
        backoff_config: backoff,
        additional_headers: headers,
    };
    let exporter = Arc::new(RestLogExporter::builder().with_config(config).build()?);

    Ok(exporter)
}

// lambda handler
pub async fn function_handler(
    s3_client: &Client,
    coralogix_exporter: DynLogExporter,
    config: &Config,
    evt: LambdaEvent<CombinedEvent>,
) -> Result<(), Error> {
    info!("Handling lambda invocation");

    // TODO this may need to be moved process
    // TODO will this always produce just one bucket/key? (check this)
    match evt.payload {
        CombinedEvent::S3(s3_event) => {
            info!("S3 EVENT Detected");
            let (bucket, key) = handle_s3_event(s3_event).await?;
            crate::process::s3(s3_client, coralogix_exporter, config, bucket, key).await?;
        }
        CombinedEvent::Sns(sns_event) => {
            debug!("SNS Event: {:?}", sns_event);
            let message = &sns_event.records[0].sns.message;
            if config.integration_type != IntegrationType::Sns {
                let records = serde_json::from_str::<serde_json::Value>(message)?;
                info!("SNS S3 EVENT Detected");
                let bucket = records["Records"][0]["s3"]["bucket"]["name"]
                    .as_str()
                    .ok_or("Bucket name not found")?
                    .to_owned();
                let key = records["Records"][0]["s3"]["object"]["key"]
                    .as_str()
                    .ok_or("Object key not found")?
                    .to_owned();
                crate::process::s3(s3_client, coralogix_exporter, config, bucket, key).await?;
            } else {
                info!("SNS TEXT EVENT Detected");
                crate::process::sns_logs(
                    sns_event.records[0].sns.message.clone(),
                    coralogix_exporter,
                    config,
                )
                .await?;
            }
        }
        CombinedEvent::CloudWatchLogs(awslogs) => {
            info!("CLOUDWATCH EVENT Detected");
            let cloudwatch_event_log = handle_cloudwatch_logs_event(awslogs).await?;
            crate::process::cloudwatch_logs(cloudwatch_event_log, coralogix_exporter, config)
                .await?;
        }
        CombinedEvent::Sqs(sqs_event) => {
            debug!("SQS Event: {:?}", sqs_event.records[0]);
            for record in &sqs_event.records {
                if let Some(message) = &record.body {
                    if config.integration_type != IntegrationType::Sqs {
                        let records = serde_json::from_str::<serde_json::Value>(message)?;
                        debug!("SQS S3 EVENT Detected");
                        let bucket = records["Records"][0]["s3"]["bucket"]["name"]
                            .as_str()
                            .ok_or("Bucket name not found")?
                            .to_owned();
                        let key = records["Records"][0]["s3"]["object"]["key"]
                            .as_str()
                            .ok_or("Object key not found")?
                            .to_owned();
            
                        crate::process::s3(s3_client, coralogix_exporter.clone(), config, bucket, key).await?;
                    } else {
                        debug!("SQS TEXT EVENT Detected");
                        crate::process::sqs_logs(
                            message.clone(),
                            coralogix_exporter.clone(),
                            config,
                        )
                        .await?;
                    }
                }
            }
        }
    };

    Ok(())
}

pub async fn handle_cloudwatch_logs_event(awslogs: AwsLogs) -> Result<AwsLogs, Error> {
    debug!("Cloudwatch Event: {:?}", awslogs.data);
    Ok(awslogs)
}
pub async fn handle_s3_event(s3_event: S3Event) -> Result<(String, String), Error> {
    debug!("S3 Event: {:?}", s3_event);
    let bucket = s3_event.records[0]
        .s3
        .bucket
        .name
        .as_ref()
        .expect("Bucket name to exist")
        .to_owned();
    let key = s3_event.records[0]
        .s3
        .object
        .key
        .as_ref()
        .expect("Object key to exist")
        .to_owned();

    let decoded_key = percent_encoding::percent_decode_str(&key)
        .decode_utf8()?
        .replace("+", " ");
    
    Ok((bucket, decoded_key))
}


#[cfg(test)]
mod test {
    use aws_lambda_events::event::s3::S3Event;
    use super::*;

    // Note: we test the s3_event handler directly here, since the integration tests will bypass it
    // using the mock s3 client. The [handle_cloudwatch_logs_event] is however invoked as part of the
    // integration test workflow.
    #[tokio::test]
    async fn test_handle_s3_event() {
        let s3_event_str = |bucket: &str, key: &str| -> String {
            format!(
                r#"{{
                "Records": [
                    {{
                    "eventVersion": "2.0",
                    "eventSource": "aws:s3",
                    "awsRegion": "eu-west-1",
                    "eventTime": "1970-01-01T00:00:00.000Z",
                    "eventName": "ObjectCreated:Put",
                    "userIdentity": {{
                        "principalId": "EXAMPLE"
                    }},
                    "requestParameters": {{
                        "sourceIPAddress": "127.0.0.1"
                    }},
                    "responseElements": {{
                        "x-amz-request-id": "EXAMPLE123456789",
                        "x-amz-id-2": "EXAMPLE123/5678abcdefghijklambdaisawesome/mnopqrstuvwxyzABCDEFGH"
                    }},
                    "s3": {{
                        "s3SchemaVersion": "1.0",
                        "configurationId": "testConfigRule",
                        "bucket": {{
                        "name": "{}",
                        "ownerIdentity": {{
                            "principalId": "EXAMPLE"
                        }},
                        "arn": "arn:aws:s3:::{}"
                        }},
                        "object": {{
                        "key": "{}",
                        "size": 311000048,
                        "eTag": "0123456789abcdef0123456789abcdef",
                        "sequencer": "0A1B2C3D4E5F678901"
                        }}
                    }}
                    }}
                ]
            }}"#,
                bucket, bucket, key
            )
        };

        // test normal s3 event
        let s3_event = s3_event_str("coralogix-serverless-repo", "coralogix-aws-shipper/s3.log");
        let evt: S3Event = serde_json::from_str(s3_event.as_str()).expect("failed to parse s3_event");
        let (bucket, key) = handle_s3_event(evt).await.unwrap();
        assert_eq!(bucket, "coralogix-serverless-repo");
        assert_eq!(key, "coralogix-aws-shipper/s3.log");

        // test s3 event with spaces in key name (note: aws event replaces spaces with +)
        let s3_event = s3_event_str("coralogix-serverless-repo", "coralogix-aws-shipper/s3+with+spaces.log");
        let evt: S3Event = serde_json::from_str(s3_event.as_str()).expect("failed to parse s3_event");
        let (bucket, key) = handle_s3_event(evt).await.unwrap();
        assert_eq!(bucket, "coralogix-serverless-repo");
        assert_eq!(key, "coralogix-aws-shipper/s3 with spaces.log");
    }
    
}