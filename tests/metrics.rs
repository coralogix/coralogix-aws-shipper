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
          "approximateArrivalTimestamp": 1701234567.123,
          "data": "eyJtZXRyaWMiOiAiQ1BVVXNhZ2UiLCAidmFsdWUiOiAxMjMuNDU2LCAidGltZXN0YW1wIjogIjIwMjQtMDYtMTFUMTM6MDA6MDBaIn0="
        },
        {
          "recordId": "record-id-5678",
          "approximateArrivalTimestamp": 1701234568.456,
          "data": "eyJtZXRyaWMiOiAiTGF0ZW5jeSIgLCAidmFsdWUiOiAzMjAuNzgsICJ0aW1lc3RhbXAiOiAiMjAyNC0wNi0xMVQxMzowMDowMFoifQ=="
        }
      ]
    });

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
