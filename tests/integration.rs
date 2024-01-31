use async_trait::async_trait;
use aws_config::BehaviorVersion;
use aws_lambda_events::event::cloudwatch_logs::LogsEvent;
use aws_lambda_events::event::s3::S3Event;
use aws_lambda_events::kafka::KafkaEvent;
use aws_lambda_events::sns::SnsEvent;
use aws_lambda_events::sqs::SqsEvent;
use aws_lambda_events::event::kinesis::KinesisEvent;
use aws_sdk_s3::Client;
use coralogix_aws_shipper::combined_event::CombinedEvent;
use coralogix_aws_shipper::config::Config;
use cx_sdk_core::auth::AuthData;
use cx_sdk_rest_logs::model::{LogBulkRequest, LogSinglesRequest};
use cx_sdk_rest_logs::LogExporter;
use lambda_runtime::{Context, LambdaEvent};
use serde::Serialize;
use serde_json::Value;

use std::string::String;
use std::sync::Arc;
use std::sync::Mutex;

pub fn s3event_string(bucket: &str, key: &str) -> String {
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

async fn run_test_folder_s3_event() {
    let s3_client =
        get_mock_s3client(Some("./tests/fixtures/s3.log")).expect("failed to create s3 client");
    let config = Config::load_from_env().expect("failed to load config from env");

    let (bucket, key) = ("coralogix-serverless-repo", "coralogix-aws-shipper/elb1/s3.log");
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
        singles[0].entries[0].application_name == "elb1",
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
async fn test_folder_s3_event() {
    temp_env::async_with_vars(
        [
            ("CORALOGIX_API_KEY", Some("1234456789X")),
            ("APP_NAME", Some("{{s3_key.2}}")),
            ("CORALOGIX_ENDPOINT", Some("localhost:8080")),
            ("SAMPLING", Some("1")),
            ("INTEGRATION_TYPE", Some("S3")),
            ("AWS_REGION", Some("eu-central-1")),
        ],
        run_test_folder_s3_event(),
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
        let expected: Value = serde_json::from_str(log_line).unwrap();
        assert_eq!(singles[0].entries[i].body, expected);
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
        let expected: Value = serde_json::from_str(log_line).unwrap();
        assert_eq!(singles[0].entries[i].body, expected);
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
        let expected: Value = serde_json::from_str(log_line).unwrap();
        assert_eq!(singles[0].entries[i].body, expected);
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

async fn run_test_s3_event_large() {
    let s3_client =
        get_mock_s3client(Some("./tests/fixtures/large.log")).expect("failed to create s3 client");
    let config = Config::load_from_env().expect("failed to load config from env");

    let (bucket, key) = (
        "coralogix-serverless-repo",
        "coralogix-aws-shipper/large.log",
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

    println!("singles --> {}", singles.len());

    assert!(singles.len() == 11);
    assert!(singles[0].entries.len() == 4754);
    assert!(singles[10].entries.len() == 2092);

    let log_lines = vec![
        "https 2023-09-05T05:35:00.264447Z app/eks-prod-white-ext-tv2-alb/acf1e236c71b3b9f 122.162.149.35:1438 10.1.136.193:32081 0.001 0.004 0.000 200 200 1839 229 \"POST https://lumberjack.razorpay.com:443/v1/track HTTP/1.1\" \"Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/116.0.0.0 Safari/537.36\" ECDHE-RSA-AES128-GCM-SHA256 TLSv1.2 arn:aws:elasticloadbalancing:ap-south-1:141592612890:targetgroup/eks-prod-white-ext-tv2-alb/c297b80d227b01fa \"Root=1-64f6be04-1556248a707c237b2ea45a98\" \"lumberjack.razorpay.com\" \"arn:aws:acm:ap-south-1:141592612890:certificate/7cc265c2-7abf-4aa1-a573-c30bcd4f4f80\" 0 2023-09-05T05:35:00.258000Z \"waf,forward\" \"-\" \"-\" \"10.1.136.193:32081\" \"200\" \"-\" \"-\"",
        "https 2023-09-05T05:35:00.272291Z app/eks-prod-white-ext-tv2-alb/acf1e236c71b3b9f 52.66.76.63:60451 10.1.135.210:32081 0.001 0.008 0.000 200 200 19931 229 \"POST https://lumberjack.razorpay.com:443/v1/track HTTP/1.1\" \"lua-resty-http/0.17.1 (Lua) ngx_lua/10021\" ECDHE-RSA-AES128-GCM-SHA256 TLSv1.2 arn:aws:elasticloadbalancing:ap-south-1:141592612890:targetgroup/eks-prod-white-ext-tv2-alb/c297b80d227b01fa \"Root=1-64f6be04-7e72712808e598e26d2904b7\" \"lumberjack.razorpay.com\" \"arn:aws:acm:ap-south-1:141592612890:certificate/7cc265c2-7abf-4aa1-a573-c30bcd4f4f80\" 0 2023-09-05T05:35:00.263000Z \"waf,forward\" \"-\" \"-\" \"10.1.135.210:32081\" \"200\" \"-\" \"-\"",
        "https 2023-09-05T05:35:17.297301Z app/eks-prod-white-ext-tv2-alb/acf1e236c71b3b9f 52.95.73.41:46938 10.1.130.135:32081 0.001 0.009 0.000 200 200 3542 197 \"POST https://stork-ext.razorpay.com:443/email/callback/ses HTTP/1.1\" \"Amazon Simple Notification Service Agent\" ECDHE-RSA-AES128-GCM-SHA256 TLSv1.2 arn:aws:elasticloadbalancing:ap-south-1:141592612890:targetgroup/eks-prod-white-ext-tv2-alb/c297b80d227b01fa \"Root=1-64f6be15-7819a44d3a13961c4cdf3353\" \"stork-ext.razorpay.com\" \"arn:aws:acm:ap-south-1:141592612890:certificate/7cc265c2-7abf-4aa1-a573-c30bcd4f4f80\" 0 2023-09-05T05:35:17.287000Z \"waf,forward\" \"-\" \"-\" \"10.1.130.135:32081\" \"200\" \"-\" \"-\"", // last log line
    ];

    // iterate first 2 log lines
    for (i, log_line) in log_lines[0..1].iter().enumerate() {
        assert!(singles[0].entries[i].body == *log_line);
    }

    // iterate last log line
    for (i, log_line) in log_lines[2..].iter().enumerate() {
        assert!(singles[10].entries[i].body == *log_line);
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
async fn test_s3_event_large() {
    temp_env::async_with_vars(
        [
            ("CORALOGIX_API_KEY", Some("1234456789X")),
            ("APP_NAME", Some("integration-testing")),
            ("CORALOGIX_ENDPOINT", Some("localhost:8080")),
            ("SAMPLING", Some("1")),
            ("INTEGRATION_TYPE", Some("S3")),
            ("AWS_REGION", Some("eu-central-1")),
        ],
        run_test_s3_event_large(),
    )
    .await;
}

async fn run_test_s3_event_large_with_sampling() {
    let s3_client =
        get_mock_s3client(Some("./tests/fixtures/large.log")).expect("failed to create s3 client");
    let config = Config::load_from_env().expect("failed to load config from env");

    let (bucket, key) = (
        "coralogix-serverless-repo",
        "coralogix-aws-shipper/large.log",
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

    assert!(singles.len() == 1);
    assert!(singles[0].entries.len() == 500);

    let log_lines = vec![
        "https 2023-09-05T05:35:00.264447Z app/eks-prod-white-ext-tv2-alb/acf1e236c71b3b9f 122.162.149.35:1438 10.1.136.193:32081 0.001 0.004 0.000 200 200 1839 229 \"POST https://lumberjack.razorpay.com:443/v1/track HTTP/1.1\" \"Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/116.0.0.0 Safari/537.36\" ECDHE-RSA-AES128-GCM-SHA256 TLSv1.2 arn:aws:elasticloadbalancing:ap-south-1:141592612890:targetgroup/eks-prod-white-ext-tv2-alb/c297b80d227b01fa \"Root=1-64f6be04-1556248a707c237b2ea45a98\" \"lumberjack.razorpay.com\" \"arn:aws:acm:ap-south-1:141592612890:certificate/7cc265c2-7abf-4aa1-a573-c30bcd4f4f80\" 0 2023-09-05T05:35:00.258000Z \"waf,forward\" \"-\" \"-\" \"10.1.136.193:32081\" \"200\" \"-\" \"-\"",
        "https 2023-09-05T05:35:00.258438Z app/eks-prod-white-ext-tv2-alb/acf1e236c71b3b9f 52.66.76.63:18102 10.1.136.193:32081 0.000 0.005 0.000 200 200 757 229 \"POST https://lumberjack.razorpay.com:443/v1/track HTTP/1.1\" \"Go-http-client/1.1\" ECDHE-RSA-AES128-GCM-SHA256 TLSv1.2 arn:aws:elasticloadbalancing:ap-south-1:141592612890:targetgroup/eks-prod-white-ext-tv2-alb/c297b80d227b01fa \"Root=1-64f6be04-1d977c737026bc8b7c4b78f9\" \"lumberjack.razorpay.com\" \"arn:aws:acm:ap-south-1:141592612890:certificate/7cc265c2-7abf-4aa1-a573-c30bcd4f4f80\" 0 2023-09-05T05:35:00.253000Z \"waf,forward\" \"-\" \"-\" \"10.1.136.193:32081\" \"200\" \"-\" \"-\"",
    ];

    // iterate first 2 log lines
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
async fn test_s3_event_large_with_sampling() {
    temp_env::async_with_vars(
        [
            ("CORALOGIX_API_KEY", Some("1234456789X")),
            ("APP_NAME", Some("integration-testing")),
            ("CORALOGIX_ENDPOINT", Some("localhost:8080")),
            ("SAMPLING", Some("100")),
            ("INTEGRATION_TYPE", Some("S3")),
            ("AWS_REGION", Some("eu-central-1")),
        ],
        run_test_s3_event_large_with_sampling(),
    )
    .await;
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

    let evt: LogsEvent = serde_json::from_str(
        r#"{
            "awslogs": {
              "data": "H4sIAAAAAAAAAHWPwQqCQBCGX0Xm7EFtK+smZBEUgXoLCdMhFtKV3akI8d0bLYmibvPPN3wz00CJxmQnTO41whwWQRIctmEcB6sQbFC3CjW3XW8kxpOpP+OC22d1Wml1qZkQGtoMsScxaczKN3plG8zlaHIta5KqWsozoTYw3/djzwhpLwivWFGHGpAFe7DL68JlBUk+l7KSN7tCOEJ4M3/qOI49vMHj+zCKdlFqLaU2ZHV2a4Ct/an0/ivdX8oYc1UVX860fQDQiMdxRQEAAA=="
            }
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
    let s3_client = get_mock_s3client(Some("./tests/fixtures/multiline.log"))
        .expect("failed to create s3 client");
    let config = Config::load_from_env().expect("failed to load config from env");

    let (bucket, key) = (
        "coralogix-serverless-repo",
        "coralogix-aws-shipper/multiline.log",
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

async fn run_test_empty_s3_event() {
    let s3_client =
        get_mock_s3client(Some("./tests/fixtures/empty.log")).expect("failed to create s3 client");
    let config = Config::load_from_env().expect("failed to load config from env");

    let (bucket, key) = ("coralogix-serverless-repo", "coralogix-aws-shipper/empty.log");
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
    assert!(singles.is_empty());
}

#[tokio::test]
async fn test_empty_s3_event() {
    temp_env::async_with_vars(
        [
            ("CORALOGIX_API_KEY", Some("1234456789X")),
            ("APP_NAME", Some("integration-testing")),
            ("CORALOGIX_ENDPOINT", Some("localhost:8080")),
            ("SAMPLING", Some("1")),
            ("SUB_NAME", Some("lambda")),
            ("AWS_REGION", Some("eu-central-1")),
            ("INTEGRATION_TYPE", Some("S3")),
        ],
        run_test_empty_s3_event(),
    )
    .await;
}

async fn run_sqs_s3_event() {
    let s3_client = get_mock_s3client(Some("./tests/fixtures/s3.log")).expect("failed to create s3 client");
    let config = Config::load_from_env().unwrap();

    let evt: SqsEvent = serde_json::from_str(
        r#"{
            "Records": [
              {
                "attributes": {
                  "ApproximateFirstReceiveTimestamp": "0",
                  "ApproximateReceiveCount": "1",
                  "SenderId": "SENDERID:EXAMPLE",
                  "SentTimestamp": "0"
                },
                "awsRegion": "us-east-1",
                "body": "{\"Records\":[{\"eventVersion\":\"2.1\",\"eventSource\":\"aws:s3\",\"awsRegion\":\"us-east-1\",\"eventTime\":\"1970-01-01T00:00:00.000Z\",\"eventName\":\"ObjectCreated:Put\",\"userIdentity\":{\"principalId\":\"PRINCIPALID:EXAMPLE\"},\"requestParameters\":{\"sourceIPAddress\":\"192.0.2.1\"},\"responseElements\":{\"x-amz-request-id\":\"REQUESTIDEXAMPLE\",\"x-amz-id-2\":\"IDEXAMPLE\"},\"s3\":{\"s3SchemaVersion\":\"1.0\",\"configurationId\":\"CONFIGEXAMPLE\",\"bucket\":{\"name\":\"coralogix-serverless-repo\",\"ownerIdentity\":{\"principalId\":\"OWNERIDEXAMPLE\"},\"arn\":\"arn:aws:s3:::coralogix-serverless-repo\"},\"object\":{\"key\":\"coralogix-aws-shipper/s3.log\",\"size\":123,\"eTag\":\"ETAGEXAMPLE\",\"sequencer\":\"SEQUENCEREXAMPLE\"}}}]}",
                "eventSource": "aws:sqs",
                "eventSourceARN": "arn:aws:sqs:us-east-1:123456789012:SQSDLQ",
                "md5OfBody": "MD5EXAMPLE",
                "messageAttributes": {},
                "messageId": "00000000-0000-0000-0000-000000000000",
                "receiptHandle": "RECEIPTHANDLEEXAMPLE"
              }
            ]
          }"#
        )
    .expect("failed to parse sqs_s3_event");

    let exporter = Arc::new(FakeLogExporter::new());
    let combined_event = CombinedEvent::Sqs(evt);
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
async fn test_sqs_s3_event() {
    temp_env::async_with_vars(
        [
            ("CORALOGIX_API_KEY", Some("1234456789X")),
            ("APP_NAME", Some("integration-testing")),
            ("CORALOGIX_ENDPOINT", Some("localhost:8080")),
            ("SAMPLING", Some("1")),
            ("INTEGRATION_TYPE", Some("S3")),
            ("AWS_REGION", Some("eu-central-1")),
        ],
        run_sqs_s3_event(),
    )
    .await;
}

async fn run_sqs_event() {
    let s3_client = get_mock_s3client(None).expect("failed to create s3 client");
    let config = Config::load_from_env().unwrap();

    let evt: SqsEvent = serde_json::from_str(
        r#"{
            "Records": [
              {
                "attributes": {
                  "ApproximateFirstReceiveTimestamp": "0",
                  "ApproximateReceiveCount": "1",
                  "SenderId": "SENDERID:EXAMPLE",
                  "SentTimestamp": "0"
                },
                "awsRegion": "us-east-1",
                "body": "[INFO] some test log line",
                "eventSource": "aws:sqs",
                "eventSourceARN": "arn:aws:sqs:us-east-1:123000000000:SQSQUEUE",
                "md5OfBody": "MD5EXAMPLE",
                "messageAttributes": {},
                "messageId": "00000000-0000-0000-0000-000000000000",
                "receiptHandle": "RECEIPTHANDLEEXAMPLE"
              }
            ]
          }"#,
    )
    .expect("failed to parse s3_event");

    let exporter = Arc::new(FakeLogExporter::new());
    let combined_event = CombinedEvent::Sqs(evt);
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
async fn test_sqs_event() {
    temp_env::async_with_vars(
        [
            ("CORALOGIX_API_KEY", Some("1234456789X")),
            ("APP_NAME", Some("integration-testing")),
            ("CORALOGIX_ENDPOINT", Some("localhost:8080")),
            ("SAMPLING", Some("1")),
            ("SUB_NAME", Some("lambda")),
            ("AWS_REGION", Some("eu-central-1")),
            ("INTEGRATION_TYPE", Some("Sqs")),
        ],
        run_sqs_event(),
    )
    .await;
}

async fn run_kinesis_event() {
    let s3_client = get_mock_s3client(None).expect("failed to create s3 client");
    let config = Config::load_from_env().unwrap();

    let evt: KinesisEvent = serde_json::from_str(
        r#"{
            "Records": [
                {
                    "awsRegion": "us-east-1",
                    "eventID": "shardId-000000000000:00000000000000000000000000000000000000000000000000000000",
                    "eventName": "aws:kinesis:record",
                    "eventSource": "aws:kinesis",
                    "eventSourceARN": "arn:aws:kinesis:us-east-1:0000000000:stream/mystream",
                    "eventVersion": "1.0",
                    "invokeIdentityArn": "arn:aws:iam::0000000000:role/cargo-lambda-role-0000000-0000-0000-0000-00000000000",
                    "kinesis": {
                        "approximateArrivalTimestamp": 1704715421.323,
                        "data": "RHVtbXkgZGF0YQ==",
                        "kinesisSchemaVersion": "1.0",
                        "partitionKey": "partition_key",
                        "sequenceNumber": "49647983248916725783135500075978324609922193443375808530"
                    }
                }
            ]
        }"#,
    )
    .expect("failed to parse kinesis_event");

    let exporter = Arc::new(FakeLogExporter::new());
    let combined_event = CombinedEvent::Kinesis(evt);
    let event = LambdaEvent::new(combined_event, Context::default());

    coralogix_aws_shipper::function_handler(&s3_client, exporter.clone(), &config, event)
        .await
        .unwrap();

    let bulks = exporter.take_bulks();
    assert!(bulks.is_empty());

    let singles = exporter.take_singles();
    assert_eq!(singles.len(), 1);
    assert_eq!(singles[0].entries.len(), 1);
    let log_lines = vec!["Dummy data"];

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
async fn test_kinesis_event() {
    temp_env::async_with_vars(
        [
            ("CORALOGIX_API_KEY", Some("1234456789X")),
            ("APP_NAME", Some("integration-testing")),
            ("CORALOGIX_ENDPOINT", Some("localhost:8080")),
            ("SAMPLING", Some("1")),
            ("SUB_NAME", Some("lambda")),
            ("AWS_REGION", Some("eu-central-1")),
            ("INTEGRATION_TYPE", Some("Kinesis")),
        ],
        run_kinesis_event(),
    )
    .await;
}

async fn run_cloudfront_s3_event() {
    let s3_client =
        get_mock_s3client(Some("./tests/fixtures/cloudfront.gz")).expect("failed to create s3 client");
    let config = Config::load_from_env().expect("failed to load config from env");

    let (bucket, key) = (
        "coralogix-serverless-repo",
        "coralogix-aws-shipper/cloudfront.gz",
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
    assert_eq!(singles[0].entries.len(), 4);

    let log_lines = vec![
        "{\"c-ip\": \"179.37.223.62\",\n  \"c-port\": 49375,\n  \"cs(Cookie)\": \"-\",\n  \"cs(Host)\": \"d2s17x7wkoojlc.cloudfront.net\",\n  \"cs(Referer)\": \"-\",\n  \"cs(User-Agent)\": \"Mozilla/5.0%20(Macintosh;%20Intel%20Mac%20OS%20X%2010_15_7)%20AppleWebKit/537.36%20(KHTML,%20like%20Gecko)%20Chrome/119.0.0.0%20Safari/537.36\",\n  \"cs-bytes\": 453,\n  \"cs-method\": \"GET\",\n  \"cs-protocol\": \"https\",\n  \"cs-protocol-version\": \"HTTP/2.0\",\n  \"cs-uri-query\": \"-\",\n  \"cs-uri-stem\": \"/\",\n  \"date\": \"2024-01-08\",\n  \"fle-encrypted-fields\": \"-\",\n  \"fle-status\": \"-\",\n  \"sc-bytes\": 780,\n  \"sc-content-len\": 507,\n  \"sc-content-type\": \"text/html\",\n  \"sc-range-end\": \"-\",\n  \"sc-range-start\": \"-\",\n  \"sc-status\": 502,\n  \"ssl-cipher\": \"TLS_AES_128_GCM_SHA256\",\n  \"ssl-protocol\": \"TLSv1.3\",\n  \"time\": \"16:56:53\",\n  \"time-taken\": \"0.167\",\n  \"time-to-first-byte\": \"0.167\",\n  \"x-edge-detailed-result-type\": \"OriginDnsError\",\n  \"x-edge-location\": \"EZE50-P2\",\n  \"x-edge-request-id\": \"3vr84z1By73gt94sb9ctFnbgUb1EK6rqQpoCPMSwbwok7D49uf3cVw==\",\n  \"x-edge-response-result-type\": \"Error\",\n  \"x-edge-result-type\": \"Error\",\n  \"x-forwarded-for\": \"-\",\n  \"x-host-header\": \"d2s17x7wkoojlc.cloudfront.net\"}",
        "{\"c-ip\": \"179.37.223.62\",\n  \"c-port\": 49375,\n  \"cs(Cookie)\": \"-\",\n  \"cs(Host)\": \"d2s17x7wkoojlc.cloudfront.net\",\n  \"cs(Referer)\": \"https://d2s17x7wkoojlc.cloudfront.net/\",\n  \"cs(User-Agent)\": \"Mozilla/5.0%20(Macintosh;%20Intel%20Mac%20OS%20X%2010_15_7)%20AppleWebKit/537.36%20(KHTML,%20like%20Gecko)%20Chrome/119.0.0.0%20Safari/537.36\",\n  \"cs-bytes\": 136,\n  \"cs-method\": \"GET\",\n  \"cs-protocol\": \"https\",\n  \"cs-protocol-version\": \"HTTP/2.0\",\n  \"cs-uri-query\": \"-\",\n  \"cs-uri-stem\": \"/favicon.ico\",\n  \"date\": \"2024-01-08\",\n  \"fle-encrypted-fields\": \"-\",\n  \"fle-status\": \"-\",\n  \"sc-bytes\": 781,\n  \"sc-content-len\": 507,\n  \"sc-content-type\": \"text/html\",\n  \"sc-range-end\": \"-\",\n  \"sc-range-start\": \"-\",\n  \"sc-status\": 502,\n  \"ssl-cipher\": \"TLS_AES_128_GCM_SHA256\",\n  \"ssl-protocol\": \"TLSv1.3\",\n  \"time\": \"16:56:54\",\n  \"time-taken\": \"0.163\",\n  \"time-to-first-byte\": \"0.163\",\n  \"x-edge-detailed-result-type\": \"OriginDnsError\",\n  \"x-edge-location\": \"EZE50-P2\",\n  \"x-edge-request-id\": \"bBnwFlTyBT0c29Ba_AuVD6ALSSu5nrUXzyW7XG74CwVMsbgpvdEF3Q==\",\n  \"x-edge-response-result-type\": \"Error\",\n  \"x-edge-result-type\": \"Error\",\n  \"x-forwarded-for\": \"-\",\n  \"x-host-header\": \"d2s17x7wkoojlc.cloudfront.net\"}",
        "{\"c-ip\": \"179.37.223.62\",\n  \"c-port\": 49391,\n  \"cs(Cookie)\": \"-\",\n  \"cs(Host)\": \"d2s17x7wkoojlc.cloudfront.net\",\n  \"cs(Referer)\": \"-\",\n  \"cs(User-Agent)\": \"Mozilla/5.0%20(Macintosh;%20Intel%20Mac%20OS%20X%2010_15_7)%20AppleWebKit/537.36%20(KHTML,%20like%20Gecko)%20Chrome/119.0.0.0%20Safari/537.36\",\n  \"cs-bytes\": 462,\n  \"cs-method\": \"GET\",\n  \"cs-protocol\": \"https\",\n  \"cs-protocol-version\": \"HTTP/2.0\",\n  \"cs-uri-query\": \"-\",\n  \"cs-uri-stem\": \"/\",\n  \"date\": \"2024-01-08\",\n  \"fle-encrypted-fields\": \"-\",\n  \"fle-status\": \"-\",\n  \"sc-bytes\": 785,\n  \"sc-content-len\": 507,\n  \"sc-content-type\": \"text/html\",\n  \"sc-range-end\": \"-\",\n  \"sc-range-start\": \"-\",\n  \"sc-status\": 502,\n  \"ssl-cipher\": \"TLS_AES_128_GCM_SHA256\",\n  \"ssl-protocol\": \"TLSv1.3\",\n  \"time\": \"16:56:59\",\n  \"time-taken\": \"0.001\",\n  \"time-to-first-byte\": \"0.001\",\n  \"x-edge-detailed-result-type\": \"Error\",\n  \"x-edge-location\": \"EZE50-P2\",\n  \"x-edge-request-id\": \"g9c8US-JEZ87C92_2dChfj-RiZ1aza8n0scq8XoXOESNqK94Yrpw9Q==\",\n  \"x-edge-response-result-type\": \"Error\",\n  \"x-edge-result-type\": \"Error\",\n  \"x-forwarded-for\": \"-\",\n  \"x-host-header\": \"d2s17x7wkoojlc.cloudfront.net\"}",
        "{\"c-ip\": \"179.37.223.62\",\n  \"c-port\": 49391,\n  \"cs(Cookie)\": \"-\",\n  \"cs(Host)\": \"d2s17x7wkoojlc.cloudfront.net\",\n  \"cs(Referer)\": \"https://d2s17x7wkoojlc.cloudfront.net/\",\n  \"cs(User-Agent)\": \"Mozilla/5.0%20(Macintosh;%20Intel%20Mac%20OS%20X%2010_15_7)%20AppleWebKit/537.36%20(KHTML,%20like%20Gecko)%20Chrome/119.0.0.0%20Safari/537.36\",\n  \"cs-bytes\": 136,\n  \"cs-method\": \"GET\",\n  \"cs-protocol\": \"https\",\n  \"cs-protocol-version\": \"HTTP/2.0\",\n  \"cs-uri-query\": \"-\",\n  \"cs-uri-stem\": \"/favicon.ico\",\n  \"date\": \"2024-01-08\",\n  \"fle-encrypted-fields\": \"-\",\n  \"fle-status\": \"-\",\n  \"sc-bytes\": 785,\n  \"sc-content-len\": 507,\n  \"sc-content-type\": \"text/html\",\n  \"sc-range-end\": \"-\",\n  \"sc-range-start\": \"-\",\n  \"sc-status\": 502,\n  \"ssl-cipher\": \"TLS_AES_128_GCM_SHA256\",\n  \"ssl-protocol\": \"TLSv1.3\",\n  \"time\": \"16:56:59\",\n  \"time-taken\": \"0.000\",\n  \"time-to-first-byte\": \"0.000\",\n  \"x-edge-detailed-result-type\": \"Error\",\n  \"x-edge-location\": \"EZE50-P2\",\n  \"x-edge-request-id\": \"d9gtwwsExRoLvnTr319jrfihZOGY3PbRbWOZq-_pPx0bVO00TKyEkw==\",\n  \"x-edge-response-result-type\": \"Error\",\n  \"x-edge-result-type\": \"Error\",\n  \"x-forwarded-for\": \"-\",\n  \"x-host-header\": \"d2s17x7wkoojlc.cloudfront.net\"}"
    ];

    for (i, log_line) in log_lines.iter().enumerate() {
        let expected: Value = serde_json::from_str(log_line).unwrap();
        assert_eq!(singles[0].entries[i].body, expected);
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
async fn test_cloudfront_s3_event() {
    temp_env::async_with_vars(
        [
            ("CORALOGIX_API_KEY", Some("1234456789X")),
            ("APP_NAME", Some("integration-testing")),
            ("CORALOGIX_ENDPOINT", Some("localhost:8080")),
            ("SAMPLING", Some("1")),
            ("AWS_REGION", Some("eu-central-1")),
            ("INTEGRATION_TYPE", Some("CloudFront")),
        ],
        run_cloudfront_s3_event(),
    )
    .await;
}

async fn run_test_s3_event_with_metadata() {
    let s3_client =
        get_mock_s3client(Some("./tests/fixtures/s3.log")).expect("failed to create s3 client");
    let config = Config::load_from_env().expect("failed to load config from env");

    let (bucket, key) = ("coralogix-serverless-repo", "coralogix-aws-shipper/s3.log");
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
    assert_eq!(singles[0].entries.len(), 4);
    let log_lines = vec![
        "{\"key_name\":\"coralogix-aws-shipper/s3.log\",\"bucket_name\":\"coralogix-serverless-repo\",\"message\":\"172.17.0.1 - - [26/Oct/2023:11:01:10 +0000] \\\"GET / HTTP/1.1\\\" 304 0 \\\"-\\\" \\\"Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/118.0.0.0 Safari/537.36\\\" \\\"-\\\"\"}",
        "{\"key_name\":\"coralogix-aws-shipper/s3.log\",\"bucket_name\":\"coralogix-serverless-repo\",\"message\":\"172.17.0.1 - - [26/Oct/2023:11:29:33 +0000] \\\"GET / HTTP/1.1\\\" 304 0 \\\"-\\\" \\\"Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/118.0.0.0 Safari/537.36\\\" \\\"-\\\"\"}",
        "{\"key_name\":\"coralogix-aws-shipper/s3.log\",\"bucket_name\":\"coralogix-serverless-repo\",\"message\":\"172.17.0.1 - - [26/Oct/2023:11:34:52 +0000] \\\"GET / HTTP/1.1\\\" 304 0 \\\"-\\\" \\\"Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/118.0.0.0 Safari/537.36\\\" \\\"-\\\"\"}",
        "{\"key_name\":\"coralogix-aws-shipper/s3.log\",\"bucket_name\":\"coralogix-serverless-repo\",\"message\":\"172.17.0.1 - - [26/Oct/2023:11:57:06 +0000] \\\"GET / HTTP/1.1\\\" 304 0 \\\"-\\\" \\\"Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/118.0.0.0 Safari/537.36\\\" \\\"-\\\"\"}",
    ];
    for (i, log_line) in log_lines.iter().enumerate() {
        let expected: Value = serde_json::from_str(log_line).unwrap();
        assert_eq!(singles[0].entries[i].body, expected);
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
async fn test_s3_event_with_metadata() {
    temp_env::async_with_vars(
        [
            ("CORALOGIX_API_KEY", Some("1234456789X")),
            ("APP_NAME", Some("integration-testing")),
            ("CORALOGIX_ENDPOINT", Some("localhost:8080")),
            ("SAMPLING", Some("1")),
            ("INTEGRATION_TYPE", Some("S3")),
            ("AWS_REGION", Some("eu-central-1")),
            ("ADD_METADATA", Some("bucket_name,key_name")),
        ],
        run_test_s3_event_with_metadata(),
    )
    .await;
}

async fn run_test_s3_event_elb() {
    let s3_client =
        get_mock_s3client(Some("./tests/fixtures/elb.log.gz")).expect("failed to create s3 client");
    let config = Config::load_from_env().expect("failed to load config from env");

    let (bucket, key) = ("coralogix-serverless-repo", "coralogix-aws-shipper/elb.log.gz");
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

    // print number of entries and singles
    println!("singles: {:?}", singles.len());
    println!("entries: {:?}", singles[0].entries.len());

    assert_eq!(singles.len(), 1);
    assert_eq!(singles[0].entries.len(), 576);

    let first_line = r#"grpcs 2024-01-15T12:10:00.200705Z app/staging-load-balancer/900ec696b1d45ff6 10.69.80.245:36584 10.69.71.185:9002 0.000 0.023 0.000 200 200 588 429 "POST https://metrics.supert.sh:443/supertenant.superproto.metrics.v1.MetricsService/ReportMetrics HTTP/2.0" "grpc-go/1.46.2" TLS_AES_128_GCM_SHA256 TLSv1.3 arn:aws:elasticloadbalancing:us-east-1:850012249230:targetgroup/sputnik1-target/7bee47c166ccb48f "Root=1-65a52098-58b5bf0604a095737ba62e06" "metrics.supert.sh" "arn:aws:acm:us-east-1:850012249230:certificate/573ba1a4-763c-47cc-8a81-e98132fef479" 2 2024-01-15T12:10:00.176000Z "forward" "-" "-" "10.69.71.185:9002" "200" "-" "-""#;
    let last_line = r#"grpcs 2024-01-15T12:15:00.094329Z app/staging-load-balancer/900ec696b1d45ff6 10.69.80.245:36584 10.69.71.185:9002 0.000 0.001 0.000 200 200 324 433 "POST https://metrics.supert.sh:443/supertenant.superproto.brain.hello.v1.HelloService/Hello HTTP/2.0" "grpc-go/1.46.2" TLS_AES_128_GCM_SHA256 TLSv1.3 arn:aws:elasticloadbalancing:us-east-1:850012249230:targetgroup/sputnik1-target/7bee47c166ccb48f "Root=1-65a521c4-0c1928a90d9e605111dd066a" "metrics.supert.sh" "arn:aws:acm:us-east-1:850012249230:certificate/573ba1a4-763c-47cc-8a81-e98132fef479" 2 2024-01-15T12:15:00.092000Z "forward" "-" "-" "10.69.71.185:9002" "200" "-" "-""#;

    assert!(first_line == singles[0].entries[0].body, "got: {}", singles[0].entries[0].body);
    assert!(last_line == singles[0].entries.last().unwrap().body, "got: {}", singles[0].entries.last().unwrap().body);

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
async fn test_s3_event_elb() {
    temp_env::async_with_vars(
        [
            ("CORALOGIX_API_KEY", Some("1234456789X")),
            ("APP_NAME", Some("integration-testing")),
            ("CORALOGIX_ENDPOINT", Some("localhost:8080")),
            ("SAMPLING", Some("1")),
            ("INTEGRATION_TYPE", Some("S3")),
            ("AWS_REGION", Some("eu-central-1")),
        ],
        run_test_s3_event_elb(),
    )
    .await;
}

async fn run_kafka_event() {
    let s3_client = get_mock_s3client(None).expect("failed to create s3 client");
    let config = Config::load_from_env().unwrap();

    let evt: KafkaEvent = serde_json::from_str(
        r#"{
            "eventSource": "SelfManagedKafka",
            "bootstrapServers":"b-2.demo-cluster-1.a1bcde.c1.kafka.us-east-1.amazonaws.com:9092,b-1.demo-cluster-1.a1bcde.c1.kafka.us-east-1.amazonaws.com:9092",
            "records":{
               "mytopic-0":[
                  {
                     "topic":"mytopic",
                     "partition":0,
                     "offset":15,
                     "timestamp":1545084650987,
                     "timestampType":"CREATE_TIME",
                     "key":"abcDEFghiJKLmnoPQRstuVWXyz1234==",
                     "value":"some log message",
                     "headers":[
                        {
                           "headerKey":[
                              104,
                              101,
                              97,
                              100,
                              101,
                              114,
                              86,
                              97,
                              108,
                              117,
                              101
                           ]
                        }
                     ]
                  }
               ]
            }
         }"#,
    )
    .expect("failed to parse kinesis_event");

    let exporter = Arc::new(FakeLogExporter::new());
    let combined_event = CombinedEvent::Kafka(evt);
    let event = LambdaEvent::new(combined_event, Context::default());

    coralogix_aws_shipper::function_handler(&s3_client, exporter.clone(), &config, event)
        .await
        .unwrap();

    let bulks = exporter.take_bulks();
    assert!(bulks.is_empty());

    let singles = exporter.take_singles();
    assert_eq!(singles.len(), 1);
    assert_eq!(singles[0].entries.len(), 1);
    let log_line = "some log message";
    assert!(
        singles[0].entries[0].body == *log_line,
        "log line: {}",
        singles[0].entries[0].body
    );

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

async fn run_kafka_event_with_base64() {
    let s3_client = get_mock_s3client(None).expect("failed to create s3 client");
    let config = Config::load_from_env().unwrap();

    let evt: KafkaEvent = serde_json::from_str(
        r#"{
            "eventSource": "SelfManagedKafka",
            "bootstrapServers":"b-2.demo-cluster-1.a1bcde.c1.kafka.us-east-1.amazonaws.com:9092,b-1.demo-cluster-1.a1bcde.c1.kafka.us-east-1.amazonaws.com:9092",
            "records":{
               "mytopic-0":[
                  {
                     "topic":"mytopic",
                     "partition":0,
                     "offset":15,
                     "timestamp":1545084650987,
                     "timestampType":"CREATE_TIME",
                     "key":"abcDEFghiJKLmnoPQRstuVWXyz1234==",
                     "value":"c29tZSBsb2cgbWVzc2FnZQ==",
                     "headers":[
                        {
                           "headerKey":[
                              104,
                              101,
                              97,
                              100,
                              101,
                              114,
                              86,
                              97,
                              108,
                              117,
                              101
                           ]
                        }
                     ]
                  }
               ]
            }
         }"#,
    )
    .expect("failed to parse kinesis_event");

    let exporter = Arc::new(FakeLogExporter::new());
    let combined_event = CombinedEvent::Kafka(evt);
    let event = LambdaEvent::new(combined_event, Context::default());

    coralogix_aws_shipper::function_handler(&s3_client, exporter.clone(), &config, event)
        .await
        .unwrap();

    let bulks = exporter.take_bulks();
    assert!(bulks.is_empty());

    let singles = exporter.take_singles();
    assert_eq!(singles.len(), 1);
    assert_eq!(singles[0].entries.len(), 1);
    let log_line = "some log message";
    assert!(
        singles[0].entries[0].body == *log_line,
        "log line: {}",
        singles[0].entries[0].body
    );

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
async fn test_kafka_event() {
    temp_env::async_with_vars(
        [
            ("CORALOGIX_API_KEY", Some("1234456789X")),
            ("APP_NAME", Some("integration-testing")),
            ("CORALOGIX_ENDPOINT", Some("localhost:8080")),
            ("SAMPLING", Some("1")),
            ("SUB_NAME", Some("lambda")),
            ("AWS_REGION", Some("eu-central-1")),
            ("INTEGRATION_TYPE", Some("MSK")),
        ],
        run_kafka_event(),
    )
    .await;

    temp_env::async_with_vars(
        [
            ("CORALOGIX_API_KEY", Some("1234456789X")),
            ("APP_NAME", Some("integration-testing")),
            ("CORALOGIX_ENDPOINT", Some("localhost:8080")),
            ("SAMPLING", Some("1")),
            ("SUB_NAME", Some("lambda")),
            ("AWS_REGION", Some("eu-central-1")),
            ("INTEGRATION_TYPE", Some("Kafka")),
        ],
        run_kafka_event(),
    )
    .await;

    temp_env::async_with_vars(
        [
            ("CORALOGIX_API_KEY", Some("1234456789X")),
            ("APP_NAME", Some("integration-testing")),
            ("CORALOGIX_ENDPOINT", Some("localhost:8080")),
            ("SAMPLING", Some("1")),
            ("SUB_NAME", Some("lambda")),
            ("AWS_REGION", Some("eu-central-1")),
            ("INTEGRATION_TYPE", Some("Kafka")),
        ],
        run_kafka_event_with_base64(),
    )
    .await;
}