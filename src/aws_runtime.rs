use aws_config::{BehaviorVersion, SdkConfig};
use aws_smithy_http_client::{
    tls::{self, rustls_provider::CryptoMode},
    Builder,
};
use std::env;

const ENABLE_AWS_FIPS: &str = "ENABLE_AWS_FIPS";

pub fn aws_fips_enabled() -> bool {
    env::var(ENABLE_AWS_FIPS)
        .map(|value| matches!(value.to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
        .unwrap_or(false)
}

pub async fn load_sdk_config() -> SdkConfig {
    if aws_fips_enabled() {
        tracing::info!("AWS FIPS mode enabled for SDK HTTP client");
        let http_client = Builder::new()
            .tls_provider(tls::Provider::Rustls(CryptoMode::AwsLcFips))
            .build_https();

        aws_config::defaults(BehaviorVersion::latest())
            .http_client(http_client)
            .load()
            .await
    } else {
        aws_config::load_defaults(BehaviorVersion::latest()).await
    }
}

#[cfg(test)]
mod tests {
    use super::ENABLE_AWS_FIPS;

    #[test]
    fn aws_fips_enabled_accepts_truthy_values() {
        for value in ["true", "1", "yes"] {
            temp_env::with_var(ENABLE_AWS_FIPS, Some(value), || {
                assert!(
                    super::aws_fips_enabled(),
                    "expected {value:?} to enable AWS FIPS"
                );
            });
        }
    }

    #[test]
    fn aws_fips_enabled_rejects_missing_or_false_values() {
        temp_env::with_var(ENABLE_AWS_FIPS, None::<&str>, || {
            assert!(!super::aws_fips_enabled());
        });

        for value in ["false", "0", "no", ""] {
            temp_env::with_var(ENABLE_AWS_FIPS, Some(value), || {
                assert!(
                    !super::aws_fips_enabled(),
                    "expected {value:?} to leave AWS FIPS disabled"
                );
            });
        }
    }
}
