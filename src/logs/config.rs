use std::fmt::Debug;
use std::num::ParseIntError;
use std::str::FromStr;
use std::string::String;
use std::{env, fmt};

use aws_config::SdkConfig;
use aws_sdk_s3::Client as S3Client;
use aws_sdk_s3::config::Credentials as S3Credentials;
use aws_sdk_secretsmanager::operation::get_secret_value::GetSecretValueError;
use aws_sdk_sts::Client as StsClient;
use cx_sdk_rest_logs::auth::ApiKey;
use thiserror::Error;

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
    pub starlark_script: Option<String>,
    pub enable_log_group_tags: bool,
    pub log_group_tags_cache_ttl_seconds: u64,
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
            starlark_script: env::var("STARLARK_SCRIPT").ok().filter(|s| !s.trim().is_empty()),
            enable_log_group_tags: env::var("ENABLE_LOG_GROUP_TAGS")
                .unwrap_or("false".to_string())
                .parse::<bool>()
                .unwrap_or(false),
            log_group_tags_cache_ttl_seconds: env::var("LOG_GROUP_TAGS_CACHE_TTL_SECONDS")
                .unwrap_or("300".to_string())
                .parse::<u64>()
                .map_err(|e| format!("Error parsing LOG_GROUP_TAGS_CACHE_TTL_SECONDS to u64 - {}", e))?,
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

// ============================================================================
// Starlark Script Loading
// ============================================================================

/// Errors that can occur during script loading
#[derive(Error, Debug)]
pub enum ScriptLoadError {
    #[error("Invalid S3 path format: {0}. Expected format: s3://bucket/key")]
    InvalidS3Path(String),
    #[error("S3 error: {0}")]
    S3Error(String),
    #[error("HTTP error: {0}")]
    HttpError(reqwest::StatusCode),
    #[error("Network error: {0}")]
    NetworkError(#[from] reqwest::Error),
    #[error("Invalid base64 encoding: {0}")]
    Base64Error(#[from] base64::DecodeError),
    #[error("Script is not valid UTF-8")]
    InvalidUtf8,
}

impl Config {
    /// Resolve the Starlark script by auto-detecting the source type.
    /// Supports: S3 paths (s3://bucket/key), HTTP/HTTPS URLs, Base64-encoded strings, or raw scripts.
    pub async fn resolve_starlark_script(
        &self,
        aws_config: &SdkConfig,
    ) -> Result<Option<String>, ScriptLoadError> {
        let Some(ref script_value) = self.starlark_script else {
            return Ok(None);
        };

        let trimmed = script_value.trim();

        // Auto-detect S3 path: starts with s3://
        if trimmed.starts_with("s3://") {
            return Self::load_from_s3(trimmed, aws_config, self.lambda_assume_role.as_deref()).await.map(Some);
        }

        // Auto-detect URL: starts with http:// or https://
        if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
            return Self::load_from_url(trimmed).await.map(Some);
        }

        // Before Base64 detection, strip all whitespace for wrapped base64 support
        // (base64 command wraps output at 76 chars by default)
        let stripped = trimmed.chars().filter(|c| !c.is_whitespace()).collect::<String>();

        // Auto-detect Base64: base64 characters only, reasonable length
        // Base64 strings are typically longer and don't contain spaces/newlines when encoded
        if Self::looks_like_base64(&stripped) {
            return Self::decode_base64(&stripped).map(Some);
        }

        // Otherwise, treat as raw script
        Ok(Some(trimmed.to_string()))
    }

    /// Heuristic to detect if a string looks like base64-encoded content
    fn looks_like_base64(s: &str) -> bool {
        // Base64 strings are typically:
        // - At least 20 characters (reasonable minimum for encoded script)
        // - Only contain base64 characters (A-Z, a-z, 0-9, +, /, =)
        // - Length is a multiple of 4 (or ends with padding)
        // Note: Whitespace (including newlines) should be stripped before calling this function
        if s.len() < 20 {
            return false;
        }

        // Check if it's all base64 characters
        let base64_chars: std::collections::HashSet<char> = 
            "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/="
                .chars()
                .collect();
        
        let all_base64 = s.chars().all(|c| base64_chars.contains(&c));
        
        // Must be all base64 chars AND (length multiple of 4 OR ends with padding)
        // AND doesn't look like actual Starlark code (contains common keywords)
        let looks_like_code = s.contains("def ") 
            || s.contains("return ") 
            || (s.contains("if ") && s.contains(":"))
            || s.contains("event")
            || s.contains("transform");
        
        all_base64 && !looks_like_code && (s.len() % 4 == 0 || s.ends_with('='))
    }

    /// Load a Starlark script from an S3 bucket
    async fn load_from_s3(
        s3_path: &str,
        aws_config: &SdkConfig,
        assume_role_arn: Option<&str>,
    ) -> Result<String, ScriptLoadError> {
        // Parse s3://bucket/key format
        let (bucket, key) = Self::parse_s3_path(s3_path)?;
        
        // Build S3 client with assumed-role credentials if configured, otherwise use base config
        let s3_client = if let Some(role_arn) = assume_role_arn {
            // Perform STS assume-role to get temporary credentials
            let sts_client = StsClient::new(aws_config);
            let response = sts_client
                .assume_role()
                .role_arn(role_arn)
                .role_session_name("CoralogixAWSShipperSession")
                .send()
                .await
                .map_err(|e| ScriptLoadError::S3Error(format!("STS assume-role failed: {}", e)))?;

            // Extract temporary credentials
            let creds = response
                .credentials()
                .ok_or_else(|| ScriptLoadError::S3Error(format!("No credentials found for role_arn: {}", role_arn)))?;

            // Build S3 client with assumed-role credentials
            let credentials = S3Credentials::new(
                creds.access_key_id(),
                creds.secret_access_key(),
                Some(creds.session_token().to_string()),
                None,
                "s3provider",
            );

            let config = aws_config::defaults(aws_config::BehaviorVersion::latest())
                .credentials_provider(credentials)
                .region(aws_config.region().cloned())
                .load()
                .await;

            S3Client::new(&config)
        } else {
            // Use base config (no assume-role)
            S3Client::new(aws_config)
        };

        let response = s3_client
            .get_object()
            .bucket(bucket)
            .key(key)
            .send()
            .await
            .map_err(|e| ScriptLoadError::S3Error(e.to_string()))?;

        // Download the content similar to process.rs
        let mut data = Vec::new();
        let mut body = response.body;
        while let Some(result) = body.next().await {
            let bytes = result.map_err(|e| ScriptLoadError::S3Error(e.to_string()))?;
            data.extend_from_slice(&bytes[..]);
        }

        String::from_utf8(data).map_err(|_| ScriptLoadError::InvalidUtf8)
    }

    /// Load a Starlark script from an HTTP/HTTPS URL
    async fn load_from_url(url: &str) -> Result<String, ScriptLoadError> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(ScriptLoadError::NetworkError)?;
        let response = client.get(url).send().await?;
        if !response.status().is_success() {
            return Err(ScriptLoadError::HttpError(response.status()));
        }
        response.text().await.map_err(ScriptLoadError::NetworkError)
    }

    /// Decode a base64-encoded Starlark script
    fn decode_base64(encoded: &str) -> Result<String, ScriptLoadError> {
        use base64::prelude::*;
        let bytes = BASE64_STANDARD.decode(encoded)?;
        String::from_utf8(bytes).map_err(|_| ScriptLoadError::InvalidUtf8)
    }

    /// Parse an S3 path in the format s3://bucket/key
    fn parse_s3_path(s3_path: &str) -> Result<(String, String), ScriptLoadError> {
        if !s3_path.starts_with("s3://") {
            return Err(ScriptLoadError::InvalidS3Path(s3_path.to_string()));
        }

        let path = &s3_path[5..]; // Remove "s3://" prefix
        let parts: Vec<&str> = path.splitn(2, '/').collect();

        if parts.len() != 2 || parts[0].is_empty() || parts[1].is_empty() {
            return Err(ScriptLoadError::InvalidS3Path(s3_path.to_string()));
        }

        Ok((parts[0].to_string(), parts[1].to_string()))
    }
}

#[cfg(test)]
mod script_loading_tests {
    use super::*;

    #[test]
    fn test_parse_s3_path_valid() {
        let (bucket, key) = Config::parse_s3_path("s3://my-bucket/path/to/script.star").unwrap();
        assert_eq!(bucket, "my-bucket");
        assert_eq!(key, "path/to/script.star");
    }

    #[test]
    fn test_parse_s3_path_invalid_format() {
        assert!(Config::parse_s3_path("https://example.com/script.star").is_err());
        assert!(Config::parse_s3_path("s3://bucket").is_err());
        assert!(Config::parse_s3_path("s3://").is_err());
    }

    #[test]
    fn test_decode_base64() {
        use base64::prelude::*;
        let script = "def transform(event):\n    return [event]";
        let encoded = BASE64_STANDARD.encode(script);
        let decoded = Config::decode_base64(&encoded).unwrap();
        assert_eq!(decoded, script);
    }

    #[test]
    fn test_decode_base64_invalid() {
        assert!(Config::decode_base64("not valid base64!!!").is_err());
    }

    #[test]
    fn test_looks_like_base64() {
        use base64::prelude::*;
        
        // Valid base64 string
        let script = "def transform(event):\n    return [event]";
        let encoded = BASE64_STANDARD.encode(script);
        assert!(Config::looks_like_base64(&encoded));
        
        // Valid - base64 with newlines (whitespace should be stripped before calling)
        let wrapped = "ZGVmIHRyYW5zZm9ybShldmVudCk6\nICAgIHJldHVybiBbZXZlbnRd";
        let stripped: String = wrapped.chars().filter(|c| !c.is_whitespace()).collect();
        assert!(Config::looks_like_base64(&stripped));
        
        // Invalid - too short
        assert!(!Config::looks_like_base64("ZGVm"));
        
        // Invalid - looks like code
        assert!(!Config::looks_like_base64("def transform(event): return [event]"));
        
        // Invalid - contains non-base64 characters
        assert!(!Config::looks_like_base64("ZGVmIHRyYW5zZm9ybShldmVudCk6!@#"));
        
        // Valid base64 with padding
        assert!(Config::looks_like_base64("ZGVmIHRyYW5zZm9ybShldmVudCk6CiAgICByZXR1cm4gW2V2ZW50XQ=="));
    }

    #[test]
    fn test_looks_like_base64_with_wrapped_output() {
        use base64::prelude::*;
        
        // Simulate wrapped base64 (newlines every 76 chars, as produced by default base64 command)
        let script = "def transform(event):\n    return [event]";
        let encoded = BASE64_STANDARD.encode(script);
        
        // Simulate base64 command wrapping at 76 characters
        let wrapped: String = encoded
            .chars()
            .collect::<Vec<_>>()
            .chunks(76)
            .map(|chunk| chunk.iter().collect::<String>())
            .collect::<Vec<_>>()
            .join("\n");
        
        // After stripping whitespace (as done in resolve_starlark_script), should be detected as base64
        let stripped: String = wrapped.chars().filter(|c| !c.is_whitespace()).collect();
        assert!(Config::looks_like_base64(&stripped));
        
        // Verify it can be decoded correctly
        let decoded = Config::decode_base64(&stripped).unwrap();
        assert_eq!(decoded, script);
    }
}
