use std::time::Duration;

use cx_sdk_otlp::{
    auth::AuthData,
    config::{BackoffConfig, ChannelConfig},
    logs::OtlpLogExporterGrpc,
    otlp::proto::collector::logs::v1::ExportLogsServiceRequest,
    OtlpExporter,
};

#[tokio::test]
async fn builds_otlp_logs_exporter_with_both_auth_modes() {
    let exporter = OtlpLogExporterGrpc::builder()
        .with_channel_config(ChannelConfig::new("http://127.0.0.1:4317".to_string()))
        .with_backoff_config(BackoffConfig {
            initial_delay: Duration::from_millis(1),
            max_delay: Duration::from_millis(2),
            max_elapsed_time: Duration::from_millis(3),
        })
        .try_build()
        .unwrap();

    let collector_auth = AuthData::default();
    let collector_future =
        exporter.export(ExportLogsServiceRequest::default(), &collector_auth);
    drop(collector_future);

    let api_key = cx_sdk_otlp::ApiKey::from("compile-only");
    let coralogix_auth = AuthData::from(&api_key);
    let coralogix_future =
        exporter.export(ExportLogsServiceRequest::default(), &coralogix_auth);
    drop(coralogix_future);
}
