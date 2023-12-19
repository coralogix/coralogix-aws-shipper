use aws_lambda_events::s3;
use aws_lambda_events::sns::SnsEvent;
use coralogix_aws_shipper::config::Config;
use async_trait::async_trait;
use aws_config::BehaviorVersion;
use aws_lambda_events::event::cloudwatch_logs::AwsLogs;
use aws_lambda_events::event::s3::S3Event;
use aws_sdk_s3::Client;
use coralogix_aws_shipper::combined_event::CombinedEvent;
use cx_sdk_core::auth::AuthData;
use cx_sdk_rest_logs::model::{LogBulkRequest, LogSinglesEntry, LogSinglesRequest};
use cx_sdk_rest_logs::LogExporter;
// use cx_sdk_rest_logs::{DynLogExporter, RestLogExporter};
use lambda_runtime::{Context, LambdaEvent};
// use reqwest::Request;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::string::String;
use std::sync::Arc;
use lazy_static::lazy_static;
use std::sync::Mutex;


#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TestSingleEntry<B> {
    pub application_name: String,
    pub subsystem_name: String,
    pub computer_name: Option<String>,
    pub body: B,
    pub class_name: Option<String>,
    pub method_name: Option<String>,
    pub thread_id: Option<String>,
    pub category: Option<String>,
}

impl<B> From<LogSinglesEntry<B>> for TestSingleEntry<B> {
    fn from(entry: LogSinglesEntry<B>) -> Self {
        Self {
            application_name: entry.application_name,
            subsystem_name: entry.subsystem_name,
            computer_name: entry.computer_name,
            body: entry.body,
            class_name: entry.class_name,
            method_name: entry.method_name,
            thread_id: entry.thread_id,
            category: entry.category,
        }
    }
}
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TestEntries<B> {
    entries: Vec<TestSingleEntry<B>>,
}

impl<B> From<LogSinglesRequest<B>> for TestEntries<B> {
    fn from(request: LogSinglesRequest<B>) -> Self {
        Self {
            entries: request
                .entries
                .into_iter()
                .map(|entry| entry.into())
                .collect(),
        }
    }
}

// TestExporter captures the request and auth data passed to the exporter
struct MockExporter {
    assert_fn: Box<dyn Fn(String) + Send + Sync>,
}

impl MockExporter {
    fn new(assert_fn: impl Fn(String) + Send + Sync + 'static) -> Self {
        Self {
            assert_fn: Box::new(assert_fn),
        }
    }
}

#[async_trait]
impl LogExporter for MockExporter {
    async fn export_bulk<B>(
        &self,
        request: LogBulkRequest<B>,
        _: &AuthData,
    ) -> Result<(), cx_sdk_rest_logs::Error>
    where
        B: Serialize + Send + Sync,
    {
        println!("export_bulk");
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
        let entries: TestEntries<B> = request.into();
        let s = serde_json::to_string(&entries)?;
        (self.assert_fn.as_ref())(s);
        Ok(())
    }
}

lazy_static! {
    static ref TEST_MUTEX: Mutex<()> = Mutex::new(());
}

// testcase macro used to create test functions
macro_rules! test_event_case {
    (
        name: $name:ident, 
        env: { $($key:expr => $value:expr),* $(,)? },
        event: $event:expr, 
        combined_event: $combined_event:ident,
        event_type: $event_type:ident,
        assert_fn: $assert_fn:expr
    ) => {
        #[tokio::test]
        async fn $name() {
            let _lock = TEST_MUTEX.lock().unwrap();
            $(
                std::env::set_var($key, $value);
            )*
            let aws_config = aws_config::load_defaults(BehaviorVersion::v2023_11_09()).await;
            let s3_client = Client::new(&aws_config);
            let config = Config::load_from_env().unwrap();


            let evt: $event_type = serde_json::from_str($event).expect("failed to parse s3_event");
            let exporter = Arc::new(MockExporter::new($assert_fn));
            let combined_event = CombinedEvent::$combined_event(evt);
            let event = LambdaEvent::new(combined_event, Context::default());

            coralogix_aws_shipper::function_handler(&s3_client, exporter, &config, event)
                .await
                .unwrap();

            // unset env vars
            $(
                std::env::remove_var($key);
            )*
            drop(_lock);
        }
    };
}

fn s3event_string(bucket: &str, key: &str) -> String {
    format!(r#"{{
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
    }}"#, bucket, bucket, key)
}


// -- implement tests -- //

test_event_case!(
    name: test_s3_event,
    env: {
        "CORALOGIX_API_KEY" => "1234456789X",
        "APP_NAME" => "integration-testing",
        "CORALOGIX_ENDPOINT" => "localhost:8080",
        "SAMPLING" => "1",
        "INTEGRATION_TYPE" => "S3",
        "AWS_REGION" => "eu-central-1",
    },
    event: r#"{
        "Records": [
            {
            "eventVersion": "2.0",
            "eventSource": "aws:s3",
            "awsRegion": "eu-west-1",
            "eventTime": "1970-01-01T00:00:00.000Z",
            "eventName": "ObjectCreated:Put",
            "userIdentity": {
                "principalId": "EXAMPLE"
            },
            "requestParameters": {
                "sourceIPAddress": "127.0.0.1"
            },
            "responseElements": {
                "x-amz-request-id": "EXAMPLE123456789",
                "x-amz-id-2": "EXAMPLE123/5678abcdefghijklambdaisawesome/mnopqrstuvwxyzABCDEFGH"
            },
            "s3": {
                "s3SchemaVersion": "1.0",
                "configurationId": "testConfigRule",
                "bucket": {
                "name": "coralogix-serverless-repo",
                "ownerIdentity": {
                    "principalId": "EXAMPLE"
                },
                "arn": "arn:aws:s3:::example-bucket"
                },
                "object": {
                "key": "coralogix-aws-shipper/s3.log",
                "size": 311000048,
                "eTag": "0123456789abcdef0123456789abcdef",
                "sequencer": "0A1B2C3D4E5F678901"
                }
            }
            }
        ]
    }"#,
    combined_event: S3,
    event_type: S3Event,
    assert_fn: |s| {
        let req: TestEntries<String> = serde_json::from_str(&s).unwrap();
        assert!(req.entries.len() == 4);
        assert!(req.entries[0].application_name == "integration-testing");
        assert!(req.entries[0].subsystem_name == "coralogix-serverless-repo");
        
        let log_lines = vec![
            "172.17.0.1 - - [26/Oct/2023:11:01:10 +0000] \"GET / HTTP/1.1\" 304 0 \"-\" \"Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/118.0.0.0 Safari/537.36\" \"-\"",
            "172.17.0.1 - - [26/Oct/2023:11:29:33 +0000] \"GET / HTTP/1.1\" 304 0 \"-\" \"Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/118.0.0.0 Safari/537.36\" \"-\"",
            "172.17.0.1 - - [26/Oct/2023:11:34:52 +0000] \"GET / HTTP/1.1\" 304 0 \"-\" \"Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/118.0.0.0 Safari/537.36\" \"-\"",
            "172.17.0.1 - - [26/Oct/2023:11:57:06 +0000] \"GET / HTTP/1.1\" 304 0 \"-\" \"Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/118.0.0.0 Safari/537.36\" \"-\"",
        ];

        for (i, log_line) in log_lines.iter().enumerate() {
            assert!(req.entries[i].body == *log_line);
        }
    }
);

test_event_case!(
    name: test_cloudtraillogs_s3_event,
    env: {
        "CORALOGIX_API_KEY" => "1234456789X",
        "APP_NAME" => "integration-testing",
        "CORALOGIX_ENDPOINT" => "localhost:8080",
        "SAMPLING" => "1",
        "AWS_REGION" => "eu-central-1",
        "INTEGRATION_TYPE" => "CloudTrail"
    },
    event: s3event_string("coralogix-serverless-repo", "coralogix-aws-shipper/cloudtrail.log.gz").as_str(),
    combined_event: S3,
    event_type: S3Event,
    assert_fn: |s| {
        let req: TestEntries<String> = serde_json::from_str(&s).unwrap();
        println!("records: {:?}", req.entries.len());
        assert!(req.entries.len() == 20);
        assert!(req.entries[0].application_name == "integration-testing");
        assert!(req.entries[0].subsystem_name == "coralogix-serverless-repo");
        let log_lines = vec![
            "{\"additionalEventData\":{\"AuthenticationMethod\":\"AuthHeader\",\"CipherSuite\":\"ECDHE-RSA-AES128-GCM-SHA256\",\"SignatureVersion\":\"SigV4\",\"bytesTransferredIn\":0,\"bytesTransferredOut\":480,\"x-amz-id-2\":\"1z7a7FcycBJ1A+larL8G04ZyJ3noJ823M3XEMt02L1jPF+QCGCBtudtO82vouBkJ+10K2jbfhA4=\"},\"awsRegion\":\"eu-central-1\",\"eventCategory\":\"Management\",\"eventID\":\"8d50073e-6d4f-4380-918e-cc17dd847be5\",\"eventName\":\"GetBucketAcl\",\"eventSource\":\"s3.amazonaws.com\",\"eventTime\":\"2023-10-17T04:53:21Z\",\"eventType\":\"AwsApiCall\",\"eventVersion\":\"1.09\",\"managementEvent\":true,\"readOnly\":true,\"recipientAccountId\":\"597078901540\",\"requestID\":\"JTT53K6AS8TR39ER\",\"requestParameters\":{\"Host\":\"aws-cloudtrail-logs-597078901540-082ac93e.s3.eu-central-1.amazonaws.com\",\"acl\":\"\",\"bucketName\":\"aws-cloudtrail-logs-597078901540-082ac93e\"},\"resources\":[{\"ARN\":\"arn:aws:s3:::aws-cloudtrail-logs-597078901540-082ac93e\",\"accountId\":\"597078901540\",\"type\":\"AWS::S3::Bucket\"}],\"responseElements\":null,\"sharedEventID\":\"d0264078-8cb0-45eb-8b53-32a8469888af\",\"sourceIPAddress\":\"cloudtrail.amazonaws.com\",\"userAgent\":\"cloudtrail.amazonaws.com\",\"userIdentity\":{\"invokedBy\":\"cloudtrail.amazonaws.com\",\"type\":\"AWSService\"}}",
            "{\"awsRegion\":\"eu-central-1\",\"eventCategory\":\"Management\",\"eventID\":\"5fb5255f-7ad4-4bc6-a0a3-0ab6d1113b8c\",\"eventName\":\"GenerateDataKey\",\"eventSource\":\"kms.amazonaws.com\",\"eventTime\":\"2023-10-17T04:53:24Z\",\"eventType\":\"AwsApiCall\",\"eventVersion\":\"1.08\",\"managementEvent\":true,\"readOnly\":true,\"recipientAccountId\":\"597078901540\",\"requestID\":\"4e67281e-2ae9-4515-b75f-e56cc6873220\",\"requestParameters\":{\"encryptionContext\":{\"aws:cloudtrail:arn\":\"arn:aws:cloudtrail:eu-central-1:597078901540:trail/Mytrail\",\"aws:s3:arn\":\"arn:aws:s3:::aws-cloudtrail-logs-597078901540-082ac93e/AWSLogs/597078901540/CloudTrail/eu-west-1/2023/10/17/597078901540_CloudTrail_eu-west-1_20231017T0450Z_KREKSWgLUgTraBu8.json.gz\"},\"keyId\":\"arn:aws:kms:eu-central-1:597078901540:key/a339d1af-e88e-4801-8d64-5c7861a4405f\",\"keySpec\":\"AES_256\"},\"resources\":[{\"ARN\":\"arn:aws:kms:eu-central-1:597078901540:key/a339d1af-e88e-4801-8d64-5c7861a4405f\",\"accountId\":\"597078901540\",\"type\":\"AWS::KMS::Key\"}],\"responseElements\":null,\"sharedEventID\":\"a2999d91-c0d3-4037-9171-93e6a1a08e53\",\"sourceIPAddress\":\"cloudtrail.amazonaws.com\",\"userAgent\":\"cloudtrail.amazonaws.com\",\"userIdentity\":{\"invokedBy\":\"cloudtrail.amazonaws.com\",\"type\":\"AWSService\"}}",
            "{\"additionalEventData\":{\"AuthenticationMethod\":\"AuthHeader\",\"CipherSuite\":\"ECDHE-RSA-AES128-GCM-SHA256\",\"SignatureVersion\":\"SigV4\",\"bytesTransferredIn\":0,\"bytesTransferredOut\":480,\"x-amz-id-2\":\"q2Jj4jfv73eSK1oWlBOTMMPsCU0YhMcYUcXrCi8W8s4NZfzPEgW9xrSmpir1iMIrV+zs0kR2MwE=\"},\"awsRegion\":\"eu-central-1\",\"eventCategory\":\"Management\",\"eventID\":\"d7ced48b-0d40-43ba-a78c-25b4389654c0\",\"eventName\":\"GetBucketAcl\",\"eventSource\":\"s3.amazonaws.com\",\"eventTime\":\"2023-10-17T04:53:26Z\",\"eventType\":\"AwsApiCall\",\"eventVersion\":\"1.09\",\"managementEvent\":true,\"readOnly\":true,\"recipientAccountId\":\"597078901540\",\"requestID\":\"19XECQVKGPN8JJ6D\",\"requestParameters\":{\"Host\":\"aws-cloudtrail-logs-597078901540-082ac93e.s3.eu-central-1.amazonaws.com\",\"acl\":\"\",\"bucketName\":\"aws-cloudtrail-logs-597078901540-082ac93e\"},\"resources\":[{\"ARN\":\"arn:aws:s3:::aws-cloudtrail-logs-597078901540-082ac93e\",\"accountId\":\"597078901540\",\"type\":\"AWS::S3::Bucket\"}],\"responseElements\":null,\"sharedEventID\":\"f513090d-b111-40e5-940a-81b0f98fe916\",\"sourceIPAddress\":\"cloudtrail.amazonaws.com\",\"userAgent\":\"cloudtrail.amazonaws.com\",\"userIdentity\":{\"invokedBy\":\"cloudtrail.amazonaws.com\",\"type\":\"AWSService\"}}"
        ];
       
        for (i, log_line) in log_lines.iter().enumerate() {
            assert!(req.entries[i].body == *log_line);
        }
    }
);

test_event_case!(
    name: test_csv_s3_event,
    env: {
        "CORALOGIX_API_KEY" => "1234456789X",
        "APP_NAME" => "integration-testing",
        "CORALOGIX_ENDPOINT" => "localhost:8080",
        "SAMPLING" => "1",
        "AWS_REGION" => "eu-central-1",
        "INTEGRATION_TYPE" => "S3Csv"
    },
    event: s3event_string("coralogix-serverless-repo", "coralogix-aws-shipper/s3csv.log").as_str(),
    combined_event: S3,
    event_type: S3Event,
    assert_fn: |s| {
        println!("{s}");
        let req: TestEntries<String> = serde_json::from_str(&s).unwrap();
        assert!(req.entries.len() == 2);
        assert!(req.entries[0].application_name == "integration-testing");
        assert!(req.entries[0].subsystem_name == "coralogix-serverless-repo");
        let log_lines = vec![
            "{\"id\":1,\"message\":\"This is an info message\",\"severity\":\"INFO\",\"timestamp\":\"2019-01-01 00:00:00\"}",
            "{\"id\":2,\"message\":\"This is another info message\",\"severity\":\"INFO\",\"timestamp\":\"2019-01-01 00:00:01\"}"
        ];

        for (i, log_line) in log_lines.iter().enumerate() {
            assert!(req.entries[i].body == *log_line);
        }
    }
);

test_event_case!(
    name: test_vpcflowlgos_s3_event,
    env: {
        "CORALOGIX_API_KEY" => "1234456789X",
        "APP_NAME" => "integration-testing",
        "CORALOGIX_ENDPOINT" => "localhost:8080",
        "SAMPLING" => "1",
        "AWS_REGION" => "eu-central-1",
        "INTEGRATION_TYPE" => "VpcFlow"
    },
    event: r#"{
        "Records": [
            {
            "eventVersion": "2.0",
            "eventSource": "aws:s3",
            "awsRegion": "eu-west-1",
            "eventTime": "1970-01-01T00:00:00.000Z",
            "eventName": "ObjectCreated:Put",
            "userIdentity": {
                "principalId": "EXAMPLE"
            },
            "requestParameters": {
                "sourceIPAddress": "127.0.0.1"
            },
            "responseElements": {
                "x-amz-request-id": "EXAMPLE123456789",
                "x-amz-id-2": "EXAMPLE123/5678abcdefghijklambdaisawesome/mnopqrstuvwxyzABCDEFGH"
            },
            "s3": {
                "s3SchemaVersion": "1.0",
                "configurationId": "testConfigRule",
                "bucket": {
                "name": "coralogix-serverless-repo",
                "ownerIdentity": {
                    "principalId": "EXAMPLE"
                },
                "arn": "arn:aws:s3:::example-bucket"
                },
                "object": {
                "key": "coralogix-aws-shipper/vpcflow.log.gz",
                "size": 311000048,
                "eTag": "0123456789abcdef0123456789abcdef",
                "sequencer": "0A1B2C3D4E5F678901"
                }
            }
            }
        ]
    }"#,
    combined_event: S3,
    event_type: S3Event,
    assert_fn: |s| {
        let req: TestEntries<String> = serde_json::from_str(&s).unwrap();
        assert!(req.entries.len() == 2);
        assert!(req.entries[0].application_name == "integration-testing");
        assert!(req.entries[0].subsystem_name == "coralogix-serverless-repo");
        let log_lines = vec![
            "{\"account-id\":\"123456789012\",\"action\":\"ACCEPT\",\"bytes\":4096,\"dstaddr\":\"172.31.9.12\",\"dstport\":3389,\"end\":1418530070,\"interface-id\":\"eni-abc123de\",\"log-status\":\"OK\",\"packets\":20,\"protocol\":6,\"srcaddr\":\"172.31.9.69\",\"srcport\":49761,\"start\":1418530010,\"version\":2}",
            "{\"account-id\":\"123456789012\",\"action\":\"ACCEPT\",\"bytes\":5060,\"dstaddr\":\"172.31.9.21\",\"dstport\":3389,\"end\":1418530070,\"interface-id\":\"eni-abc123de\",\"log-status\":\"OK\",\"packets\":20,\"protocol\":6,\"srcaddr\":\"172.31.9.69\",\"srcport\":49761,\"start\":1418530010,\"version\":2}",
        ];
        for (i, log_line) in log_lines.iter().enumerate() {
            assert!(req.entries[i].body == *log_line);
        };
    }
);

test_event_case!(
    name: test_sns_s3_event,
    env: {
        "CORALOGIX_API_KEY" => "1234456789X",
        "APP_NAME" => "integration-testing",
        "CORALOGIX_ENDPOINT" => "localhost:8080",
        "SAMPLING" => "1",
        "SUB_NAME" => "lambda",
        "INTEGRATION_TYPE" => "Sns",
        "AWS_REGION" => "eu-central-1",
    },
    event: r#"
        {
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
    combined_event: Sns,
    event_type: SnsEvent,
    assert_fn: |s| {
        let req: TestEntries<String> = serde_json::from_str(&s).unwrap();
        assert!(req.entries.len() == 1);
        assert!(req.entries[0].body == "[INFO] some test log line");
        assert!(req.entries[0].application_name == "integration-testing");
        assert!(req.entries[0].subsystem_name == "lambda");
    }
);

test_event_case!(
    name: test_cloudwatchlogs_event,
    env: {
        "CORALOGIX_API_KEY" => "1234456789X",
        "APP_NAME" => "integration-testing",
        "CORALOGIX_ENDPOINT" => "localhost:8080",
        "SAMPLING" => "1",
        "SUB_NAME" => "testLogGroup",
        "INTEGRATION_TYPE" => "CloudWatch",
        "AWS_REGION" => "eu-central-1",
    },
    event: r#"{
        "data": "H4sIAAAAAAAAAHWPwQqCQBCGX0Xm7EFtK+smZBEUgXoLCdMhFtKV3akI8d0bLYmibvPPN3wz00CJxmQnTO41whwWQRIctmEcB6sQbFC3CjW3XW8kxpOpP+OC22d1Wml1qZkQGtoMsScxaczKN3plG8zlaHIta5KqWsozoTYw3/djzwhpLwivWFGHGpAFe7DL68JlBUk+l7KSN7tCOEJ4M3/qOI49vMHj+zCKdlFqLaU2ZHV2a4Ct/an0/ivdX8oYc1UVX860fQDQiMdxRQEAAA=="
    }"#,

    combined_event: CloudWatchLogs,
    event_type: AwsLogs,
    assert_fn: |s| {
        let req: TestEntries<String> = serde_json::from_str(&s).unwrap();
        assert!(req.entries.len() == 2);
        assert!(req.entries[0].body == "[ERROR] First test message");
        assert!(req.entries[1].body == "[ERROR] Second test message");
        assert!(req.entries[0].application_name == "integration-testing");
        assert!(req.entries[0].subsystem_name == "testLogGroup");
    }
);

