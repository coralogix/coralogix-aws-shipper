use std::fmt::Debug;
use std::num::ParseIntError;
use std::str::FromStr;
use std::string::String;
use std::{env, fmt};

use aws_config::SdkConfig;
use aws_sdk_secretsmanager::operation::get_secret_value::GetSecretValueError;
use cx_sdk_rest_logs::auth::ApiKey;

pub struct Config {
    pub newline_pattern: String,  // this should be regex
    pub blocking_pattern: String, // this should be regex
    pub sampling: usize,
    pub logs_per_batch: usize,
    pub integration_type: IntegrationType,
    pub app_name: Option<String>,
    pub sub_name: Option<String>,
    pub api_key: ApiKey,
    pub endpoint: String,
    pub max_elapsed_time: u64,
    pub csv_delimiter: String,
    pub batches_max_size: usize,
    pub batches_max_concurrency: usize,
    pub add_metadata: String,
    pub dlq_arn: Option<String>,
    pub dlq_url: Option<String>,
    pub dlq_retry_limit: Option<String>,
    pub dlq_s3_bucket: Option<String>,
    pub lambda_assume_role: Option<String>,
    pub enable_log_group_tags: bool,
}

#[derive(PartialEq, Eq, Debug, Clone, Copy)]
pub enum IntegrationType {
    VpcFlow,
    S3Csv,
    S3,
    CloudTrail,
    CloudWatch,
    Sns,
    Sqs,
    Kinesis,
    CloudFront,
    Kafka,
    EcrScan,
}

impl FromStr for IntegrationType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        match s {
            "VpcFlow" => Ok(IntegrationType::VpcFlow),
            "S3Csv" => Ok(IntegrationType::S3Csv),
            "S3" => Ok(IntegrationType::S3),
            "CloudTrail" => Ok(IntegrationType::CloudTrail),
            "CloudWatch" => Ok(IntegrationType::CloudWatch),
            "Sns" => Ok(IntegrationType::Sns),
            "Sqs" => Ok(IntegrationType::Sqs),
            "Kinesis" => Ok(IntegrationType::Kinesis),
            "CloudFront" => Ok(IntegrationType::CloudFront),
            "MSK" => Ok(IntegrationType::Kafka),
            "Kafka" => Ok(IntegrationType::Kafka),
            "EcrScan" => Ok(IntegrationType::EcrScan),
            other => Err(format!("Invalid or Unsupported integration type {}", other)),
        }
    }
}

impl fmt::Display for IntegrationType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl Config {
    pub fn load_from_env() -> Result<Config, String> {
        // let conf: Config;
        let conf = Config {
            newline_pattern: env::var("NEWLINE_PATTERN").unwrap_or("".to_string()),

            blocking_pattern: env::var("BLOCKING_PATTERN").unwrap_or("".to_string()),

            sampling: env::var("SAMPLING")
                .map_err(|e| format!("sampling not set - {}", e))?
                .to_string()
                .parse()
                .map_err(|e: ParseIntError| format!("error parsing sampling to int - {}", e))?,
            logs_per_batch: env::var("LOGS_PER_BATCH")
                .unwrap_or("500".to_string())
                .parse::<usize>()
                .map_err(|e| format!("Error parsing LOGS_PER_BATCH to usize - {}", e))?,
            integration_type: env::var("INTEGRATION_TYPE")
                .map_err(|e| format!("INTEGRATION_TYPE not set - {}", e))
                .and_then(|s| s.parse::<IntegrationType>())?,

            api_key: env::var("CORALOGIX_API_KEY")
                .map_err(|e| format!("CORALOGIX_API_KEY not set - {}", e))?
                .into(),
            endpoint: env::var("CORALOGIX_ENDPOINT")
                .map_err(|e| format!("CORALOGIX_ENDPOINT not set - {}", e))?,

            app_name: env::var("APP_NAME").ok(),
            sub_name: env::var("SUB_NAME").ok(),
            max_elapsed_time: env::var("MAX_ELAPSED_TIME")
                .unwrap_or("250".to_string())
                .parse::<u64>()
                .map_err(|e| format!("Error parsing MAX_ELAPSED_TIME to u64 - {}", e))?,
            csv_delimiter: env::var("CSV_DELIMITER").unwrap_or(",".to_string()),
            batches_max_size: env::var("BATCHES_MAX_SIZE")
                .unwrap_or("4".to_string())
                .parse::<usize>()
                .map_err(|e| format!("Error parsing BATCHES_MAX_SIZE to usize - {}", e))?,
            batches_max_concurrency: env::var("BATCHES_MAX_CONCURRENCY")
                .unwrap_or("10".to_string())
                .parse::<usize>()
                .map_err(|e| format!("Error parsing BATCHES_MAX_CONCURRENCY to usize - {}", e))?,
            add_metadata: env::var("ADD_METADATA").unwrap_or(" ".to_string()),
            dlq_arn: env::var("DLQ_ARN").ok(),
            dlq_url: env::var("DLQ_URL").ok(),
            dlq_retry_limit: env::var("DLQ_RETRY_LIMIT").ok(),
            dlq_s3_bucket: env::var("DLQ_S3_BUCKET").ok(),
            lambda_assume_role: env::var("LAMBDA_ASSUME_ROLE").ok(),
            enable_log_group_tags: env::var("ENABLE_LOG_GROUP_TAGS")
                .unwrap_or("false".to_string())
                .parse::<bool>()
                .unwrap_or(false),
        };

        Ok(conf)
    }
}

#[derive(thiserror::Error, Debug)]
pub enum KeySourceError {
    #[error("Failed to access AWS Secrets Manager. Please make sure the lambda function has permissions to access the {secret_id} secret. Error: {error:?}")]
    FailedToAccessSecretsManager {
        secret_id: String,
        error: GetSecretValueError,
    },
    #[error("Didn't find the {secret_id} secret in AWS secretsmanager")]
    MissingSecret { secret_id: String },
}

pub async fn get_api_key_from_secrets_manager(
    aws_config: &SdkConfig,
    secret_id: String,
) -> Result<ApiKey, Box<dyn std::error::Error>> {
    let secretsmanager = aws_sdk_secretsmanager::Client::new(aws_config);
    let response = secretsmanager
        .get_secret_value()
        .set_secret_id(Some(secret_id.clone()))
        .send()
        .await
        .map_err(|error| KeySourceError::FailedToAccessSecretsManager {
            secret_id: secret_id.clone(),
            error: error.into_service_error(),
        })?;
    let secret = response
        .secret_string
        .ok_or(KeySourceError::MissingSecret { secret_id })?;
    Ok(ApiKey::from(secret))
}
