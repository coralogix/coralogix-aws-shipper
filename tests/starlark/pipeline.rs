//! Pipeline integration tests: script loading (S3/HTTP/base64), caching, and fail-open behavior.

use aws_config::{BehaviorVersion, SdkConfig};
use aws_sdk_s3::config::{Credentials as S3Credentials, SharedCredentialsProvider};
use aws_sdk_s3::config::Region as S3Region;
use aws_smithy_runtime::client::http::test_util::ReplayEvent;
use aws_smithy_runtime::client::http::test_util::StaticReplayClient;
use aws_smithy_types::body::SdkBody;
use base64::Engine;
use coralogix_aws_shipper::logs::config::{Config, IntegrationType, ScriptLoadError};
use coralogix_aws_shipper::logs::transform::{reset_cache, transform_logs};
use cx_sdk_rest_logs::auth::ApiKey;
use http::Response;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn create_test_config(starlark_script: Option<String>) -> Config {
    Config {
        newline_pattern: "".to_string(),
        blocking_pattern: "".to_string(),
        sampling: 1,
        logs_per_batch: 500,
        integration_type: IntegrationType::S3,
        app_name: None,
        sub_name: None,
        api_key: ApiKey::from("test-api-key"),
        endpoint: "https://test.coralogix.com".to_string(),
        max_elapsed_time: 250,
        csv_delimiter: ",".to_string(),
        batches_max_size: 4,
        batches_max_concurrency: 10,
        add_metadata: " ".to_string(),
        dlq_arn: None,
        dlq_url: None,
        dlq_retry_limit: None,
        dlq_s3_bucket: None,
        lambda_assume_role: None,
        starlark_script,
        enable_log_group_tags: false,
        log_group_tags_cache_ttl_seconds: 300,
    }
}

fn aws_config() -> SdkConfig {
    SdkConfig::builder()
        .behavior_version(BehaviorVersion::latest())
        .region(S3Region::new("us-east-1"))
        .build()
}

// =============================================================================
// Script resolution
// =============================================================================

#[tokio::test]
async fn test_load_script_from_http_success() {
    let mock_server = MockServer::start().await;

    let script_content = "def transform(event):\n    return [event]";

    Mock::given(method("GET"))
        .and(path("/script.star"))
        .respond_with(ResponseTemplate::new(200).set_body_string(script_content))
        .mount(&mock_server)
        .await;

    let script_url = format!("{}/script.star", mock_server.uri());
    let config = create_test_config(Some(script_url));
    let aws = aws_config();

    let result = config.resolve_starlark_script(&aws).await.unwrap();
    assert_eq!(result, Some(script_content.to_string()));
}

#[tokio::test]
async fn test_load_script_from_http_404() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/script.star"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&mock_server)
        .await;

    let script_url = format!("{}/script.star", mock_server.uri());
    let config = create_test_config(Some(script_url));
    let aws = aws_config();

    let result = config.resolve_starlark_script(&aws).await;
    assert!(matches!(
        result,
        Err(ScriptLoadError::HttpError(status)) if status.as_u16() == 404
    ));
}

#[tokio::test]
async fn test_load_script_from_http_timeout() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/script.star"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string("def transform(event):\n    return [event]")
                .set_delay(std::time::Duration::from_secs(10)),
        )
        .mount(&mock_server)
        .await;

    let script_url = format!("{}/script.star", mock_server.uri());
    let config = create_test_config(Some(script_url));
    let aws = aws_config();

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        config.resolve_starlark_script(&aws),
    )
    .await;

    assert!(result.is_err());
}

#[tokio::test]
async fn test_load_script_from_s3_mock() {
    let script_content = "def transform(event):\n    return [event]";
    let bucket = "test-bucket";
    let key = "scripts/transform.star";

    let response_body = SdkBody::from(script_content.as_bytes());
    let replay_event = ReplayEvent::new(
        http::Request::builder()
            .method("GET")
            .uri(format!("https://{}.s3.amazonaws.com/{}", bucket, key))
            .body(SdkBody::empty())
            .unwrap(),
        Response::builder()
            .status(200)
            .body(response_body)
            .unwrap(),
    );

    let replay_client = StaticReplayClient::new(vec![replay_event]);

    let aws = SdkConfig::builder()
        .behavior_version(BehaviorVersion::latest())
        .credentials_provider(SharedCredentialsProvider::new(S3Credentials::new(
            "SOMETESTKEYID",
            "somesecretkey",
            Some("somesessiontoken".to_string()),
            None,
            "",
        )))
        .region(S3Region::new("us-east-1"))
        .http_client(replay_client)
        .build();

    let s3_path = format!("s3://{}/{}", bucket, key);
    let config = create_test_config(Some(s3_path));

    let result = config.resolve_starlark_script(&aws).await.unwrap();
    assert_eq!(result, Some(script_content.to_string()));
}

#[tokio::test]
async fn test_load_script_from_s3_invalid_path() {
    let config = create_test_config(Some("s3://bucket".to_string()));
    let aws = aws_config();

    let result = config.resolve_starlark_script(&aws).await;
    assert!(matches!(result, Err(ScriptLoadError::InvalidS3Path(_))));
}

#[tokio::test]
async fn test_load_script_from_base64() {
    let script = "def transform(event):\n    return [event]";
    let encoded = base64::engine::general_purpose::STANDARD.encode(script);

    let config = create_test_config(Some(encoded));
    let aws = aws_config();

    let result = config.resolve_starlark_script(&aws).await.unwrap();
    assert_eq!(result, Some(script.to_string()));
}

#[tokio::test]
async fn test_load_script_from_base64_invalid() {
    let config = create_test_config(Some("not valid base64!!!".to_string()));
    let aws = aws_config();

    let result = config.resolve_starlark_script(&aws).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_load_script_raw() {
    let script = "def transform(event):\n    return [event]";
    let config = create_test_config(Some(script.to_string()));
    let aws = aws_config();

    let result = config.resolve_starlark_script(&aws).await.unwrap();
    assert_eq!(result, Some(script.to_string()));
}

#[tokio::test]
async fn test_load_script_none() {
    let config = create_test_config(None);
    let aws = aws_config();

    let result = config.resolve_starlark_script(&aws).await.unwrap();
    assert_eq!(result, None);
}

#[tokio::test]
async fn test_priority_s3_over_url() {
    let s3_path = "s3://bucket/key";
    let config = create_test_config(Some(s3_path.to_string()));
    let aws = aws_config();

    let result = config.resolve_starlark_script(&aws).await;
    assert!(result.is_err());
    assert!(!matches!(result, Err(ScriptLoadError::HttpError(_))));
}

#[tokio::test]
async fn test_base64_padding_variants() {
    // Scripts must be 15+ chars so encoded form meets looks_like_base64's 20-char minimum
    let script1 = "short script here";
    let script2 = "medium length script content";
    let script3 = "very long script content that needs proper encoding";

    for script in [script1, script2, script3] {
        let encoded = base64::engine::general_purpose::STANDARD.encode(script);
        let config = create_test_config(Some(encoded));
        let aws = aws_config();

        let result = config.resolve_starlark_script(&aws).await.unwrap();
        assert_eq!(result, Some(script.to_string()));
    }
}

// =============================================================================
// Fail-open behavior
// =============================================================================

#[tokio::test]
async fn test_transform_logs_fail_open_on_script_resolution_failure() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/script.star"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&mock_server)
        .await;

    let script_url = format!("{}/script.star", mock_server.uri());
    let config = create_test_config(Some(script_url));
    let aws = aws_config();

    reset_cache().await;

    let logs = vec![
        r#"{"msg": "log1", "level": "info"}"#.to_string(),
        r#"{"msg": "log2", "level": "error"}"#.to_string(),
    ];
    let logs_clone = logs.clone();

    let result = transform_logs(logs, &config, &aws).await;

    assert!(result.is_ok(), "should pass through on resolution failure");
    let passed = result.unwrap();
    assert_eq!(passed, logs_clone, "should return original logs unchanged");
}

#[tokio::test]
async fn test_transform_logs_caches_failure_on_script_resolution() {
    // When resolution fails, we cache Some(None) so subsequent calls skip re-resolution.
    // Without this, record-by-record handlers (Kinesis, SQS) would retry network per record.
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/script.star"))
        .respond_with(ResponseTemplate::new(404))
        .expect(1) // Assert exactly one HTTP request; second call must use cache
        .mount(&mock_server)
        .await;

    let script_url = format!("{}/script.star", mock_server.uri());
    let config = create_test_config(Some(script_url));
    let aws = aws_config();

    reset_cache().await;

    let logs = vec![r#"{"msg": "log1"}"#.to_string()];
    let logs_clone = logs.clone();

    let result1 = transform_logs(logs.clone(), &config, &aws).await;
    assert!(result1.is_ok());
    assert_eq!(result1.unwrap(), logs_clone);

    let result2 = transform_logs(logs, &config, &aws).await;
    assert!(result2.is_ok());
    assert_eq!(result2.unwrap(), logs_clone);

    // MockServer verifies expect(1) on drop - if cache didn't work, we'd have 2 requests and panic
}

#[tokio::test]
async fn test_transform_logs_fail_open_on_script_compilation_failure() {
    let bad_script = r#"
def process(event):
    return [event]
"#;
    let config = create_test_config(Some(bad_script.to_string()));
    let aws = aws_config();

    reset_cache().await;

    let logs = vec![
        r#"{"msg": "log1", "level": "info"}"#.to_string(),
        r#"{"msg": "log2", "level": "error"}"#.to_string(),
    ];
    let logs_clone = logs.clone();

    let result = transform_logs(logs, &config, &aws).await;

    assert!(result.is_ok(), "should pass through on compilation failure");
    let passed = result.unwrap();
    assert_eq!(passed, logs_clone, "should return original logs unchanged");
}

#[tokio::test]
async fn test_transform_logs_caches_failure_on_script_compilation() {
    // When compilation fails (e.g. missing transform function), we cache Some(None)
    // so subsequent calls skip re-compilation.
    let bad_script = r#"
def process(event):
    return [event]
"#;
    let config = create_test_config(Some(bad_script.to_string()));
    let aws = aws_config();

    reset_cache().await;

    let logs = vec![
        r#"{"msg": "log1", "level": "info"}"#.to_string(),
        r#"{"msg": "log2", "level": "error"}"#.to_string(),
    ];
    let logs_clone = logs.clone();

    let result1 = transform_logs(logs.clone(), &config, &aws).await;
    assert!(result1.is_ok(), "first call should pass through on compilation failure");
    assert_eq!(result1.unwrap(), logs_clone);

    let result2 = transform_logs(logs, &config, &aws).await;
    assert!(result2.is_ok(), "second call should use cache and pass through without re-compiling");
    assert_eq!(result2.unwrap(), logs_clone);
}

#[tokio::test]
async fn test_transform_logs_fail_open_on_syntax_error() {
    let bad_script = r#"
def transform(event)  # Missing colon
    return [event]
"#;
    let config = create_test_config(Some(bad_script.to_string()));
    let aws = aws_config();

    reset_cache().await;

    let logs = vec![r#"{"msg": "test"}"#.to_string()];
    let logs_clone = logs.clone();

    let result = transform_logs(logs, &config, &aws).await;

    assert!(result.is_ok(), "should pass through on syntax error");
    let passed = result.unwrap();
    assert_eq!(passed, logs_clone, "should return original logs unchanged");
}
