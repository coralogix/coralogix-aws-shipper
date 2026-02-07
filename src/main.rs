pub mod clients;
pub mod events;
pub mod logs;
pub mod metrics;

use crate::events::Combined;
use aws_config::BehaviorVersion;
use lambda_runtime::{run, service_fn, Error, LambdaEvent};
use tracing::{info, warn};

// define constants TELEMETRY_MODE env variable name
const TELEMETRY_MODE: &str = "TELEMETRY_MODE";

enum TelemetryMode {
    Logs,
    Metrics,
    Traces,
}

impl From<&str> for TelemetryMode {
    fn from(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "logs" => TelemetryMode::Logs,
            "metrics" => TelemetryMode::Metrics,
            "traces" => TelemetryMode::Traces,
            _ => {
                warn!("Invalid telemetry mode, {} defaulting to [logs]", s);
                TelemetryMode::Logs
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    coralogix_aws_shipper::set_up_logging();

    info!(
        "Initializing {} version {}",
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION")
    );

    let mode = TelemetryMode::from(
        std::env::var(TELEMETRY_MODE)
            .unwrap_or("".to_string())
            .as_str(),
    );

    let aws_config = aws_config::load_defaults(BehaviorVersion::v2025_01_17()).await;
    let mut aws_clients = clients::AwsClients::new(&aws_config);

    match mode {
        TelemetryMode::Traces => {
            warn!("traces telemetry mode not implemented");
            Ok(())
        }

        TelemetryMode::Metrics => {
            info!("running in metrics telemetry mode");
            let mut conf = metrics::config::Config::load_from_env()?;
            if conf.api_key.token().starts_with("arn:aws")
                && conf.api_key.token().contains(":secretsmanager")
            {
                // TODO: move the get_api_key_from_secrets_manager out of the logs module
                conf.api_key = crate::logs::config::get_api_key_from_secrets_manager(
                    &aws_config,
                    conf.api_key.token().to_string(),
                )
                .await
                .map_err(|e| e.to_string())?;
            };

            run(service_fn(|request: LambdaEvent<Combined>| {
                metrics::handler(&conf, request)
            }))
            .await
        }

        // default to logs telemetry mode
        _ => {
            info!("Running in logs telemetry mode");
            let mut conf = crate::logs::config::Config::load_from_env()?;
            let api_key_value = conf.api_key.token().to_string();
            if api_key_value.starts_with("arn:aws") && api_key_value.contains(":secretsmanager") {
                conf.api_key = crate::logs::config::get_api_key_from_secrets_manager(
                    &aws_config,
                    api_key_value,
                )
                .await
                .map_err(|e| e.to_string())?;
            };

            // override config if using assume role
            if let Some(role_arn) = conf.lambda_assume_role.as_ref() {
                aws_clients = clients::AwsClients::new_assume_role(&aws_config, role_arn).await?;
            }

            let coralogix_exporter = crate::logs::set_up_coralogix_exporter(&conf)?;
            run(service_fn(|request: LambdaEvent<Combined>| {
                logs::handler(&aws_clients, coralogix_exporter.clone(), &conf, &aws_config, request)
            }))
            .await
        }
    }
}
