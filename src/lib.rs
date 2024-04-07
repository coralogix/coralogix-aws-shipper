use async_recursion::async_recursion;
use aws_config::SdkConfig;
use aws_lambda_events::cloudwatch_logs::LogsEvent;
use aws_lambda_events::event::cloudwatch_logs::AwsLogs;
use aws_lambda_events::event::s3::S3Event;
use aws_sdk_ecr::Client as EcrClient;
use aws_sdk_s3::Client as S3Client;
use aws_sdk_sqs::types::MessageAttributeValue;
use aws_sdk_sqs::Client as SqsClient;
use chrono;
use combined_event::CombinedEvent;
use cx_sdk_rest_logs::config::{BackoffConfig, LogExporterConfig};
use cx_sdk_rest_logs::{DynLogExporter, RestLogExporter};
use http::header::USER_AGENT;
use lambda_runtime::{Context, Error, LambdaEvent};
use md5;
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
pub mod ecr;
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

#[async_recursion]
// lambda handler
pub async fn function_handler(
    clients: &AwsClients,
    coralogix_exporter: DynLogExporter,
    config: &Config,
    evt: LambdaEvent<CombinedEvent>,
) -> Result<(), Error> {
    info!("Handling lambda invocation");

    // TODO this may need to be moved process
    // TODO will this always produce just one bucket/key? (check this)
    debug!("Handling event: {:?}", evt);
    debug!("Handling event payload: {:?}", evt.payload);
    match evt.payload {
        CombinedEvent::S3(s3_event) => {
            info!("S3 EVENT Detected");
            let (bucket, key) = handle_s3_event(s3_event).await?;
            crate::process::s3(&clients.s3, coralogix_exporter, config, bucket, key).await?;
        }
        CombinedEvent::Sns(sns_event) => {
            debug!("SNS Event: {:?}", sns_event);
            let message = &sns_event.records[0].sns.message;
            if config.integration_type != IntegrationType::Sns {
                let s3_event = serde_json::from_str::<S3Event>(message)?;
                let (bucket, key) = handle_s3_event(s3_event).await?;
                info!("SNS S3 EVENT Detected");
                crate::process::s3(&clients.s3, coralogix_exporter, config, bucket, key).await?;
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
        CombinedEvent::CloudWatchLogs(logs_event) => {
            info!("CLOUDWATCH EVENT Detected");
            let cloudwatch_event_log = handle_cloudwatch_logs_event(logs_event).await?;
            crate::process::cloudwatch_logs(cloudwatch_event_log, coralogix_exporter, config)
                .await?;
        }
        CombinedEvent::Sqs(sqs_event) => {
            debug!("SQS Event: {:?}", sqs_event.records[0]);
            for record in &sqs_event.records {
                if let Some(message) = &record.body {
                    if config.integration_type != IntegrationType::Sqs {
                        let evt: CombinedEvent = serde_json::from_str(message)?;
                        let internal_event = LambdaEvent::new(evt, Context::default());

                        // recursively call function_handler
                        // note that there is no risk of hitting the recursion stack limit
                        // here as recursiion will only be called as many times as there are nested
                        // events in an SQS message
                        let result = function_handler(
                            clients,
                            coralogix_exporter.clone(),
                            config,
                            internal_event,
                        )
                        .await;

                        if result.is_ok() {
                            continue;
                        }

                        if let (Some(dlq_arn), Some(event_source_arn), Some(dlq_url)) = (
                            config.dlq_arn.clone(),
                            record.event_source_arn.clone(),
                            config.dlq_url.clone(),
                        ) {
                            if dlq_arn != event_source_arn {
                                continue;
                            }

                            let mut current_retry_count = record
                                .message_attributes
                                .get("retry")
                                .and_then(|attr| attr.string_value.as_deref()) // Convert Option<String> to Option<&str>
                                .map_or(Ok(0), str::parse::<i32>) // Parse as i32 or default to 0; map_or returns Result<i32, ParseIntError>
                                .unwrap_or(0); // In case of parse error, default to 0

                            let retry_limit = config
                                .dlq_retry_limit
                                .clone()
                                .unwrap_or("3".to_string())
                                .parse::<i32>()
                                .map_err(|e| format!("failed parse dlq retry limit - {}", e))?;

                            if current_retry_count >= retry_limit {
                                tracing::info!(
                                    "Retry limit reached for message: {:?}",
                                    record.body
                                );
                                s3_store_failed_event(
                                    &clients.s3,
                                    config.dlq_s3_bucket.clone().unwrap(),
                                    record.body.clone().unwrap(),
                                )
                                .await?;

                                continue;
                            }

                            // increment retry count
                            current_retry_count += 1;

                            let retry_attr = MessageAttributeValue::builder()
                                .set_data_type(Some("String".to_string()))
                                .set_string_value(Some(current_retry_count.to_string()))
                                .build()?;

                            let last_err_attr = MessageAttributeValue::builder()
                                .set_data_type(Some("String".to_string()))
                                .set_string_value(Some(result.err().unwrap().to_string()))
                                .build()?;

                            // send sqs event to dlq
                            clients
                                .sqs
                                .send_message()
                                .queue_url(dlq_url)
                                .message_attributes("retry", retry_attr)
                                .message_attributes("LastError", last_err_attr)
                                .message_body(message)
                                .send()
                                .await?;
                        }

                        // result.err();
                        // handle_dlq_error(result, clients, config, record);
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
        CombinedEvent::Kinesis(kinesis_event) => {
            for record in kinesis_event.records {
                debug!("Kinesis record: {:?}", record);
                let message = record.kinesis.data;
                debug!("Kinesis data: {:?}", &message);
                crate::process::kinesis_logs(message, coralogix_exporter.clone(), config).await?;
            }
        }
        CombinedEvent::Kafka(kafka_event) => {
            let mut all_records = Vec::new();
            for (topic_partition, mut records) in kafka_event.records {
                debug!("Kafka record: {topic_partition:?} --> {records:?}");
                all_records.append(&mut records)
            }
            crate::process::kafka_logs(all_records, coralogix_exporter.clone(), config).await?;
        }
        CombinedEvent::EcrScan(ecr_scan_event) => {
            debug!("ECR Scan event: {:?}", ecr_scan_event);
            crate::process::ecr_scan_logs(
                &clients.ecr,
                ecr_scan_event,
                coralogix_exporter.clone(),
                config,
            )
            .await?;
        }
    };

    Ok(())
}

pub async fn handle_cloudwatch_logs_event(logs_event: LogsEvent) -> Result<AwsLogs, Error> {
    debug!("Cloudwatch Event: {:?}", logs_event.aws_logs.data);
    Ok(logs_event.aws_logs)
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

/// A type used to hold the AWS clients required to interact with AWS services
/// used by the lambda function.
#[derive(Clone)]
pub struct AwsClients {
    pub s3: S3Client,
    pub ecr: EcrClient,
    pub sqs: SqsClient,
}

impl AwsClients {
    pub fn new(sdk_config: &SdkConfig) -> Self {
        AwsClients {
            s3: S3Client::new(&sdk_config),
            ecr: EcrClient::new(&sdk_config),
            sqs: SqsClient::new(&sdk_config),
        }
    }
}

async fn s3_store_failed_event(
    s3client: &S3Client,
    bucket: String,
    event: String,
) -> Result<(), String> {
    // create object name using md5sum of the data string
    let digest = md5::compute(event.as_bytes());
    let mut object_name = format!("{:x}.json", digest);
    let mut key = chrono::Local::now()
        .format("coraligx-aws-shipper/failed-events/%Y/%m/%d/%H")
        .to_string();

    let mut data = event.clone().as_bytes().to_owned();

    // if s3 event, read the object from s3
    if let Ok(e) = serde_json::from_str::<S3Event>(&event) {
        let bucket = e.records[0]
            .s3
            .bucket
            .name
            .as_deref()
            .ok_or_else(|| format!("failed to get bucket name"))?;
        let key = e.records[0]
            .s3
            .object
            .key
            .as_deref()
            .ok_or_else(|| format!("failed to get object key name"))?;
        object_name = format!("{}/{}", bucket, key);
        data = process::get_bytes_from_s3(s3client, bucket.to_string(), key.to_string())
            .await
            .map_err(|e| format!("failed to read object from s3 - {}", e))?;
    }

    // if cloudwatch logs event, use loggroup name and md5 sum as object name
    if let Ok(e) = serde_json::from_str::<LogsEvent>(&event) {
        object_name = format!(
            "{}/{}/{}",
            e.aws_logs.data.log_group, e.aws_logs.data.log_stream, object_name
        );
    }

    key = format!("{}/{}", key, object_name);
    let buffer =
        aws_smithy_types::byte_stream::ByteStream::new(aws_smithy_types::body::SdkBody::from(data));

    tracing::debug!("uploading failed event to S3: s3://{}", key);
    s3client
        .put_object()
        .bucket(bucket)
        .key(key)
        .body(buffer)
        .send()
        .await
        .map_err(|e| format!("failed uploading file to bucket - {}", e))?;

    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;
    use aws_lambda_events::event::s3::S3Event;

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
        let evt: S3Event =
            serde_json::from_str(s3_event.as_str()).expect("failed to parse s3_event");
        let (bucket, key) = handle_s3_event(evt).await.unwrap();
        assert_eq!(bucket, "coralogix-serverless-repo");
        assert_eq!(key, "coralogix-aws-shipper/s3.log");

        // test s3 event with spaces in key name (note: aws event replaces spaces with +)
        let s3_event = s3_event_str(
            "coralogix-serverless-repo",
            "coralogix-aws-shipper/s3+with+spaces.log",
        );
        let evt: S3Event =
            serde_json::from_str(s3_event.as_str()).expect("failed to parse s3_event");
        let (bucket, key) = handle_s3_event(evt).await.unwrap();
        assert_eq!(bucket, "coralogix-serverless-repo");
        assert_eq!(key, "coralogix-aws-shipper/s3 with spaces.log");
    }
}
