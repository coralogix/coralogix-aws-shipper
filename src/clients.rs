use aws_sdk_s3::Client as S3Client;
use aws_sdk_sqs::Client as SqsClient;
use aws_sdk_ecr::Client as EcrClient;
use aws_config::SdkConfig;


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
