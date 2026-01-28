use crate::assume_role;
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
            s3: S3Client::new(&sdk_config),
            ecr: EcrClient::new(&sdk_config),
            sqs: SqsClient::new(&sdk_config),
            logs: LogsClient::new(&sdk_config),
        }
    }

    // new_assume_role() method to create a new AWS client with the provided role
    pub async fn new_assume_role(sdk_config: &SdkConfig, role_arn: &str) -> Result<Self, StsError> {
        let sts_client = StsClient::new(&sdk_config);
        let response = sts_client
            .assume_role()
            .role_arn(role_arn)
            .role_session_name("CoralogixAWSShipperSession")
            .send()
            .await?;

        // Extract temporary credentials
        let creds = response
            .credentials()
            .expect(format!("no credentials found for role_arn: {}", role_arn).as_str());

        Ok(AwsClients {
            s3: assume_role!(
                "s3provider",
                role_arn,
                creds,
                S3Credentials,
                S3Client,
                sdk_config
            ),
            ecr: assume_role!(
                "ecrprovider",
                role_arn,
                creds,
                EcrCredentials,
                EcrClient,
                sdk_config
            ),

            // SQS permissions are only required for managing DLQ. The default client is sufficient for this.
            sqs: SqsClient::new(&sdk_config),
            logs: assume_role!(
                "logsprovider",
                role_arn,
                creds,
                LogsCredentials,
                LogsClient,
                sdk_config
            ),
        })
    }
}

// macro for creating a new AWS client with the provided role
#[macro_export]
macro_rules! assume_role {
    ($provider_name:expr, $role_arn:expr, $creds:expr, $credentials:ident, $client:ty, $sdk_config:expr) => {{
        let credentials = $credentials::new(
            $creds.access_key_id(),
            $creds.secret_access_key(),
            Some($creds.session_token().to_string()),
            None,
            $provider_name,
        );

        let config = aws_config::defaults(aws_config::BehaviorVersion::latest())
            .credentials_provider(credentials)
            .load()
            .await;

        <$client>::new(&config)
    }};
}
