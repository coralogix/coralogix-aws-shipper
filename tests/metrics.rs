use wiremock;
use serde_json;
use coralogix_aws_shipper::metrics::config::Config;
use lambda_runtime::{Context, LambdaEvent};
use coralogix_aws_shipper::events;
use std::sync::Arc;


use opentelemetry_proto::tonic::collector::metrics::v1::ExportMetricsServiceRequest; // Example path for trace data
use opentelemetry_proto::tonic::metrics::v1::metric::Data::Summary;

use opentelemetry_proto::tonic::common::v1::any_value;
use prost::Message;


async fn run_test_firehose_transform_flow() {
    let data = std::fs::read("./tests/fixtures/cwstream_metrics.json").unwrap();
    let config = Config::load_from_env().unwrap();
    let evt: events::Combined = serde_json::from_slice(&data).unwrap();
    let evt = LambdaEvent::new(evt, Context::default());
    coralogix_aws_shipper::metrics::handler(&config, evt).await.unwrap();
}


#[tokio::test]
async fn test_firehose_transform_flow() {

    let message_count = Arc::new(std::sync::Mutex::new(0));
    let counter = message_count.clone();
    // let (tx, mut rx) = tokio::sync::mpsc::channel::<Vec<u8>>(2);

    let server = wiremock::MockServer::start().await;
    wiremock::Mock::given(wiremock::matchers::method("POST"))
        .and(wiremock::matchers::path("/v1/metrics"))
        .and(move |r: &wiremock::Request| -> bool {

            // increment message count
            let mut count = counter.lock().unwrap();

            let buffer = r.body.clone();
            let metrics = ExportMetricsServiceRequest::decode(&*buffer).unwrap();

            for resource in metrics.resource_metrics {
                for scope_metrics in resource.scope_metrics {
                  for metric in scope_metrics.metrics.iter() {

                    assert!(metric.unit == "");
                    let data = metric.data.clone().ok_or("no metric data").unwrap();
                    // let summary = data.summary.ok_or("no summary").unwrap();
                    
                    match data {
                        Summary(summary) => {
                            summary.data_points.iter().for_each(|dp| {
                                let expected_sub_name = any_value::Value::StringValue("testsubsystem".to_string());
                                let expected_app_name = any_value::Value::StringValue("testapp".to_string());
                                
                                // asert cx.application.name is in labels
                                assert!(dp.attributes.iter().any(|label| {
                                    label.key == "cx.application.name"
                                }) == true);
                                
                                // asert cx.subsystem.name is in labels
                                assert!(dp.attributes.iter().any(|label| {
                                    label.key == "cx.subsystem.name"
                                }) == true);

                                dp.attributes.iter().for_each(|label| {
                                    if label.key == "cx.application.name" {
                                        let v = label.value.clone()
                                            .ok_or("no label value for metric")
                                            .unwrap()
                                            .value
                                            .unwrap();
                                        assert!(v == expected_app_name);                                   
                                    }

                                    if label.key == "cx.subsystem.name" {
                                        
                                        let v = label.value.clone()
                                            .ok_or("no label value for metric")
                                            .unwrap()
                                            .value
                                            .unwrap();

                                        assert!(v == expected_sub_name);                                    
                                    }
                                })
                            })
                        },
                        _ => continue
                    }
                  }
                }
            }
            *count += 1;
            true
        })
        .respond_with(wiremock::ResponseTemplate::new(200))
        .mount(&server)
        .await;


    temp_env::async_with_vars(
        [
            ("CORALOGIX_API_KEY", Some("1234456789X")),
            ("APP_NAME", Some("testapp")),
            ("SUB_NAME", Some("testsubsystem")),
            ("CORALOGIX_ENDPOINT", Some(server.uri().as_str())),
            ("INTEGRATION_TYPE", Some("S3")),
            ("AWS_REGION", Some("eu-central-1")),
            ("TELEMETRY_MODE", Some("metrics")),
        ],
        run_test_firehose_transform_flow(),
    )
    .await;

    assert_eq!(message_count.lock().unwrap().clone(), 2);
}

