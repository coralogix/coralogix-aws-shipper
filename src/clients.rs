use aws_config::SdkConfig;
use aws_sdk_cloudwatchlogs::config::Credentials as LogsCredentials;
use aws_sdk_cloudwatchlogs::Client as LogsClient;
use aws_sdk_ecr::config::Credentials as EcrCredentials;
use aws_sdk_ecr::Client as EcrClient;
use aws_sdk_s3::config::Credentials as S3Credentials;
use aws_sdk_s3::Client as S3Client;
use aws_sdk_sqs::Client as SqsClient;
use aws_sdk_sts::{Client as StsClient, Error as StsError};

/// A type used to hold the AWS clients required to interact with AWS services
/// used by the lambda function.
#[derive(Clone)]
pub struct AwsClients {
    pub s3: S3Client,
    pub ecr: EcrClient,
    pub sqs: SqsClient,
    pub logs: LogsClient,
}

impl AwsClients {
    pub fn new(sdk_config: &SdkConfig) -> Self {
        AwsClients {
            s3: S3Client::new(sdk_config),
            ecr: EcrClient::new(sdk_config),
            sqs: SqsClient::new(sdk_config),
            logs: LogsClient::new(sdk_config),
        }
    }

    // new_assume_role() method to create a new AWS client with the provided role
    pub async fn new_assume_role(sdk_config: &SdkConfig, role_arn: &str) -> Result<Self, StsError> {
        let sts_client = StsClient::new(sdk_config);
        let response = sts_client
            .assume_role()
            .role_arn(role_arn)
            .role_session_name("CoralogixAWSShipperSession")
            .send()
            .await?;

        // Extract temporary credentials
        let creds = response
            .credentials()
            .unwrap_or_else(|| panic!("no credentials found for role_arn: {}", role_arn));

        let s3_credentials = S3Credentials::new(
            creds.access_key_id(),
            creds.secret_access_key(),
            Some(creds.session_token().to_string()),
            None,
            "s3provider",
        );
        let ecr_credentials = EcrCredentials::new(
            creds.access_key_id(),
            creds.secret_access_key(),
            Some(creds.session_token().to_string()),
            None,
            "ecrprovider",
        );
        let logs_credentials = LogsCredentials::new(
            creds.access_key_id(),
            creds.secret_access_key(),
            Some(creds.session_token().to_string()),
            None,
            "logsprovider",
        );

        let s3 = S3Client::from_conf(assumed_role_s3_config(sdk_config, s3_credentials));
        let ecr_config = aws_sdk_ecr::config::Builder::from(sdk_config)
            .credentials_provider(ecr_credentials)
            .build();
        let logs_config = aws_sdk_cloudwatchlogs::config::Builder::from(sdk_config)
            .credentials_provider(logs_credentials)
            .build();

        Ok(AwsClients {
            s3,
            ecr: EcrClient::from_conf(ecr_config),

            // SQS permissions are only required for managing DLQ. The default client is sufficient for this.
            sqs: SqsClient::new(sdk_config),
            logs: LogsClient::from_conf(logs_config),
        })
    }
}

fn assumed_role_s3_config(
    sdk_config: &SdkConfig,
    credentials: S3Credentials,
) -> aws_sdk_s3::Config {
    aws_sdk_s3::config::Builder::from(sdk_config)
        .credentials_provider(credentials)
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_config::{BehaviorVersion, Region};

    #[test]
    fn assumed_role_s3_config_preserves_base_sdk_region() {
        let sdk_config = SdkConfig::builder()
            .behavior_version(BehaviorVersion::latest())
            .region(Region::new("us-gov-west-1"))
            .build();
        let credentials = S3Credentials::new(
            "access-key",
            "secret-key",
            Some("session-token".to_string()),
            None,
            "s3provider",
        );

        let s3_config = assumed_role_s3_config(&sdk_config, credentials);

        assert_eq!(
            s3_config.region().map(|region| region.as_ref()),
            Some("us-gov-west-1")
        );
    }
}
