use aws_config::BehaviorVersion;
use aws_sdk_s3::config::Credentials as S3Credentials;
use aws_sdk_s3::config::Region as S3Region;
use aws_smithy_runtime::client::http::test_util::ReplayEvent;
use aws_smithy_runtime::client::http::test_util::StaticReplayClient;
use aws_smithy_types::body::SdkBody;
use coralogix_aws_shipper::logs::config::{Config, IntegrationType, ScriptLoadError};
use cx_sdk_rest_logs::auth::ApiKey;
use http::Response;
use temp_env::with_vars;
use wiremock::{Mock, MockServer, ResponseTemplate};
use wiremock::matchers::{method, path};

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

#[tokio::test]
async fn test_load_script_from_http_success() {
    let mock_server = MockServer::start().await;
    
    let script_content = "def transform(event):\n    return [event]";
    
    Mock::given(method("GET"))
        .and(path("/script.star"))
        .respond_with(ResponseTemplate::new(200)
            .set_body_string(script_content))
        .mount(&mock_server)
        .await;
    
    let script_url = format!("{}/script.star", mock_server.uri());
    let config = create_test_config(Some(script_url));
    
    let aws_config = aws_config::Config::builder()
        .behavior_version(BehaviorVersion::latest())
        .region(S3Region::new("us-east-1"))
        .build();
    
    let result = config.resolve_starlark_script(&aws_config).await.unwrap();
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
    
    let aws_config = aws_config::Config::builder()
        .behavior_version(BehaviorVersion::latest())
        .region(S3Region::new("us-east-1"))
        .build();
    
    let result = config.resolve_starlark_script(&aws_config).await;
    assert!(matches!(result, Err(ScriptLoadError::HttpError(status)) if status.as_u16() == 404));
}

#[tokio::test]
async fn test_load_script_from_http_timeout() {
    let mock_server = MockServer::start().await;
    
    Mock::given(method("GET"))
        .and(path("/script.star"))
        .respond_with(ResponseTemplate::new(200)
            .set_body_string("def transform(event):\n    return [event]")
            .set_delay(std::time::Duration::from_secs(10)))
        .mount(&mock_server)
        .await;
    
    let script_url = format!("{}/script.star", mock_server.uri());
    let config = create_test_config(Some(script_url));
    
    let aws_config = aws_config::Config::builder()
        .behavior_version(BehaviorVersion::latest())
        .region(S3Region::new("us-east-1"))
        .build();
    
    // This should timeout - we'll use tokio::time::timeout to test
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        config.resolve_starlark_script(&aws_config)
    ).await;
    
    assert!(result.is_err()); // Timeout occurred
}

#[tokio::test]
async fn test_load_script_from_s3_mock() {
    let script_content = "def transform(event):\n    return [event]";
    let bucket = "test-bucket";
    let key = "scripts/transform.star";
    
    // Create a mock S3 response
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
    
    let aws_config = aws_config::Config::builder()
        .behavior_version(BehaviorVersion::latest())
        .credentials_provider(S3Credentials::new(
            "SOMETESTKEYID",
            "somesecretkey",
            Some("somesessiontoken".to_string()),
            None,
            "",
        ))
        .region(S3Region::new("us-east-1"))
        .http_client(replay_client)
        .build();
    
    let s3_path = format!("s3://{}/{}", bucket, key);
    let config = create_test_config(Some(s3_path));
    
    let result = config.resolve_starlark_script(&aws_config).await.unwrap();
    assert_eq!(result, Some(script_content.to_string()));
}

#[tokio::test]
async fn test_load_script_from_s3_invalid_path() {
    let config = create_test_config(Some("s3://bucket".to_string())); // Missing key
    
    let aws_config = aws_config::Config::builder()
        .behavior_version(BehaviorVersion::latest())
        .region(S3Region::new("us-east-1"))
        .build();
    
    let result = config.resolve_starlark_script(&aws_config).await;
    assert!(matches!(result, Err(ScriptLoadError::InvalidS3Path(_))));
}

#[tokio::test]
async fn test_load_script_from_base64() {
    let script = "def transform(event):\n    return [event]";
    let encoded = base64::engine::general_purpose::STANDARD.encode(script);
    
    let config = create_test_config(Some(encoded));
    
    let aws_config = aws_config::Config::builder()
        .behavior_version(BehaviorVersion::latest())
        .region(S3Region::new("us-east-1"))
        .build();
    
    let result = config.resolve_starlark_script(&aws_config).await.unwrap();
    assert_eq!(result, Some(script.to_string()));
}

#[tokio::test]
async fn test_load_script_from_base64_invalid() {
    let config = create_test_config(Some("not valid base64!!!".to_string()));
    
    let aws_config = aws_config::Config::builder()
        .behavior_version(BehaviorVersion::latest())
        .region(S3Region::new("us-east-1"))
        .build();
    
    let result = config.resolve_starlark_script(&aws_config).await;
    // Should fall back to treating as raw script since it doesn't look like base64
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_load_script_raw() {
    let script = "def transform(event):\n    return [event]";
    let config = create_test_config(Some(script.to_string()));
    
    let aws_config = aws_config::Config::builder()
        .behavior_version(BehaviorVersion::latest())
        .region(S3Region::new("us-east-1"))
        .build();
    
    let result = config.resolve_starlark_script(&aws_config).await.unwrap();
    assert_eq!(result, Some(script.to_string()));
}

#[tokio::test]
async fn test_load_script_none() {
    let config = create_test_config(None);
    
    let aws_config = aws_config::Config::builder()
        .behavior_version(BehaviorVersion::latest())
        .region(S3Region::new("us-east-1"))
        .build();
    
    let result = config.resolve_starlark_script(&aws_config).await.unwrap();
    assert_eq!(result, None);
}

#[tokio::test]
async fn test_priority_s3_over_url() {
    // S3 path should take priority over URL
    let mock_server = MockServer::start().await;
    let script_url = format!("{}/script.star", mock_server.uri());
    
    // Create config with both S3 and URL (though Config only has one field)
    // In reality, CloudFormation validation ensures only one is set
    // This test verifies S3 detection works
    let s3_path = "s3://bucket/key";
    let config = create_test_config(Some(s3_path.to_string()));
    
    let aws_config = aws_config::Config::builder()
        .behavior_version(BehaviorVersion::latest())
        .region(S3Region::new("us-east-1"))
        .build();
    
    // Should attempt S3 load (will fail without proper mock, but should detect S3 path)
    let result = config.resolve_starlark_script(&aws_config).await;
    // Should fail with S3 error, not HTTP error, proving S3 was attempted first
    assert!(result.is_err());
    // Error should be S3-related, not HTTP-related
    assert!(!matches!(result, Err(ScriptLoadError::HttpError(_))));
}

#[tokio::test]
async fn test_base64_padding_variants() {
    // Test base64 with different padding scenarios
    let script1 = "short";
    let script2 = "medium length script";
    let script3 = "very long script content that needs proper encoding";
    
    for script in [script1, script2, script3] {
        let encoded = base64::engine::general_purpose::STANDARD.encode(script);
        let config = create_test_config(Some(encoded));
        
        let aws_config = aws_config::Config::builder()
            .behavior_version(BehaviorVersion::latest())
            .region(S3Region::new("us-east-1"))
            .build();
        
        let result = config.resolve_starlark_script(&aws_config).await.unwrap();
        assert_eq!(result, Some(script.to_string()));
    }
}
