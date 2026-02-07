use aws_lambda_events::cloudwatch_logs::AwsLogs;
use aws_lambda_events::cloudwatch_logs::LogData;
use aws_lambda_events::ecr_scan::EcrScanEvent;
use aws_lambda_events::encodings::Base64Data;
use aws_lambda_events::kafka::KafkaRecord;
use aws_sdk_ecr::Client as EcrClient;
use aws_sdk_cloudwatchlogs::Client as LogsClient;
use aws_sdk_s3::Client;
use base64::prelude::*;
use cx_sdk_rest_logs::DynLogExporter;
use fancy_regex::Regex;
use flate2::read::MultiGzDecoder;
use itertools::Itertools;
use lambda_runtime::Error;
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::env;
use std::ffi::OsStr;
use std::io::Read;
use std::ops::Range;
use std::path::Path;
use std::string::String;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};

use crate::logs::config::{Config, IntegrationType};
use crate::logs::coralogix;
use crate::logs::ecr;

// Type alias for the error type returned by list_tags_log_group
// Using Box<dyn Error> to handle the SDK error type which has private generic parameters
type LogGroupTagsError = Box<dyn std::error::Error + Send + Sync>;

// Lazy initialization with once_cell
static METADATA_EVALUATION_WITH_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"\{\{\s*(?<key>[a-z\.0-9_]+)\s*\|?\s*r'(?<regex>.*)'\s*\}\}"#)
        .expect("Failed to create regex")
});

static METADATA_EVALUATION_DEFAULT: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"\{\{\s*(?<key>[a-z\.0-9_]+)\s*\}\}"#).expect("Failed to create regex")
});

#[derive(Default)]
pub struct MetadataContext {
    inner: Arc<RwLock<HashMap<String, Option<String>>>>,
}

impl MetadataContext {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn insert(&self, key: String, value: Option<String>) {
        let mut inner = self.inner.write().unwrap();
        inner.insert(key, value);
    }

    pub fn get(&self, key: &str) -> Option<String> {
        let inner = self.inner.read().unwrap();
        if let Some(v) = inner.get(key).cloned() {
            v
        } else {
            None
        }
    }

    /// evalueate dynamic metadata values for application_name and subsystem_name values.
    /// values are expected to be in the format of<br>
    /// `{{key}}` or `{{key|regex}}`, the regex must include a capture group which will be
    /// used to extract the value. The key is the key in the metadata context map.
    pub fn evaluate(&self, value: String) -> Result<String, String> {
        if !value.starts_with("{{") && !value.ends_with("}}") {
            return Ok(value);
        };

        let (reg, key) = if value.contains("|") {
            let captures = METADATA_EVALUATION_WITH_REGEX
                .captures(&value)
                .map_err(|e| format!("capture error: {}", e))?;
            let captures = captures.ok_or("no captures found")?;

            let key = captures
                .name("key")
                .ok_or("key not found")?
                .as_str()
                .to_string();
            let reg = captures
                .name("regex")
                .ok_or("regex not found")?
                .as_str()
                .to_string();
            (reg, key)
        } else {
            let captures = METADATA_EVALUATION_DEFAULT
                .captures(&value)
                .map_err(|e| format!("capture error: {}", e))?;
            let captures = captures.ok_or("no captures found")?;
            (
                "".to_string(),
                captures
                    .name("key")
                    .ok_or("key not found")?
                    .as_str()
                    .to_string(),
            )
        };

        if let Some(v) = self.get(&key) {
            if reg.is_empty() {
                return Ok(v);
            }

            // apply reg
            let re = Regex::new(reg.as_str())
                .map_err(|e| format!("invalid regex for dynamic metadata: {}", e))?;
            let captures = re
                .captures(&v)
                .map_err(|e| format!("regex capture group error: {}", e))?;

            // If regex doesn't match, fall back to the raw metadata value
            if let Some(captures) = captures {
                // Try capture groups 1, 2, 3... to handle alternation patterns
                // In alternation like (A)|(B), group 1 is A and group 2 is B
                for i in 1..=captures.len().saturating_sub(1) {
                    if let Some(group) = captures.get(i) {
                        let captured = group.as_str();
                        if !captured.is_empty() {
                            return Ok(captured.to_string());
                        }
                    }
                }
                // If all capture groups are empty, fall back to raw value
                tracing::warn!("Regex '{}' matched but all capture groups were empty for value '{}', falling back to raw value", reg, v);
                return Ok(v);
            } else {
                tracing::warn!(
                    "Regex '{}' did not match value '{}', falling back to raw value",
                    reg,
                    v
                );
                return Ok(v);
            }
        };

        Err(format!("metadata key '{}' not found", key))
    }
}

#[derive(Clone)]
struct TagCacheEntry {
    tags: HashMap<String, String>,
    fetched_at: Instant,
}

// Static cache for log group tags, shared across Lambda invocations
static LOG_GROUP_TAGS_CACHE: Lazy<Mutex<HashMap<String, TagCacheEntry>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

/// Fetches tags for a CloudWatch Log Group, using a static cache to avoid repeated API calls.
/// 
/// # Returns
/// - `Ok(HashMap<String, String>)` on successful fetch (from cache or API), even if the log group has no tags
/// - `Err(Error)` on API failures - errors are NOT cached and allow immediate retry on next invocation
/// 
/// # Caching Behavior
/// - Successful fetches (even with empty tags) are cached with the specified TTL
/// - Errors are NOT cached, allowing immediate retry on the next Lambda invocation
/// - Cache is bypassed entirely if `cache_ttl` is zero
async fn fetch_log_group_tags(
    logs_client: &LogsClient,
    log_group_name: &str,
    cache_ttl: Duration,
) -> Result<HashMap<String, String>, LogGroupTagsError> {
    let use_cache = !cache_ttl.is_zero();

    debug!(
        "fetch_log_group_tags called for log group: {}, cache_ttl: {:?}, cache_enabled: {}",
        log_group_name, cache_ttl, use_cache
    );

    // If TTL is zero, bypass cache entirely
    if !use_cache {
        debug!(
            "Log group tags cache TTL is zero ({} seconds), bypassing cache for log group: {}",
            cache_ttl.as_secs(),
            log_group_name
        );
        return fetch_log_group_tags_from_api(logs_client, log_group_name).await;
    }

    let now = Instant::now();

    // Check cache first
    let cached_tags = {
        let mut cache = LOG_GROUP_TAGS_CACHE.lock().unwrap();
        let cache_size = cache.len();
        debug!(
            "Checking cache for log group: {} (cache currently has {} entries)",
            log_group_name, cache_size
        );

        if let Some(cached_entry) = cache.get(log_group_name) {
            let age = now.duration_since(cached_entry.fetched_at);
            let time_remaining = cache_ttl.saturating_sub(age);
            let is_expired = age >= cache_ttl;

            debug!(
                "Cache entry found for log group: {}, age: {:?}, ttl: {:?}, time_remaining: {:?}, expired: {}",
                log_group_name, age, cache_ttl, time_remaining, is_expired
            );

            if !is_expired {
                debug!(
                    "CACHE HIT: Using cached tags for log group: {} (age: {:?}, remaining: {:?}, tags_count: {})",
                    log_group_name,
                    age,
                    time_remaining,
                    cached_entry.tags.len()
                );
                Some(cached_entry.tags.clone())
            } else {
                debug!(
                    "CACHE EXPIRED: Removing expired entry for log group: {} (age: {:?} >= ttl: {:?})",
                    log_group_name, age, cache_ttl
                );
                cache.remove(log_group_name);
                None
            }
        } else {
            debug!(
                "CACHE MISS: No cache entry found for log group: {}",
                log_group_name
            );
            None
        }
    };

    // If we have cached tags, return them
    if let Some(tags) = cached_tags {
        return Ok(tags);
    }

    // Cache miss or expired - fetch from API
    debug!(
        "CACHE MISS: Fetching tags from API for log group: {}",
        log_group_name
    );
    let fetch_start = Instant::now();
    let tags_result = fetch_log_group_tags_from_api(logs_client, log_group_name).await;
    let fetch_duration = fetch_start.elapsed();

    match tags_result {
        Ok(tags) => {
            debug!(
                "Fetched {} tags from API for log group: {} in {:?}",
                tags.len(),
                log_group_name,
                fetch_duration
            );

            // Only cache successful results
            {
                let mut cache = LOG_GROUP_TAGS_CACHE.lock().unwrap();
                let cache_size_before = cache.len();
                cache.insert(
                    log_group_name.to_string(),
                    TagCacheEntry {
                        tags: tags.clone(),
                        fetched_at: Instant::now(),
                    },
                );
                let cache_size_after = cache.len();
                debug!(
                    "CACHE UPDATE: Stored {} tags for log group: {} in cache (cache size: {} -> {})",
                    tags.len(),
                    log_group_name,
                    cache_size_before,
                    cache_size_after
                );
            }

            Ok(tags)
        }
        Err(e) => {
            debug!(
                "API call failed for log group: {} in {:?}. Error will NOT be cached, allowing immediate retry on next invocation.",
                log_group_name,
                fetch_duration
            );
            Err(e)
        }
    }
}

/// Fetches tags for a CloudWatch Log Group from the AWS API.
/// 
/// # Returns
/// - `Ok(HashMap<String, String>)` on successful API call, even if the log group has no tags (empty HashMap)
/// - `Err(SdkError)` on API failures (throttling, IAM errors, network issues, etc.)
/// 
/// # Errors
/// This function will return an error if the API call fails for any reason.
/// Errors are NOT cached and will allow immediate retry on the next invocation.
async fn fetch_log_group_tags_from_api(
    logs_client: &LogsClient,
    log_group_name: &str,
) -> Result<HashMap<String, String>, LogGroupTagsError> {
    debug!("Fetching tags for log group: {}", log_group_name);
    let tags_result = logs_client
        .list_tags_log_group()
        .log_group_name(log_group_name)
        .send()
        .await;

    match tags_result {
        Ok(response) => {
            let mut tags = HashMap::new();
            if let Some(tags_map) = response.tags() {
                debug!("Tags map from API ({} tags): {:?}", tags_map.len(), tags_map);
                // Iterate through ALL tags - ensure we get all of them
                for (key, value) in tags_map.iter() {
                    debug!("Adding tag: {} = {}", key, value);
                    tags.insert(key.to_string(), value.to_string());
                }
                // Verify we got all tags
                if tags.len() != tags_map.len() {
                    warn!(
                        "Tag count mismatch: expected {} tags, got {} tags for log group: {}",
                        tags_map.len(),
                        tags.len(),
                        log_group_name
                    );
                }
            } else {
                debug!("No tags found in response for log group: {}", log_group_name);
            }

            debug!(
                "Fetched {} tags for log group: {} - Tags: {:?}",
                tags.len(),
                log_group_name,
                tags
            );
            Ok(tags)
        }
        Err(e) => {
            warn!(
                "Failed to fetch tags for log group {}: {:?}",
                log_group_name, e
            );
            Err(Box::new(e) as LogGroupTagsError)
        }
    }
}

pub async fn s3(
    mctx: &MetadataContext,
    s3_client: &Client,
    coralogix_exporter: DynLogExporter,
    config: &Config,
    aws_config: &aws_config::SdkConfig,
    bucket: String,
    key: String,
) -> Result<(), Error> {
    let defined_app_name = config
        .app_name
        .clone()
        .unwrap_or_else(|| bucket.to_string());
    let defined_sub_name = config
        .sub_name
        .clone()
        .unwrap_or_else(|| bucket.to_string());

    let key_str = key.clone();
    let key_path = Path::new(&key_str);
    mctx.insert("s3.bucket".to_string(), Some(bucket.clone()));
    mctx.insert("s3.object.key".to_string(), Some(key.clone()));

    let batches = match config.integration_type {
        // VPC Flow Logs Needs Prefix and Sufix to be exact AWSLogs/ and .log.gz
        IntegrationType::VpcFlow => {
            let raw_data = get_bytes_from_s3(s3_client, bucket.clone(), key.clone()).await?;

            process_vpcflows(raw_data, config.sampling, &config.blocking_pattern, key).await?
        }
        IntegrationType::S3Csv => {
            let raw_data = get_bytes_from_s3(s3_client, bucket.clone(), key.clone()).await?;
            process_csv(
                raw_data,
                config.sampling,
                key_path,
                &config.csv_delimiter,
                &config.blocking_pattern,
                key,
            )
            .await?
        }
        IntegrationType::CloudFront => {
            let raw_data = get_bytes_from_s3(s3_client, bucket.clone(), key.clone()).await?;
            let cloudfront_delimiter: &str = "\t";
            process_csv(
                raw_data,
                config.sampling,
                key_path,
                cloudfront_delimiter,
                &config.blocking_pattern,
                key,
            )
            .await?
        }
        IntegrationType::S3 => {
            let raw_data = get_bytes_from_s3(s3_client, bucket.clone(), key.clone()).await?;
            process_s3(
                raw_data,
                config.sampling,
                key_path,
                &config.newline_pattern,
                &config.blocking_pattern,
                key,
            )
            .await?
        }
        IntegrationType::CloudTrail => {
            let raw_data = get_bytes_from_s3(s3_client, bucket.clone(), key.clone()).await?;
            process_cloudtrail(raw_data, config.sampling, &config.blocking_pattern, key).await?
        }
        _ => {
            tracing::warn!(
                "Received event doesn't match the configured integration type: {:?}",
                config.integration_type
            );
            Vec::new()
        }
    };

    coralogix::process_batches(
        batches,
        &defined_app_name,
        &defined_sub_name,
        config,
        &mctx,
        coralogix_exporter,
        aws_config,
    )
    .await?;

    Ok(())
}

pub async fn kinesis_logs(
    mctx: &MetadataContext,
    kinesis_message: Base64Data,
    coralogix_exporter: DynLogExporter,
    config: &Config,
    aws_config: &aws_config::SdkConfig,
) -> Result<(), Error> {
    let defined_app_name = config
        .app_name
        .clone()
        .unwrap_or_else(|| "NO APPLICATION NAME".to_string());
    let defined_sub_name = config
        .sub_name
        .clone()
        .unwrap_or_else(|| "NO SUBSYSTEM NAME".to_string());
    let v = kinesis_message.0;

    let decompressed_data = match gunzip(v.clone(), String::new()) {
        Ok(data) => data,
        Err(_) => {
            tracing::debug!("Data does not appear to be valid gzip format. Treating as UTF-8");
            v // set decompressed_data to the original data if decompression fails
        }
    };

    let decoded_data = match String::from_utf8(decompressed_data) {
        Ok(s) => s,
        Err(error) => {
            tracing::debug!(?error, "Failed to decode data");
            String::new()
        }
    };

    // String::from_utf8(decompressed_data.clone())?;
    let batches = match serde_json::from_str(&decoded_data) {
        Ok(logs) => {
            process_cloudwatch_logs(&mctx, logs, config.sampling, &config.blocking_pattern).await?
        }
        Err(_) => {
            tracing::debug!("Failed to decode data");
            if decoded_data.is_empty() {
                Vec::new()
            } else {
                vec![decoded_data]
            }
        }
    };

    coralogix::process_batches(
        batches,
        &defined_app_name,
        &defined_sub_name,
        config,
        &mctx,
        coralogix_exporter,
        aws_config,
    )
    .await?;
    Ok(())
}

pub async fn sqs_logs(
    mctx: &MetadataContext,
    sqs_message: String,
    coralogix_exporter: DynLogExporter,
    config: &Config,
    aws_config: &aws_config::SdkConfig,
) -> Result<(), Error> {
    let defined_app_name = config
        .app_name
        .clone()
        .unwrap_or_else(|| "NO APPLICATION NAME".to_string());
    let defined_sub_name = config
        .sub_name
        .clone()
        .unwrap_or_else(|| "NO SUBSYSTEM NAME".to_string());
    let mut batches = Vec::new();
    tracing::debug!("SQS Message: {:?}", sqs_message);
    batches.push(sqs_message.clone());
    coralogix::process_batches(
        batches,
        &defined_app_name,
        &defined_sub_name,
        config,
        &mctx,
        coralogix_exporter,
        aws_config,
    )
    .await?;
    Ok(())
}
pub async fn sns_logs(
    mctx: &MetadataContext,
    sns_message: String,
    coralogix_exporter: DynLogExporter,
    config: &Config,
    aws_config: &aws_config::SdkConfig,
) -> Result<(), Error> {
    let defined_app_name = config
        .app_name
        .clone()
        .unwrap_or_else(|| "NO APPLICATION NAME".to_string());
    let defined_sub_name = config
        .sub_name
        .clone()
        .unwrap_or_else(|| "NO SUBSYSTEM NAME".to_string());
    let mut batches = Vec::new();
    tracing::debug!("SNS Message: {:?}", sns_message);
    batches.push(sns_message.clone());
    coralogix::process_batches(
        batches,
        &defined_app_name,
        &defined_sub_name,
        config,
        &mctx,
        coralogix_exporter,
        aws_config,
    )
    .await?;
    Ok(())
}
pub async fn cloudwatch_logs(
    mctx: &MetadataContext,
    cloudwatch_event_log: AwsLogs,
    coralogix_exporter: DynLogExporter,
    config: &Config,
    logs_client: &LogsClient,
    aws_config: &aws_config::SdkConfig,
) -> Result<(), Error> {
    let defined_app_name = config
        .app_name
        .clone()
        .unwrap_or_else(|| "NO APPLICATION NAME".to_string());
    let defined_sub_name = if let Some(sub_name) = &config.sub_name {
        if sub_name.is_empty() {
            cloudwatch_event_log.data.log_group.to_string()
        } else {
            sub_name.clone()
        }
    } else {
        cloudwatch_event_log.data.log_group.to_string()
    };

    // Fetch log group tags if enabled
    if config.enable_log_group_tags {
        debug!(
            "CloudWatch Log Group tags feature enabled. Log group: {}, cache_ttl_seconds: {}",
            cloudwatch_event_log.data.log_group,
            config.log_group_tags_cache_ttl_seconds
        );
        let cache_ttl = Duration::from_secs(config.log_group_tags_cache_ttl_seconds);
        debug!(
            "Calling fetch_log_group_tags for log group: {} with cache_ttl: {:?} ({} seconds)",
            cloudwatch_event_log.data.log_group,
            cache_ttl,
            config.log_group_tags_cache_ttl_seconds
        );
        let tags_result = fetch_log_group_tags(
            logs_client,
            &cloudwatch_event_log.data.log_group,
            cache_ttl,
        )
        .await;
        
        match tags_result {
            Ok(tags) => {
                debug!(
                    "Received {} tags for log group: {} - Tags: {:?}",
                    tags.len(),
                    cloudwatch_event_log.data.log_group,
                    tags
                );
                
                // Serialize tags to JSON string and store in MetadataContext
                if !tags.is_empty() {
                    match serde_json::to_string(&tags) {
                        Ok(tags_json) => {
                            debug!(
                                "Storing {} tags in metadata context for log group: {} (JSON length: {} bytes)",
                                tags.len(),
                                cloudwatch_event_log.data.log_group,
                                tags_json.len()
                            );
                            mctx.insert("cw.tags".to_string(), Some(tags_json));
                            debug!("Successfully stored tags in metadata context as cw.tags");
                        }
                        Err(e) => {
                            warn!("Failed to serialize log group tags to JSON: {:?}", e);
                        }
                    }
                } else {
                    debug!(
                        "No tags to store for log group: {} (tags map is empty)",
                        cloudwatch_event_log.data.log_group
                    );
                }
            }
            Err(e) => {
                warn!(
                    "Failed to fetch tags for log group {}: {:?}. Continuing without tags.",
                    cloudwatch_event_log.data.log_group,
                    e
                );
                // Error is not cached, so next invocation will retry
            }
        }
    }

    let logs = match config.integration_type {
        IntegrationType::CloudWatch => {
            process_cloudwatch_logs(
                &mctx,
                cloudwatch_event_log.data,
                config.sampling,
                &config.blocking_pattern,
            )
            .await?
        }
        _ => {
            tracing::warn!(
                "Received event doesn't match the configured integration type: {:?}",
                config.integration_type
            );
            Vec::new()
        }
    };

    coralogix::process_batches(
        logs,
        &defined_app_name,
        &defined_sub_name,
        config,
        &mctx,
        coralogix_exporter,
        aws_config,
    )
    .await?;

    Ok(())
}

pub async fn ecr_scan_logs(
    mctx: &MetadataContext,
    ecr_client: &EcrClient,
    ecr_scan_event: EcrScanEvent,
    coralogix_exporter: DynLogExporter,
    config: &Config,
    aws_config: &aws_config::SdkConfig,
) -> Result<(), Error> {
    let defined_app_name = config
        .app_name
        .clone()
        .unwrap_or_else(|| "NO APPLICATION NAME".to_string());
    let defined_sub_name = config
        .sub_name
        .clone()
        .unwrap_or_else(|| "NO SUBSYSTEM NAME".to_string());
    //let mut batches = Vec::new();

    tracing::debug!("ECR Scan Event: {:?}", ecr_scan_event);
    let payload = ecr::process_ecr_scan_event(ecr_scan_event, config, &ecr_client).await?;
    coralogix::process_batches(
        payload,
        &defined_app_name,
        &defined_sub_name,
        config,
        &mctx,
        coralogix_exporter,
        aws_config,
    )
    .await?;
    Ok(())
}

pub async fn get_bytes_from_s3(
    s3_client: &Client,
    bucket: String,
    key: String,
) -> Result<Vec<u8>, Error> {
    let start_time = Instant::now();
    let request = s3_client
        .get_object()
        .bucket(bucket.clone())
        .key(key.clone())
        .response_content_type("application/json");
    let response = request.send().await?;
    tracing::info!(
        "Received response from S3 in {}ms",
        start_time.elapsed().as_millis()
    );

    // Downloading the content this way is faster and allocates less memory than using `body.collect().await?.to_vec()`
    let mut data = Vec::with_capacity(response.content_length.unwrap_or(1024 * 1024) as usize);
    let mut body = response.body;
    while let Some(result) = body.next().await {
        let bytes = result?;
        data.extend_from_slice(&bytes[..])
    }

    tracing::info!(
        "Downloaded file from S3 in {}ms. Actual size: {} bytes. Name of the file: {}",
        start_time.elapsed().as_millis(),
        data.len(),
        key
    );

    Ok(data)
}

async fn process_cloudwatch_logs(
    metadata: &MetadataContext,
    cw_event: LogData,
    sampling: usize,
    blocking_pattern: &str,
) -> Result<Vec<String>, Error> {
    // Add CW metadata
    metadata.insert("cw.log.group".to_string(), Some(cw_event.log_group.clone()));
    metadata.insert(
        "cw.log.stream".to_string(),
        Some(cw_event.log_stream.clone()),
    );
    metadata.insert("cw.owner".to_string(), Some(cw_event.owner.clone()));

    let log_entries: Vec<String> = cw_event
        .log_events
        .into_iter()
        .map(|entry| entry.message)
        .collect();
    info!("Received {} CloudWatch logs", log_entries.len());
    debug!("Cloudwatch Logs: {:?}", log_entries);
    //Ok(sample(sampling, log_entries))
    let re_block: Regex = Regex::new(blocking_pattern)?;
    info!("Blocking Pattern: {:?}", blocking_pattern);
    Ok(sample(
        sampling,
        block(re_block, log_entries, blocking_pattern)?,
    ))
}
async fn process_vpcflows(
    raw_data: Vec<u8>,
    sampling: usize,
    blocking_pattern: &str,
    key: String,
) -> Result<Vec<String>, Error> {
    info!("VPC Flow Integration Type");
    let v = gunzip(raw_data, key)?;
    let s = String::from_utf8(v)?;
    let array_s = split(Regex::new(r"\n")?, s.as_str())?;
    let flow_header = split(Regex::new(r"\s+")?, array_s[0])?;
    let csv_delimiter = " ";
    let records: Vec<&str> = array_s
        .iter()
        .skip(1)
        .filter(|&line| !line.trim().is_empty())
        .copied()
        .collect_vec();
    let parsed_records = parse_records(&flow_header, &records, csv_delimiter)?;
    let re_block: Regex = Regex::new(blocking_pattern)?;
    tracing::debug!("Parsed Records: {:?}", &parsed_records);
    Ok(sample(
        sampling,
        block(re_block, parsed_records, blocking_pattern)?,
    ))
}

async fn process_csv(
    raw_data: Vec<u8>,
    sampling: usize,
    key_path: &Path,
    mut csv_delimiter: &str,
    blocking_pattern: &str,
    key: String,
) -> Result<Vec<String>, Error> {
    info!("CSV Integration Type");
    if csv_delimiter == "\\t" {
        debug!("Replacing \\t with \t");
        csv_delimiter = "\t";
    }
    let s = if key_path.extension() == Some(OsStr::new("gz")) {
        let v = gunzip(raw_data, key)?;
        let s = String::from_utf8(v)?;
        debug!("ZIP S3 object: {}", s);
        s
    } else {
        let s = String::from_utf8(raw_data)?;
        debug!("NON-ZIP S3 object: {}", s);
        s
    };
    let custom_header = env::var("CUSTOM_CSV_HEADER").unwrap_or("".to_string());
    let mut flow_header: Vec<&str>;
    let array_s = split(Regex::new(r"\n")?, s.as_str())?;
    let records: Vec<&str> = if array_s[0].starts_with("#Version") {
        debug!("Array 0: {:?}", &array_s[0]);
        flow_header = array_s[1].split(' ').collect_vec();
        flow_header.remove(0);
        tracing::debug!("Flow Header: {:?}", &flow_header);
        array_s
            .iter()
            .skip(2)
            .filter(|&line| !line.trim().is_empty())
            .copied()
            .collect_vec()
    } else {
        if custom_header.len() > 0 {
            flow_header = custom_header.split(csv_delimiter).collect_vec();
        } else {
            flow_header = array_s[0].split(csv_delimiter).collect_vec();
        }
        tracing::debug!("Flow Header: {:?}", &flow_header);
        array_s
            .iter()
            .skip(1)
            .filter(|&line| !line.trim().is_empty())
            .copied()
            .collect_vec()
    };
    let re_block: Regex = Regex::new(blocking_pattern)?;
    let parsed_records = parse_records(&flow_header, &records, csv_delimiter)?;
    debug!("Parsed Records: {:?}", &parsed_records);
    Ok(sample(
        sampling,
        block(re_block, parsed_records, blocking_pattern)?,
    ))
}

async fn process_s3(
    raw_data: Vec<u8>,
    sampling: usize,
    key_path: &Path,
    newline_pattern: &str,
    blocking_pattern: &str,
    key: String,
) -> Result<Vec<String>, Error> {
    let s = if key_path.extension() == Some(OsStr::new("gz")) {
        let v = gunzip(raw_data, key)?;
        let s = String::from_utf8(v)?;
        debug!("ZIP S3 object: {}", s);
        s
    } else {
        let s = String::from_utf8(raw_data)?;
        debug!("NON-ZIP S3 object: {}", s);
        s
    };
    let pattern = if newline_pattern.is_empty() {
        debug!("Newline pattern not set, using default");
        r"\n"
    } else {
        newline_pattern
    };
    let re = Regex::new(pattern)?;
    let re_block: Regex = Regex::new(blocking_pattern)?;
    debug!("OUT S3 object: {}", s);
    let logs: Vec<String> = sample(
        sampling,
        block(
            re_block,
            split(re, &s)?
                .into_iter()
                .map(|line| line.to_owned())
                .collect_vec(),
            blocking_pattern,
        )?, // TODO Can we avoid copying?
    );

    Ok(logs)
}

async fn process_cloudtrail(
    raw_data: Vec<u8>,
    sampling: usize,
    blocking_pattern: &str,
    key: String,
) -> Result<Vec<String>, Error> {
    tracing::info!("Cloudtrail Integration Type");
    let v = gunzip(raw_data, key)?;
    let s = String::from_utf8(v)?;
    let mut logs_vec: Vec<String> = Vec::new();
    let array_s = serde_json::from_str::<serde_json::Value>(&s)?;
    tracing::debug!(
        "Array: {}",
        serde_json::to_string(&array_s).unwrap_or("".to_string())
    );
    let re_block: Regex = Regex::new(blocking_pattern)?;
    if let Some(records) = array_s.get("Records") {
        if let Some(records_array) = records.as_array() {
            for log in records_array.iter().map(|log| log.to_string()) {
                tracing::debug!("Logs: {}", log);
                logs_vec.push(log.to_string());
            }
            tracing::debug!("OUT S3 object: {}", s);
            let logs: Vec<String> = sample(sampling, block(re_block, logs_vec, blocking_pattern)?);

            Ok(logs)
        } else {
            Ok(Vec::new())
        }
    } else {
        tracing::warn!("Non Cloudtrail Log Ingested - Skipping - {}", s);
        Ok(Vec::new())
    }
}

pub async fn kafka_logs(
    mctx: &MetadataContext,
    records: Vec<KafkaRecord>,
    coralogix_exporter: DynLogExporter,
    config: &Config,
    aws_config: &aws_config::SdkConfig,
) -> Result<(), Error> {
    let defined_app_name = config
        .app_name
        .clone()
        .unwrap_or_else(|| "NO APPLICATION NAME".to_string());

    let defined_sub_name = config
        .sub_name
        .clone()
        .unwrap_or_else(|| "NO SUBSYSTEM NAME".to_string());

    let mut batch = Vec::new();
    for record in records {
        if let Some(value) = record.value {
            mctx.insert("kafka.topic".to_string(), record.topic.clone());
            // check if value is base64 encoded
            if let Ok(message) = BASE64_STANDARD.decode(&value) {
                batch.push(String::from_utf8(message)?);
            } else {
                batch.push(value);
            }
        }
    }

    coralogix::process_batches(
        batch,
        &defined_app_name,
        &defined_sub_name,
        config,
        &mctx,
        coralogix_exporter,
        aws_config,
    )
    .await?;

    Ok(())
}

fn gunzip(compressed_data: Vec<u8>, _: String) -> Result<Vec<u8>, Error> {
    if compressed_data.is_empty() {
        tracing::warn!("Input data is empty, cannot ungzip a zero-byte file.");
        return Ok(Vec::new());
    }
    let mut decoder = MultiGzDecoder::new(&compressed_data[..]);

    let mut output = Vec::new();
    let mut chunk = [0; 8192];
    loop {
        match decoder.read(&mut chunk) {
            Ok(bytes_read) => {
                if bytes_read == 0 {
                    break;
                }
                output.extend_from_slice(&chunk[..bytes_read]);
            }
            Err(err) => {
                tracing::warn!(
                    ?err,
                    "Problem decompressing data after {} bytes",
                    output.len()
                );
                break;
            }
        }
    }
    if output.is_empty() {
        tracing::warn!("Uncompressed failed. zero-file result");
        return Err(Error::from("Uncompressed Failed, zero-file result"));
    }
    Ok(output)
}

fn parse_records(
    flow_header: &[&str],
    records: &[&str],
    csv_delimiter: &str,
) -> Result<Vec<String>, String> {
    records
        .iter()
        .map(|record| {
            let values: Vec<&str> = record.split(csv_delimiter).collect();
            let mut parsed_log = serde_json::Map::new();
            debug!("DELIMITER: {:?}", csv_delimiter);
            for (index, field) in flow_header.iter().enumerate() {
                // Use a default empty string if the value is missing
                let value = values.get(index).unwrap_or(&"");

                let parsed_value = if let Ok(num) = value.parse::<i32>() {
                    serde_json::Value::Number(serde_json::Number::from(num))
                } else {
                    serde_json::Value::String(value.to_string())
                };
                debug!("Parsed Value: {:?}", parsed_value);
                parsed_log.insert(field.to_string(), parsed_value);
            }

            serde_json::to_string(&parsed_log).map_err(|e| e.to_string())
        })
        .collect()
}

fn split(re: Regex, string: &str) -> Result<Vec<&str>, Error> {
    let mut logs: Vec<&str> = Vec::new();
    let mut next_log_start: usize = 0;

    for m in re.find_iter(string) {
        let Range { start, end } = m?.range();
        if start != 0 {
            // this will skip creating an empty log at the beginning
            logs.push(&string[next_log_start..start]);
        }
        next_log_start = end;
    }
    logs.push(&string[next_log_start..]);

    Ok(logs)
}

fn block(re_block: Regex, v: Vec<String>, b: &str) -> Result<Vec<String>, Error> {
    if b.is_empty() {
        return Ok(v);
    }
    let mut logs: Vec<String> = Vec::new();
    for l in v {
        if re_block.is_match(&l)? {
            tracing::warn!("Blocking pattern matched");
            tracing::debug!("Log Matched: {}", l)
        } else {
            tracing::debug!("Log Not Matched: {}", l);
            logs.push(l);
        }
    }
    Ok(logs)
}

fn sample(sampling: usize, v: Vec<String>) -> Vec<String> {
    v.into_iter().step_by(sampling).collect_vec()
}

#[cfg(test)]
mod test {
    use fancy_regex::Regex;
    use itertools::Itertools;

    use crate::logs::process::{block, sample, split};

    #[test]
    fn test_sampling() {
        let log_file_contents = r#"09-24 16:09:07.042: ERROR1 System.out(4844): java.lang.NullPointerException
at com.temp.ttscancel.MainActivity.onCreate(MainActivity.java:43)
at android.app.ActivityThread$H.handleMessage(ActivityThread.java:1210)
09-24 16:09:07.042: ERROR2 System.out(4844): java.lang.NullPointerException
at com.temp.ttscancel.MainActivity.onCreate(MainActivity.java:43)
at android.app.ActivityThread$H.handleMessage(ActivityThread.java:1210)
09-24 16:09:07.042: ERROR3 System.out(4844): java.lang.NullPointerException
at com.temp.ttscancel.MainActivity.onCreate(MainActivity.java:43)
at android.app.ActivityThread$H.handleMessage(ActivityThread.java:1210)"#;

        let regex = Regex::new(r#"\n(?=\d{2}\-\d{2}\s\d{2}\:\d{2}\:\d{2}\.\d{3})"#).unwrap();
        let regex_block: Regex = Regex::new(r#"ERROR3"#).unwrap();
        let blocking_pattern = "ERROR3";
        let sampling = 1;
        let logs = sample(
            sampling,
            block(
                regex_block,
                split(regex, log_file_contents)
                    .unwrap()
                    .into_iter()
                    .map(|l| l.to_owned())
                    .collect_vec(),
                blocking_pattern,
            )
            .unwrap(),
        );
        tracing::info!("{:?}", logs);
        assert_eq!(logs.len(), 2);
        assert_eq!(
            logs[0],
            r#"09-24 16:09:07.042: ERROR1 System.out(4844): java.lang.NullPointerException
at com.temp.ttscancel.MainActivity.onCreate(MainActivity.java:43)
at android.app.ActivityThread$H.handleMessage(ActivityThread.java:1210)"#
        );
        //    assert_eq!(logs[1], r#"09-24 16:09:07.042: ERROR2 System.out(4844): java.lang.NullPointerException
        //at com.temp.ttscancel.MainActivity.onCreate(MainActivity.java:43)
        //at android.app.ActivityThread$H.handleMessage(ActivityThread.java:1210)"#);
    }

    #[test]
    fn test_metadata_context_and_evaluation() {
        let metadata = super::MetadataContext::new();
        metadata.insert("key1".to_string(), Some("hello".to_string()));
        metadata.insert("key2".to_string(), Some("world".to_string()));
        metadata.insert("key_with_underscore".to_string(), Some("devs".to_string()));
        assert_eq!(metadata.get("key1").unwrap(), "hello");
        assert_eq!(metadata.get("key2").unwrap(), "world");
        assert_eq!(metadata.get("key3"), None);

        // evaluate
        let r = metadata.evaluate("{{key1}}".to_string()).unwrap();
        assert_eq!(r, "hello");

        let r = metadata
            .evaluate(r#"{{key2|r'^(\w).*'}}"#.to_string())
            .unwrap();
        assert_eq!(r, "w");

        // with underscore
        let r = metadata
            .evaluate("{{key_with_underscore}}".to_string())
            .unwrap();
        assert_eq!(r, "devs");

        // with spaces
        let r = metadata
            .evaluate(r#"{{ key2 | r'^(\w).*' }}"#.to_string())
            .unwrap();
        assert_eq!(r, "w");

        // invalid regex
        let r = metadata.evaluate(r#"{{ key2 | r'^(\w' }}"#.to_string());
        assert!(r.is_err());
    }

    #[test]
    fn test_metadata_context_regex_fallback() {
        let metadata = super::MetadataContext::new();

        // Test case: ECS log group that should match the first part of the regex
        metadata.insert(
            "cw.log.group".to_string(),
            Some("/ecs/my-service-prod_logs_abc123".to_string()),
        );

        // Customer's regex pattern - should extract "my-service" from the ECS pattern
        let customer_regex =
            r#"{{ cw.log.group | r'^/ecs/([a-z0-9-]+?)(?:-[a-z]+)?_logs_[a-z0-9]+$|^(.*)$' }}"#;
        let result = metadata.evaluate(customer_regex.to_string()).unwrap();
        assert_eq!(result, "my-service");

        // Test case: Non-ECS log group that should fall back to the second part of the regex
        metadata.insert(
            "cw.log.group".to_string(),
            Some("/some/other/log-group".to_string()),
        );
        let result = metadata.evaluate(customer_regex.to_string()).unwrap();
        assert_eq!(result, "/some/other/log-group");

        // Test case: Regex that doesn't match anything - should fall back to raw value
        metadata.insert(
            "cw.log.group".to_string(),
            Some("test-log-group".to_string()),
        );
        let non_matching_regex = r#"{{ cw.log.group | r'^/this/will/never/match/([a-z]+)$' }}"#;
        let result = metadata.evaluate(non_matching_regex.to_string()).unwrap();
        assert_eq!(result, "test-log-group"); // Should fall back to raw value

        // Test case: Missing metadata key
        let missing_key_regex = r#"{{ nonexistent.key | r'^(.*)$' }}"#;
        let result = metadata.evaluate(missing_key_regex.to_string());
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .contains("metadata key 'nonexistent.key' not found"));
    }
}
