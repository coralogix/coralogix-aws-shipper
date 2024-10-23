pub mod logs;
pub mod clients;
pub mod events;

use aws_config::BehaviorVersion;
use crate::events::Combined;
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

    let aws_config = aws_config::load_defaults(BehaviorVersion::v2024_03_28()).await;
    let aws_clients = clients::AwsClients::new(&aws_config);
    // let mut config = config::Config::load_from_env()?;


    match mode {
        TelemetryMode::Metrics => {
            warn!("metrics telemetry mode not implemented");
            Ok(())
        }

        TelemetryMode::Traces => {
            warn!("traces telemetry mode not implemented");
            Ok(())
        }

        // default to logs telemetry mode
        _ => {
            info!("Running in logs telemetry mode");
            let mut conf = crate::logs::config::Config::load_from_env()?;
            let api_key_value = conf.api_key.token().to_string();
            if api_key_value.starts_with("arn:aws") && api_key_value.contains(":secretsmanager") {
                conf.api_key = crate::logs::config::get_api_key_from_secrets_manager(&aws_config, api_key_value)
                    .await
                    .map_err(|e| e.to_string())?;
            };

            let coralogix_exporter = crate::logs::set_up_coralogix_exporter(&conf)?;
            run(service_fn(|request: LambdaEvent<Combined>| {
                logs::function_handler(
                    &aws_clients,
                    coralogix_exporter.clone(),
                    &conf,
                    request,
                )
            }))
            .await
        }
    }
}
