use async_trait::async_trait;
use aws_config::BehaviorVersion;
use aws_lambda_events::event::cloudwatch_logs::AwsLogs;
use aws_lambda_events::event::s3::S3Event;
use aws_lambda_events::sns::SnsEvent;
use aws_sdk_s3::Client;
use coralogix_aws_shipper::combined_event::CombinedEvent;
use coralogix_aws_shipper::config::Config;
use cx_sdk_core::auth::AuthData;
use cx_sdk_rest_logs::model::{LogBulkRequest, LogSinglesRequest};
use cx_sdk_rest_logs::LogExporter;
use lambda_runtime::{Context, LambdaEvent};
use serde::Serialize;

use std::string::String;
use std::sync::Arc;
use std::sync::Mutex;

fn s3event_string(bucket: &str, key: &str) -> String {
    format!(
        r#"{{
        "Records": [
            {{
            "eventVersion": "2.0",
            "eventSource": "aws:s3",
            "awsRegion": "eu-west-1",
            "eventTime": "1970-01-01T00:00:00.000Z",
            "eventName": "ObjectCreated:Put",
            "userIdentity": {{
                "principalId": "EXAMPLE"
            }},
            "requestParameters": {{
                "sourceIPAddress": "127.0.0.1"
            }},
            "responseElements": {{
                "x-amz-request-id": "EXAMPLE123456789",
                "x-amz-id-2": "EXAMPLE123/5678abcdefghijklambdaisawesome/mnopqrstuvwxyzABCDEFGH"
            }},
            "s3": {{
                "s3SchemaVersion": "1.0",
                "configurationId": "testConfigRule",
                "bucket": {{
                "name": "{}",
                "ownerIdentity": {{
                    "principalId": "EXAMPLE"
                }},
                "arn": "arn:aws:s3:::{}"
                }},
                "object": {{
                "key": "{}",
                "size": 311000048,
                "eTag": "0123456789abcdef0123456789abcdef",
                "sequencer": "0A1B2C3D4E5F678901"
                }}
            }}
            }}
        ]
    }}"#,
        bucket, bucket, key
    )
}

// get_mock_s3client returns a mock s3 client that returns the data from the given file
fn get_mock_s3client(src: Option<&str>) -> Result<Client, String> {
    let data = match src {
        Some(source) => std::fs::read(source).map_err(|e| e.to_string())?,
        None => Vec::new(),
    };

    let replay_event = aws_smithy_runtime::client::http::test_util::ReplayEvent::new(
        http::Request::builder()
            .body(aws_smithy_types::body::SdkBody::from(""))
            .unwrap(),
        http::Response::builder()
            .status(200)
            .body(aws_smithy_types::body::SdkBody::from(data))
            .unwrap(),
    );

    let conf = aws_sdk_s3::Config::builder()
        .behavior_version(BehaviorVersion::latest())
        .credentials_provider(aws_sdk_s3::config::Credentials::new(
            "SOMETESTKEYID",
            "somesecretkey",
            Some("somesessiontoken".to_string()),
            None,
            "",
        ))
        .region(aws_sdk_s3::config::Region::new("eu-central-1"))
        .http_client(
            aws_smithy_runtime::client::http::test_util::StaticReplayClient::new(vec![
                replay_event,
            ]),
        )
        .build();

    Ok(aws_sdk_s3::Client::from_conf(conf))
}

#[derive(Default, Debug, Clone)]
pub struct FakeLogExporter {
    bulks: Arc<Mutex<Vec<LogBulkRequest<serde_json::Value>>>>,
    singles: Arc<Mutex<Vec<LogSinglesRequest<serde_json::Value>>>>,
}

impl FakeLogExporter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn take_bulks(&self) -> Vec<LogBulkRequest<serde_json::Value>> {
        std::mem::take(&mut self.bulks.lock().unwrap())
    }

    pub fn take_singles(&self) -> Vec<LogSinglesRequest<serde_json::Value>> {
        std::mem::take(&mut self.singles.lock().unwrap())
    }
}

#[async_trait]
impl LogExporter for FakeLogExporter {
    async fn export_bulk<B>(
        &self,
        request: LogBulkRequest<B>,
        _: &AuthData,
    ) -> Result<(), cx_sdk_rest_logs::Error>
    where
        B: Serialize + Send + Sync,
    {
        self.bulks
            .lock()
            .unwrap()
            .push(request.try_map_body(serde_json::to_value)?);
        Ok(())
    }

    async fn export_singles<B>(
        &self,
        request: LogSinglesRequest<B>,
        _: &AuthData,
    ) -> Result<(), cx_sdk_rest_logs::Error>
    where
        B: Serialize + Send + Sync,
    {
        self.singles
            .lock()
            .unwrap()
            .push(request.try_map_body(serde_json::to_value)?);
        Ok(())
    }
}

async fn run_test_s3_event() {
    let s3_client =
        get_mock_s3client(Some("./tests/fixtures/s3.log")).expect("failed to create s3 client");
    let config = Config::load_from_env().expect("failed to load config from env");

    let (bucket, key) = ("coralogix-serverless-repo", "coralogix-aws-shipper/s3.log");
    let evt: S3Event = serde_json::from_str(
        s3event_string(bucket, key).as_str(),
    )
    .expect("failed to parse s3_event");

    let exporter = Arc::new(FakeLogExporter::new());
    let combined_event = CombinedEvent::S3(evt);
    let event = LambdaEvent::new(combined_event, Context::default());

    coralogix_aws_shipper::function_handler(&s3_client, exporter.clone(), &config, event)
        .await
        .unwrap();

    let bulks = exporter.take_bulks();
    assert!(bulks.is_empty());

    let singles = exporter.take_singles();
    assert_eq!(singles.len(), 1);
    assert_eq!(singles[0].entries.len(), 4);
    let log_lines = vec![
        "172.17.0.1 - - [26/Oct/2023:11:01:10 +0000] \"GET / HTTP/1.1\" 304 0 \"-\" \"Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/118.0.0.0 Safari/537.36\" \"-\"",
        "172.17.0.1 - - [26/Oct/2023:11:29:33 +0000] \"GET / HTTP/1.1\" 304 0 \"-\" \"Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/118.0.0.0 Safari/537.36\" \"-\"",
        "172.17.0.1 - - [26/Oct/2023:11:34:52 +0000] \"GET / HTTP/1.1\" 304 0 \"-\" \"Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/118.0.0.0 Safari/537.36\" \"-\"",
        "172.17.0.1 - - [26/Oct/2023:11:57:06 +0000] \"GET / HTTP/1.1\" 304 0 \"-\" \"Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/118.0.0.0 Safari/537.36\" \"-\"",
    ];
    for (i, log_line) in log_lines.iter().enumerate() {
        assert!(singles[0].entries[i].body == *log_line);
    }

    assert!(
        singles[0].entries[0].application_name == "integration-testing",
        "got application_name: {}",
        singles[0].entries[0].application_name
    );
    assert!(
        singles[0].entries[0].subsystem_name == "coralogix-serverless-repo",
        "got subsystem_name: {}",
        singles[0].entries[0].subsystem_name
    );
}

#[tokio::test]
async fn test_s3_event() {
    temp_env::async_with_vars(
        [
            ("CORALOGIX_API_KEY", Some("1234456789X")),
            ("APP_NAME", Some("integration-testing")),
            ("CORALOGIX_ENDPOINT", Some("localhost:8080")),
            ("SAMPLING", Some("1")),
            ("INTEGRATION_TYPE", Some("S3")),
            ("AWS_REGION", Some("eu-central-1")),
        ],
        run_test_s3_event(),
    )
    .await;
}

async fn run_cloudtraillogs_s3_event() {
    let s3_client = get_mock_s3client(Some("./tests/fixtures/cloudtrail.log.gz"))
        .expect("failed to create s3 client");
    let config = Config::load_from_env().expect("failed to load config from env");

    let evt: S3Event = serde_json::from_str(
        s3event_string(
            "coralogix-serverless-repo",
            "coralogix-aws-shipper/cloudtrail.log.gz",
        )
        .as_str(),
    )
    .expect("failed to parse s3_event");

    let exporter = Arc::new(FakeLogExporter::new());
    let combined_event = CombinedEvent::S3(evt);
    let event = LambdaEvent::new(combined_event, Context::default());

    coralogix_aws_shipper::function_handler(&s3_client, exporter.clone(), &config, event)
        .await
        .unwrap();

    let bulks = exporter.take_bulks();
    assert!(bulks.is_empty());

    let singles = exporter.take_singles();
    assert_eq!(singles.len(), 1);
    assert_eq!(singles[0].entries.len(), 20);
    let log_lines = vec![
        "{\"additionalEventData\":{\"AuthenticationMethod\":\"AuthHeader\",\"CipherSuite\":\"ECDHE-RSA-AES128-GCM-SHA256\",\"SignatureVersion\":\"SigV4\",\"bytesTransferredIn\":0,\"bytesTransferredOut\":480,\"x-amz-id-2\":\"1z7a7FcycBJ1A+larL8G04ZyJ3noJ823M3XEMt02L1jPF+QCGCBtudtO82vouBkJ+10K2jbfhA4=\"},\"awsRegion\":\"eu-central-1\",\"eventCategory\":\"Management\",\"eventID\":\"8d50073e-6d4f-4380-918e-cc17dd847be5\",\"eventName\":\"GetBucketAcl\",\"eventSource\":\"s3.amazonaws.com\",\"eventTime\":\"2023-10-17T04:53:21Z\",\"eventType\":\"AwsApiCall\",\"eventVersion\":\"1.09\",\"managementEvent\":true,\"readOnly\":true,\"recipientAccountId\":\"597078901540\",\"requestID\":\"JTT53K6AS8TR39ER\",\"requestParameters\":{\"Host\":\"aws-cloudtrail-logs-597078901540-082ac93e.s3.eu-central-1.amazonaws.com\",\"acl\":\"\",\"bucketName\":\"aws-cloudtrail-logs-597078901540-082ac93e\"},\"resources\":[{\"ARN\":\"arn:aws:s3:::aws-cloudtrail-logs-597078901540-082ac93e\",\"accountId\":\"597078901540\",\"type\":\"AWS::S3::Bucket\"}],\"responseElements\":null,\"sharedEventID\":\"d0264078-8cb0-45eb-8b53-32a8469888af\",\"sourceIPAddress\":\"cloudtrail.amazonaws.com\",\"userAgent\":\"cloudtrail.amazonaws.com\",\"userIdentity\":{\"invokedBy\":\"cloudtrail.amazonaws.com\",\"type\":\"AWSService\"}}",
        "{\"awsRegion\":\"eu-central-1\",\"eventCategory\":\"Management\",\"eventID\":\"5fb5255f-7ad4-4bc6-a0a3-0ab6d1113b8c\",\"eventName\":\"GenerateDataKey\",\"eventSource\":\"kms.amazonaws.com\",\"eventTime\":\"2023-10-17T04:53:24Z\",\"eventType\":\"AwsApiCall\",\"eventVersion\":\"1.08\",\"managementEvent\":true,\"readOnly\":true,\"recipientAccountId\":\"597078901540\",\"requestID\":\"4e67281e-2ae9-4515-b75f-e56cc6873220\",\"requestParameters\":{\"encryptionContext\":{\"aws:cloudtrail:arn\":\"arn:aws:cloudtrail:eu-central-1:597078901540:trail/Mytrail\",\"aws:s3:arn\":\"arn:aws:s3:::aws-cloudtrail-logs-597078901540-082ac93e/AWSLogs/597078901540/CloudTrail/eu-west-1/2023/10/17/597078901540_CloudTrail_eu-west-1_20231017T0450Z_KREKSWgLUgTraBu8.json.gz\"},\"keyId\":\"arn:aws:kms:eu-central-1:597078901540:key/a339d1af-e88e-4801-8d64-5c7861a4405f\",\"keySpec\":\"AES_256\"},\"resources\":[{\"ARN\":\"arn:aws:kms:eu-central-1:597078901540:key/a339d1af-e88e-4801-8d64-5c7861a4405f\",\"accountId\":\"597078901540\",\"type\":\"AWS::KMS::Key\"}],\"responseElements\":null,\"sharedEventID\":\"a2999d91-c0d3-4037-9171-93e6a1a08e53\",\"sourceIPAddress\":\"cloudtrail.amazonaws.com\",\"userAgent\":\"cloudtrail.amazonaws.com\",\"userIdentity\":{\"invokedBy\":\"cloudtrail.amazonaws.com\",\"type\":\"AWSService\"}}",
        "{\"additionalEventData\":{\"AuthenticationMethod\":\"AuthHeader\",\"CipherSuite\":\"ECDHE-RSA-AES128-GCM-SHA256\",\"SignatureVersion\":\"SigV4\",\"bytesTransferredIn\":0,\"bytesTransferredOut\":480,\"x-amz-id-2\":\"q2Jj4jfv73eSK1oWlBOTMMPsCU0YhMcYUcXrCi8W8s4NZfzPEgW9xrSmpir1iMIrV+zs0kR2MwE=\"},\"awsRegion\":\"eu-central-1\",\"eventCategory\":\"Management\",\"eventID\":\"d7ced48b-0d40-43ba-a78c-25b4389654c0\",\"eventName\":\"GetBucketAcl\",\"eventSource\":\"s3.amazonaws.com\",\"eventTime\":\"2023-10-17T04:53:26Z\",\"eventType\":\"AwsApiCall\",\"eventVersion\":\"1.09\",\"managementEvent\":true,\"readOnly\":true,\"recipientAccountId\":\"597078901540\",\"requestID\":\"19XECQVKGPN8JJ6D\",\"requestParameters\":{\"Host\":\"aws-cloudtrail-logs-597078901540-082ac93e.s3.eu-central-1.amazonaws.com\",\"acl\":\"\",\"bucketName\":\"aws-cloudtrail-logs-597078901540-082ac93e\"},\"resources\":[{\"ARN\":\"arn:aws:s3:::aws-cloudtrail-logs-597078901540-082ac93e\",\"accountId\":\"597078901540\",\"type\":\"AWS::S3::Bucket\"}],\"responseElements\":null,\"sharedEventID\":\"f513090d-b111-40e5-940a-81b0f98fe916\",\"sourceIPAddress\":\"cloudtrail.amazonaws.com\",\"userAgent\":\"cloudtrail.amazonaws.com\",\"userIdentity\":{\"invokedBy\":\"cloudtrail.amazonaws.com\",\"type\":\"AWSService\"}}"
    ];

    for (i, log_line) in log_lines.iter().enumerate() {
        assert!(singles[0].entries[i].body == *log_line);
    }

    assert!(
        singles[0].entries[0].application_name == "integration-testing",
        "got application_name: {}",
        singles[0].entries[0].application_name
    );
    assert!(
        singles[0].entries[0].subsystem_name == "coralogix-serverless-repo",
        "got subsystem_name: {}",
        singles[0].entries[0].subsystem_name
    );
}

#[tokio::test]
async fn test_cloudtraillogs_s3_event() {
    temp_env::async_with_vars(
        [
            ("CORALOGIX_API_KEY", Some("1234456789X")),
            ("APP_NAME", Some("integration-testing")),
            ("CORALOGIX_ENDPOINT", Some("localhost:8080")),
            ("SAMPLING", Some("1")),
            ("AWS_REGION", Some("eu-central-1")),
            ("INTEGRATION_TYPE", Some("CloudTrail")),
        ],
        run_cloudtraillogs_s3_event(),
    )
    .await;
}

async fn run_csv_s3_event() {
    let s3_client =
        get_mock_s3client(Some("./tests/fixtures/s3csv.log")).expect("failed to create s3 client");
    let config = Config::load_from_env().expect("failed to load config from env");

    let (bucket, key) = (
        "coralogix-serverless-repo",
        "coralogix-aws-shipper/s3csv.log",
    );
    let evt: S3Event = serde_json::from_str(s3event_string(bucket, key).as_str())
        .expect("failed to parse s3_event");

    let exporter = Arc::new(FakeLogExporter::new());
    let combined_event = CombinedEvent::S3(evt);
    let event = LambdaEvent::new(combined_event, Context::default());

    coralogix_aws_shipper::function_handler(&s3_client, exporter.clone(), &config, event)
        .await
        .unwrap();

    let bulks = exporter.take_bulks();
    assert!(bulks.is_empty());

    let singles = exporter.take_singles();
    assert_eq!(singles.len(), 1);
    assert_eq!(singles[0].entries.len(), 2);
    let log_lines = vec![
        "{\"id\":1,\"message\":\"This is an info message\",\"severity\":\"INFO\",\"timestamp\":\"2019-01-01 00:00:00\"}",
        "{\"id\":2,\"message\":\"This is another info message\",\"severity\":\"INFO\",\"timestamp\":\"2019-01-01 00:00:01\"}"
    ];

    for (i, log_line) in log_lines.iter().enumerate() {
        assert!(singles[0].entries[i].body == *log_line);
    }

    assert!(
        singles[0].entries[0].application_name == "integration-testing",
        "got application_name: {}",
        singles[0].entries[0].application_name
    );
    assert!(
        singles[0].entries[0].subsystem_name == "coralogix-serverless-repo",
        "got subsystem_name: {}",
        singles[0].entries[0].subsystem_name
    );
}

#[tokio::test]
async fn test_csv_s3_event() {
    temp_env::async_with_vars(
        [
            ("CORALOGIX_API_KEY", Some("1234456789X")),
            ("APP_NAME", Some("integration-testing")),
            ("CORALOGIX_ENDPOINT", Some("localhost:8080")),
            ("SAMPLING", Some("1")),
            ("AWS_REGION", Some("eu-central-1")),
            ("INTEGRATION_TYPE", Some("S3Csv")),
        ],
        run_csv_s3_event(),
    )
    .await;
}

async fn run_vpcflowlgos_s3_event() {
    let s3_client = get_mock_s3client(Some("./tests/fixtures/vpcflow.log.gz"))
        .expect("failed to create s3 client");
    let config = Config::load_from_env().unwrap();

    let evt: S3Event = serde_json::from_str(
        s3event_string(
            "coralogix-serverless-repo",
            "coralogix-aws-shipper/vpcflow.log.gz",
        )
        .as_str(),
    )
    .expect("failed to parse s3_event");

    let exporter = Arc::new(FakeLogExporter::new());
    let combined_event = CombinedEvent::S3(evt);
    let event = LambdaEvent::new(combined_event, Context::default());

    coralogix_aws_shipper::function_handler(&s3_client, exporter.clone(), &config, event)
        .await
        .unwrap();

    let bulks = exporter.take_bulks();
    assert!(bulks.is_empty());

    let singles = exporter.take_singles();
    assert_eq!(singles.len(), 1);
    assert_eq!(singles[0].entries.len(), 2);
    let log_lines = vec![
        "{\"account-id\":\"123456789012\",\"action\":\"ACCEPT\",\"bytes\":4096,\"dstaddr\":\"172.31.9.12\",\"dstport\":3389,\"end\":1418530070,\"interface-id\":\"eni-abc123de\",\"log-status\":\"OK\",\"packets\":20,\"protocol\":6,\"srcaddr\":\"172.31.9.69\",\"srcport\":49761,\"start\":1418530010,\"version\":2}",
        "{\"account-id\":\"123456789012\",\"action\":\"ACCEPT\",\"bytes\":5060,\"dstaddr\":\"172.31.9.21\",\"dstport\":3389,\"end\":1418530070,\"interface-id\":\"eni-abc123de\",\"log-status\":\"OK\",\"packets\":20,\"protocol\":6,\"srcaddr\":\"172.31.9.69\",\"srcport\":49761,\"start\":1418530010,\"version\":2}",
    ];

    for (i, log_line) in log_lines.iter().enumerate() {
        assert!(singles[0].entries[i].body == *log_line);
    }

    assert!(
        singles[0].entries[0].application_name == "integration-testing",
        "got application_name: {}",
        singles[0].entries[0].application_name
    );
    assert!(
        singles[0].entries[0].subsystem_name == "coralogix-serverless-repo",
        "got subsystem_name: {}",
        singles[0].entries[0].subsystem_name
    );
}

#[tokio::test]
async fn test_vpcflowlgos_s3_event() {
    temp_env::async_with_vars(
        [
            ("CORALOGIX_API_KEY", Some("1234456789X")),
            ("APP_NAME", Some("integration-testing")),
            ("CORALOGIX_ENDPOINT", Some("localhost:8080")),
            ("SAMPLING", Some("1")),
            ("AWS_REGION", Some("eu-central-1")),
            ("INTEGRATION_TYPE", Some("VpcFlow")),
        ],
        run_vpcflowlgos_s3_event(),
    )
    .await;
}

async fn run_sns_event() {
    let s3_client = get_mock_s3client(None).expect("failed to create s3 client");
    let config = Config::load_from_env().unwrap();

    let evt: SnsEvent = serde_json::from_str(
        r#"{
            "Records": [
            {
                "EventVersion": "1.0",
                "EventSubscriptionArn": "arn:aws:sns:REGION:ACCOUNT-ID:TOPIC-NAME:SUBSCRIPTION-ID",
                "EventSource": "aws:sns",
                "Sns": {
                "Type": "Notification",
                "MessageId": "95df01b4-ee98-5cb9-9903-4c221d41eb5e",
                "TopicArn": "arn:aws:sns:REGION:ACCOUNT-ID:TOPIC-NAME",
                "Subject": "Amazon S3 Notification",
                "Message": "[INFO] some test log line",
                "Timestamp": "1970-01-01T00:00:00.000Z",
                "SignatureVersion": "1",
                "Signature": "EXAMPLE",
                "SigningCertUrl": "EXAMPLE",
                "UnsubscribeUrl": "EXAMPLE",
                "MessageAttributes": {}
                }
            }
            ]
        }"#,
    )
    .expect("failed to parse s3_event");

    let exporter = Arc::new(FakeLogExporter::new());
    let combined_event = CombinedEvent::Sns(evt);
    let event = LambdaEvent::new(combined_event, Context::default());

    coralogix_aws_shipper::function_handler(&s3_client, exporter.clone(), &config, event)
        .await
        .unwrap();

    let bulks = exporter.take_bulks();
    assert!(bulks.is_empty());

    let singles = exporter.take_singles();
    assert_eq!(singles.len(), 1);
    assert_eq!(singles[0].entries.len(), 1);
    let log_lines = vec!["[INFO] some test log line"];

    for (i, log_line) in log_lines.iter().enumerate() {
        assert!(
            singles[0].entries[i].body == *log_line,
            "log line: {}",
            singles[0].entries[i].body
        );
    }

    assert!(
        singles[0].entries[0].application_name == "integration-testing",
        "got application_name: {}",
        singles[0].entries[0].application_name
    );
    assert!(
        singles[0].entries[0].subsystem_name == "lambda",
        "got subsystem_name: {}",
        singles[0].entries[0].subsystem_name
    );
}

#[tokio::test]
async fn test_sns_event() {
    temp_env::async_with_vars(
        [
            ("CORALOGIX_API_KEY", Some("1234456789X")),
            ("APP_NAME", Some("integration-testing")),
            ("CORALOGIX_ENDPOINT", Some("localhost:8080")),
            ("SAMPLING", Some("1")),
            ("SUB_NAME", Some("lambda")),
            ("AWS_REGION", Some("eu-central-1")),
            ("INTEGRATION_TYPE", Some("Sns")),
        ],
        run_sns_event(),
    )
    .await;
}

async fn run_cloudwatchlogs_event() {
    let s3_client = get_mock_s3client(None).expect("failed to create s3 client");
    let config = Config::load_from_env().unwrap();

    let evt: AwsLogs = serde_json::from_str(
        r#"{
            "data": "H4sIAAAAAAAAAHWPwQqCQBCGX0Xm7EFtK+smZBEUgXoLCdMhFtKV3akI8d0bLYmibvPPN3wz00CJxmQnTO41whwWQRIctmEcB6sQbFC3CjW3XW8kxpOpP+OC22d1Wml1qZkQGtoMsScxaczKN3plG8zlaHIta5KqWsozoTYw3/djzwhpLwivWFGHGpAFe7DL68JlBUk+l7KSN7tCOEJ4M3/qOI49vMHj+zCKdlFqLaU2ZHV2a4Ct/an0/ivdX8oYc1UVX860fQDQiMdxRQEAAA=="
        }"#)
    .expect("failed to parse cloudwatchlogs event");

    let exporter = Arc::new(FakeLogExporter::new());
    let combined_event = CombinedEvent::CloudWatchLogs(evt);
    let event = LambdaEvent::new(combined_event, Context::default());

    coralogix_aws_shipper::function_handler(&s3_client, exporter.clone(), &config, event)
        .await
        .unwrap();

    let bulks = exporter.take_bulks();
    assert!(bulks.is_empty());

    let singles = exporter.take_singles();
    assert_eq!(singles.len(), 1);
    assert_eq!(singles[0].entries.len(), 2);
    let log_lines = vec!["[ERROR] First test message", "[ERROR] Second test message"];

    for (i, log_line) in log_lines.iter().enumerate() {
        assert!(
            singles[0].entries[i].body == *log_line,
            "log line: {}",
            singles[0].entries[i].body
        );
    }

    assert!(
        singles[0].entries[0].application_name == "integration-testing",
        "got application_name: {}",
        singles[0].entries[0].application_name
    );
    assert!(
        singles[0].entries[0].subsystem_name == "lambda",
        "got subsystem_name: {}",
        singles[0].entries[0].subsystem_name
    );
}

#[tokio::test]
async fn test_cloudwatchlogs_event() {
    temp_env::async_with_vars(
        [
            ("CORALOGIX_API_KEY", Some("1234456789X")),
            ("APP_NAME", Some("integration-testing")),
            ("CORALOGIX_ENDPOINT", Some("localhost:8080")),
            ("SAMPLING", Some("1")),
            ("SUB_NAME", Some("lambda")),
            ("AWS_REGION", Some("eu-central-1")),
            ("INTEGRATION_TYPE", Some("CloudWatch")),
        ],
        run_cloudwatchlogs_event(),
    )
    .await;
}

async fn run_blocking_and_newline_pattern() {
    let s3_client =
        get_mock_s3client(Some("./tests/fixtures/multiline.log")).expect("failed to create s3 client");
    let config = Config::load_from_env().expect("failed to load config from env");

    let (bucket, key) = (
        "coralogix-serverless-repo",
        "coralogix-aws-shipper/multiline.log",
    );

    let evt: S3Event = serde_json::from_str(
        s3event_string(
            bucket,
            key,
        )
        .as_str(),
    )
    .expect("failed to parse s3_event");

    let exporter = Arc::new(FakeLogExporter::new());
    let combined_event = CombinedEvent::S3(evt);
    let event = LambdaEvent::new(combined_event, Context::default());

    coralogix_aws_shipper::function_handler(&s3_client, exporter.clone(), &config, event)
        .await
        .unwrap();

    let bulks = exporter.take_bulks();
    assert!(bulks.is_empty());

    println!("{:?}", exporter);

    let singles = exporter.take_singles();
    assert_eq!(singles.len(), 1);
    assert_eq!(singles[0].entries.len(), 1);
    let log_lines = vec!["00:40:45.810 [main] INFO  example.MapMessageExample"];

    for (i, log_line) in log_lines.iter().enumerate() {
        assert!(
            singles[0].entries[i].body == *log_line,
            "log line: {}",
            singles[0].entries[i].body
        );
    }

    assert!(
        singles[0].entries[0].application_name == "integration-testing",
        "got application_name: {}",
        singles[0].entries[0].application_name
    );
    assert!(
        singles[0].entries[0].subsystem_name == "lambda",
        "got subsystem_name: {}",
        singles[0].entries[0].subsystem_name
    );
}

#[tokio::test]
async fn test_blocking_and_newline_pattern() {
    temp_env::async_with_vars(
        [
            ("CORALOGIX_API_KEY", Some("1234456789X")),
            ("APP_NAME", Some("integration-testing")),
            ("CORALOGIX_ENDPOINT", Some("localhost:8080")),
            ("SAMPLING", Some("1")),
            ("SUB_NAME", Some("lambda")),
            ("AWS_REGION", Some("eu-central-1")),
            ("INTEGRATION_TYPE", Some("S3")),
            ("BLOCKING_PATTERN", Some("ERROR")), // blocking pattern
            ("NEWLINE_PATTERN", Some(r"\<\|\>")), // newline pattern
        ],
        run_blocking_and_newline_pattern(),
    )
    .await;
}


// Note: we test the s3_event handler directly here, since the integration tests above will bypass it
// using the mock s3 client. The cloudwatch and sns events are not bypassed, since they don't use
// the s3client, as such, they are covered by the integration tests above.
#[tokio::test]
async fn test_s3_event_handler() {
    // test normal s3 event
    let s3_event = s3event_string("coralogix-serverless-repo", "coralogix-aws-shipper/s3.log");
    let evt: S3Event = serde_json::from_str(s3_event.as_str()).expect("failed to parse s3_event");
    let (bucket, key) = coralogix_aws_shipper::handle_s3_event(evt).await.unwrap();
    assert_eq!(bucket, "coralogix-serverless-repo");
    assert_eq!(key, "coralogix-aws-shipper/s3.log");

    // test s3 event with spaces in key name (note: aws event replaces spaces with +)
    let s3_event = s3event_string("coralogix-serverless-repo", "coralogix-aws-shipper/s3+with+spaces.log");
    let evt: S3Event = serde_json::from_str(s3_event.as_str()).expect("failed to parse s3_event");
    let (bucket, key) = coralogix_aws_shipper::handle_s3_event(evt).await.unwrap();
    assert_eq!(bucket, "coralogix-serverless-repo");
    assert_eq!(key, "coralogix-aws-shipper/s3 with spaces.log");
}