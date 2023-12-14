use aws_config::BehaviorVersion;
use aws_lambda_events::event::cloudwatch_logs::AwsLogs;
use aws_lambda_events::event::s3::S3Event;
use aws_sdk_s3::Client;
use combined_event::CombinedEvent;
use cx_sdk_rest_logs::config::{BackoffConfig, LogExporterConfig};
use cx_sdk_rest_logs::{DynLogExporter, RestLogExporter};
use http::header::USER_AGENT;
use lambda_runtime::{run, service_fn, Error, LambdaEvent};
use std::collections::HashMap;
use std::string::String;
use std::sync::Arc;
use std::time::Duration;
use tracing::level_filters::LevelFilter;
use tracing::{debug, info};
use tracing_subscriber::EnvFilter;

use crate::config::{Config, IntegrationType};

mod combined_event;
mod config;
pub mod coralogix;
pub mod process;

#[tokio::main]
async fn main() -> Result<(), Error> {
    set_up_logging();

    info!(
        "Initializing {} version {}",
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION")
    );

    let aws_config = aws_config::load_defaults(BehaviorVersion::v2023_11_09()).await;
    let s3_client = Client::new(&aws_config);
    let mut config = Config::load_from_env()?;

    // if APIKey provided is an ARN, get the APIKey from Secrets Manager
    let api_key_value = config.api_key.token().to_string();
    if api_key_value.starts_with("arn:aws:secretsmanager:") {
        config.api_key = config::get_api_key_from_secrets_manager(&aws_config, api_key_value)
            .await
            .map_err(|e| e.to_string())?;
    };

    let coralogix_exporter = set_up_coralogix_exporter(&config)?;

    run(service_fn(|request: LambdaEvent<CombinedEvent>| {
        function_handler(&s3_client, coralogix_exporter.clone(), &config, request)
    }))
    .await
}

fn set_up_logging() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::builder()
                .with_default_directive(LevelFilter::INFO.into())
                .from_env_lossy(),
        )
        .init();
}

fn set_up_coralogix_exporter(config: &Config) -> Result<DynLogExporter, Error> {
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
async fn function_handler(
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
            let (bucket, key) = handle_s3_event(s3_event).await?;
            crate::process::s3(s3_client, coralogix_exporter, config, bucket, key).await?;
        }
        CombinedEvent::Sns(sns_event) => {
            debug!("SNS Event: {:?}", sns_event.records[0]);
            let message = &sns_event.records[0].sns.message;
            debug!("SNS Message: {:?}", message);
            //let json: serde_json::Value = serde_json::from_str(message)?;
            //let result = serde_json::from_str::<serde_json::Value>(message);

            if config.integration_type != IntegrationType::Sns {
                let records = serde_json::from_str::<serde_json::Value>(message)?;
                debug!("SNS S3 EVENT Detected");
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
                debug!("SNS TEXT EVENT Detected");
                crate::process::sns_logs(
                    sns_event.records[0].sns.message.clone(),
                    coralogix_exporter,
                    config,
                )
                .await?;
            }
        }
        CombinedEvent::CloudWatchLogs(awslogs) => {
            let cloudwatch_event_log = handle_cloudwatch_logs_event(awslogs).await?;
            crate::process::cloudwatch_logs(cloudwatch_event_log, coralogix_exporter, config)
                .await?;
        }
    };

    Ok(())
}

async fn handle_cloudwatch_logs_event(awslogs: AwsLogs) -> Result<AwsLogs, Error> {
    debug!("Cloudwatch Event: {:?}", awslogs.data);
    Ok(awslogs)
}
async fn handle_s3_event(s3_event: S3Event) -> Result<(String, String), Error> {
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
    Ok((bucket, key))
}

// async fn handle_sns_event(sns_event: SnsEvent) -> Result<(String, String), Error> {
//     let message = sns_event.records[0].sns.message.clone();
//     let json: serde_json::Value = serde_json::from_str(&message)?;
//     let bucket = json["Records"][0]["s3"]["bucket"]["name"]
//         .as_str()
//         .ok_or("Bucket name not found")?
//         .to_owned();
//     let key = json["Records"][0]["s3"]["object"]["key"]
//         .as_str()
//         .ok_or("Object key not found")?
//         .to_owned();
//     Ok((bucket, key))
// }
