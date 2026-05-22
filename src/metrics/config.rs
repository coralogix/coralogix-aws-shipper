use crate::custom_metadata::custom_metadata_from_env;
use cx_sdk_rest_logs::auth::ApiKey;
use std::collections::HashMap;
use std::env;
use std::string::String;
use std::time::Duration;

pub struct Config {
    pub api_key: ApiKey,
    pub endpoint: String,
    pub app_name: String,
    pub sub_name: String,
    pub retry_limit: usize,
    pub retry_delay: u64,
    pub batching_enabled: bool,
    // Maximum batch size in bytes for metrics payloads (encoded protobuf)
    pub batch_max_size_bytes: usize,
    /// When true, resolve AWS resource tags via Resource Groups Tagging API (Firehose metrics).
    pub tag_enrichment_enabled: bool,
    /// When true, tagging / discovery errors skip enrichment for affected namespaces (Go lambda default).
    pub continue_on_resource_failure: bool,
    pub file_cache_enabled: bool,
    pub file_cache_path: String,
    pub file_cache_expiration: Duration,
    /// Same format as logs: comma-separated `key=value` from `CUSTOM_METADATA`.
    pub custom_metadata: HashMap<String, String>,
}

fn env_default_true(var: &str) -> bool {
    match env::var(var) {
        Ok(v) => !matches!(v.to_lowercase().as_str(), "0" | "false" | "no"),
        Err(_) => true,
    }
}

impl Config {
    pub fn load_from_env() -> Result<Config, String> {
        let api_key = env::var("CORALOGIX_API_KEY")
            .map_err(|e| format!("CORALOGIX_API_KEY is not set: {}", e))?
            .into();
        let endpoint = env::var("CORALOGIX_ENDPOINT")
            .unwrap_or_else(|_| "https://ingress.private.coralogix.com".to_string());
        let app_name = env::var("APP_NAME").unwrap_or_else(|_| "aws".to_string());
        let sub_name = env::var("SUB_NAME").unwrap_or_else(|_| "metrics".to_string());
        let retry_limit: usize = env::var("RETRY_LIMIT")
            .unwrap_or_else(|_| "3".to_string())
            .parse()
            .unwrap_or(3);
        let retry_delay: u64 = env::var("RETRY_DELAY")
            .unwrap_or_else(|_| "5".to_string())
            .parse()
            .unwrap_or(5);

        let batching_enabled = env::var("BATCH_METRICS")
            .map(|v| matches!(v.to_lowercase().as_str(), "1" | "true" | "yes"))
            .unwrap_or(false);

        // Metrics batch max size (in MB); default 4MB similar to logs
        let batch_max_size_mb: usize = env::var("METRICS_BATCH_MAX_SIZE")
            .unwrap_or_else(|_| "4".to_string())
            .parse()
            .unwrap_or(4);
        let batch_max_size_bytes = batch_max_size_mb * 1024 * 1024;

        let tag_enrichment_enabled = match env::var("METRICS_TAG_ENRICHMENT_ENABLED") {
            Err(_) => true,
            Ok(v) => !matches!(v.to_lowercase().as_str(), "0" | "false" | "no"),
        };

        let continue_on_resource_failure = env_default_true("CONTINUE_ON_RESOURCE_FAILURE");

        let file_cache_enabled = env_default_true("FILE_CACHE_ENABLED");
        let file_cache_path = env::var("FILE_CACHE_PATH").unwrap_or_else(|_| "/tmp".to_string());

        let file_cache_expiration = match env::var("FILE_CACHE_EXPIRATION") {
            Ok(s) if !s.is_empty() => humantime::parse_duration(&s)
                .map_err(|e| format!("FILE_CACHE_EXPIRATION: {}", e))?,
            _ => Duration::from_secs(3600),
        };

        let custom_metadata = custom_metadata_from_env();

        Ok(Config {
            api_key,
            endpoint,
            app_name,
            sub_name,
            retry_limit,
            retry_delay,
            batching_enabled,
            batch_max_size_bytes,
            tag_enrichment_enabled,
            continue_on_resource_failure,
            file_cache_enabled,
            file_cache_path,
            file_cache_expiration,
            custom_metadata,
        })
    }
}

impl Clone for Config {
    fn clone(&self) -> Self {
        Config {
            api_key: self.api_key.clone(),
            endpoint: self.endpoint.clone(),
            app_name: self.app_name.clone(),
            sub_name: self.sub_name.clone(),
            retry_limit: self.retry_limit,
            retry_delay: self.retry_delay,
            batching_enabled: self.batching_enabled,
            batch_max_size_bytes: self.batch_max_size_bytes,
            tag_enrichment_enabled: self.tag_enrichment_enabled,
            continue_on_resource_failure: self.continue_on_resource_failure,
            file_cache_enabled: self.file_cache_enabled,
            file_cache_path: self.file_cache_path.clone(),
            file_cache_expiration: self.file_cache_expiration,
            custom_metadata: self.custom_metadata.clone(),
        }
    }
}
