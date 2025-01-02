use wiremock;
use async_trait::async_trait;
// use wiremock::matchers::{method, path, body};
use serde_json;
use coralogix_aws_shipper::metrics::config::Config;
use lambda_runtime::{Context, LambdaEvent};
use coralogix_aws_shipper::events;

async fn run_test_firehose_transform_flow() {

    let event = serde_json::json!({
      "invocationId": "invocation-id-123456",
      "deliveryStreamArn": "arn:aws:firehose:region:account-id:deliverystream/stream-name",
      "region": "us-east-1",
      "records": [
        {
          "recordId": "record-id-1234",
          "approximateArrivalTimestamp": 1701234567,
          "data": "eyJtZXRyaWMiOiAiQ1BVVXNhZ2UiLCAidmFsdWUiOiAxMjMuNDU2LCAidGltZXN0YW1wIjogIjIwMjQtMDYtMTFUMTM6MDA6MDBaIn0="
        },
        {
          "recordId": "record-id-5678",
          "approximateArrivalTimestamp": 1701234568,
          "data": "eyJtZXRyaWMiOiAiTGF0ZW5jeSIgLCAidmFsdWUiOiAzMjAuNzgsICJ0aW1lc3RhbXAiOiAiMjAyNC0wNi0xMVQxMzowMDowMFoifQ=="
        }
      ]
    });

    let bytes = vec![130, 94, 10, 255, 93, 10, 186, 1, 10, 23, 10, 14, 99, 108, 111, 117, 100, 46, 112, 114, 111, 118, 105, 100, 101, 114, 18, 5, 10, 3, 97, 119, 115, 10, 34, 10, 16, 99, 108, 111, 117, 100, 46, 97, 99, 99, 111, 117, 110, 116, 46, 105, 100, 18, 14, 10, 12, 55, 55, 49, 48, 51, 57, 54, 52, 57, 52, 52, 48, 10, 27, 10, 12, 99, 108, 111, 117, 100, 46, 114, 101, 103, 105, 111, 110, 18, 11, 10, 9, 101, 117, 45, 119, 101, 115, 116, 45, 49];


    let result = String::from_utf8(bytes.clone());
    match result {
        Ok(string) => println!("String: {}", string),
        Err(e) => println!("Invalid UTF-8: {}", e),
    }

    // let event = serde_json::json!({
    //   "invocationId": "595b024c-949a-40c1-ae67-dbc825afc66c",
    //   "deliveryStreamArn": "arn:aws:firehose:eu-west-1:771039649440:deliverystream/MetricStreams-QuickFull-KKn5QV-nJq6VxcV",
    //   "sourceKinesisStreamArn": null,
    //   "region": "eu-west-1",
    //   "records": [
    //     {
    //       "recordId": "shardId-00000000000000000000000000000000000000000000000000000000000000000000000000000000000049657715769015733549787243891873794809635777967011397634000000000000",
    //       "approximateArrivalTimestamp": "2024-12-17T13:24:07.033Z",
    //       "data": "eyJtZXRyaWMiOiAiTGF0ZW5jeSIgLCAidmFsdWUiOiAzMjAuNzgsICJ0aW1lc3RhbXAiOiAiMjAyNC0wNi0xMVQxMzowMDowMFoifQ=="
    //     }
    //   ]
    // });

    let config = Config::load_from_env().unwrap();
    let evt: events::Combined = serde_json::from_value(event).unwrap();
    let evt = LambdaEvent::new(evt, Context::default());
    coralogix_aws_shipper::metrics::handler(&config, evt).await.unwrap();

}


#[tokio::test]
async fn test_firehose_transform_flow() {
    let mock_server = wiremock::MockServer::start().await;

    wiremock::Mock::given(wiremock::matchers::method("POST"))
        .and(wiremock::matchers::path("/"))
        .and(wiremock::matchers::body_json(r#"{"a": "b"}"#)) 
        .respond_with(wiremock::ResponseTemplate::new(200))
        .mount(&mock_server)
        .await;


    temp_env::async_with_vars(
        [
            ("CORALOGIX_API_KEY", Some("1234456789X")),
            ("APP_NAME", Some(mock_server.uri().as_str())),
            ("CORALOGIX_DOMAIN", Some("localhost:8080")),
            ("INTEGRATION_TYPE", Some("S3")),
            ("AWS_REGION", Some("eu-central-1")),
            ("TELEMETRY_MODE", Some("metrics")),
        ],
        run_test_firehose_transform_flow(),
    )
    .await;
}
