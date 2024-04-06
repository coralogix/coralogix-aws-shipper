use aws_config::BehaviorVersion;
use aws_sdk_s3::Client;
use aws_sdk_ecr::Client as EcrClient;
use coralogix_aws_shipper::combined_event::CombinedEvent;
use coralogix_aws_shipper::config;
use lambda_runtime::{run, service_fn, Error, LambdaEvent};
use tracing::info;

#[tokio::main]
async fn main() -> Result<(), Error> {
    coralogix_aws_shipper::set_up_logging();

    info!(
        "Initializing {} version {}",
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION")
    );

    let aws_config = aws_config::load_defaults(BehaviorVersion::v2023_11_09()).await;
    let clients = coralogix_aws_shipper::AwsClients::new(&aws_config); 
    let mut config = config::Config::load_from_env()?;

    // if APIKey provided is an ARN, get the APIKey from Secrets Manager
    let api_key_value = config.api_key.token().to_string();
    if api_key_value.starts_with("arn:aws:secretsmanager:") {
        config.api_key = config::get_api_key_from_secrets_manager(&aws_config, api_key_value)
            .await
            .map_err(|e| e.to_string())?;
    };

    let coralogix_exporter = coralogix_aws_shipper::set_up_coralogix_exporter(&config)?;

    run(service_fn(|request: LambdaEvent<CombinedEvent>| {
        coralogix_aws_shipper::function_handler(
            &clients,
            coralogix_exporter.clone(),
            &config,
            request,
        )
    }))
    .await
}
