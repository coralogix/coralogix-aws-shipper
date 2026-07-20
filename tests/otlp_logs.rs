use std::sync::{Arc, Mutex};

use opentelemetry_proto::tonic::collector::logs::v1::{
    logs_service_server::{LogsService, LogsServiceServer},
    ExportLogsServiceRequest, ExportLogsServiceResponse,
};
use opentelemetry_proto::tonic::{common::v1::any_value, logs::v1::SeverityNumber};
use tokio::sync::oneshot;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::{codec::CompressionEncoding, metadata::MetadataMap, Request, Response, Status};

#[derive(Default)]
struct Captured {
    requests: Mutex<Vec<ExportLogsServiceRequest>>,
    metadata: Mutex<Vec<MetadataMap>>,
}

#[derive(Clone)]
struct CaptureService(Arc<Captured>);

#[tonic::async_trait]
impl LogsService for CaptureService {
    async fn export(
        &self,
        request: Request<ExportLogsServiceRequest>,
    ) -> Result<Response<ExportLogsServiceResponse>, Status> {
        self.0
            .metadata
            .lock()
            .unwrap()
            .push(request.metadata().clone());
        self.0.requests.lock().unwrap().push(request.into_inner());
        Ok(Response::new(ExportLogsServiceResponse::default()))
    }
}

async fn start_collector() -> (
    String,
    Arc<Captured>,
    oneshot::Sender<()>,
    tokio::task::JoinHandle<()>,
) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let incoming = TcpListenerStream::new(listener);
    let captured = Arc::new(Captured::default());
    let service = CaptureService(captured.clone());
    let (shutdown_tx, shutdown_rx) = oneshot::channel();

    let server = tokio::spawn(async move {
        tonic::transport::Server::builder()
            .add_service(
                LogsServiceServer::new(service).accept_compressed(CompressionEncoding::Gzip),
            )
            .serve_with_incoming_shutdown(incoming, async {
                let _ = shutdown_rx.await;
            })
            .await
            .unwrap();
    });

    (format!("http://{address}"), captured, shutdown_tx, server)
}

async fn stop_collector(shutdown: oneshot::Sender<()>, server: tokio::task::JoinHandle<()>) {
    shutdown.send(()).unwrap();
    server.await.unwrap();
}

fn test_log() -> coralogix_aws_shipper::logs::model::ProcessedLog {
    use coralogix_aws_shipper::logs::model::{LogSeverity, ProcessedLog};

    ProcessedLog {
        application_name: "application".to_string(),
        subsystem_name: "subsystem".to_string(),
        body: serde_json::json!({"message": "hello"}),
        severity: LogSeverity::Info,
        timestamp: time::OffsetDateTime::UNIX_EPOCH,
    }
}

fn assert_standard_otlp_payload(captured: &Captured) {
    let requests = captured.requests.lock().unwrap();
    assert_eq!(requests.len(), 1);

    let resource_logs = &requests[0].resource_logs;
    assert_eq!(resource_logs.len(), 1);
    let resource = resource_logs[0]
        .resource
        .as_ref()
        .expect("resource must be present");
    assert_eq!(resource.attributes.len(), 2);
    assert_eq!(resource.attributes[0].key, "cx.application.name");
    assert!(matches!(
        resource.attributes[0]
            .value
            .as_ref()
            .and_then(|value| value.value.as_ref()),
        Some(any_value::Value::StringValue(value)) if value == "application"
    ));
    assert_eq!(resource.attributes[1].key, "cx.subsystem.name");
    assert!(matches!(
        resource.attributes[1]
            .value
            .as_ref()
            .and_then(|value| value.value.as_ref()),
        Some(any_value::Value::StringValue(value)) if value == "subsystem"
    ));

    assert_eq!(resource_logs[0].scope_logs.len(), 1);
    let records = &resource_logs[0].scope_logs[0].log_records;
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].time_unix_nano, 0);
    assert_eq!(records[0].observed_time_unix_nano, 0);
    assert_eq!(records[0].severity_number, SeverityNumber::Info as i32);
    assert_eq!(records[0].severity_text, "Info");
    let body = records[0].body.as_ref().expect("log body must be present");
    let Some(any_value::Value::KvlistValue(body)) = body.value.as_ref() else {
        panic!("log body must preserve its structured JSON object");
    };
    assert_eq!(body.values.len(), 1);
    assert_eq!(body.values[0].key, "message");
    assert!(matches!(
        body.values[0]
            .value
            .as_ref()
            .and_then(|value| value.value.as_ref()),
        Some(any_value::Value::StringValue(value)) if value == "hello"
    ));
}

#[tokio::test]
async fn sends_standard_otlp_logs_without_authorization() {
    use coralogix_aws_shipper::logs::exporter::{otlp::OtlpGrpcExporter, LogExporter};
    use cx_sdk_otlp::auth::AuthData;

    let (endpoint, captured, shutdown, server) = start_collector().await;
    let exporter =
        OtlpGrpcExporter::new(endpoint, AuthData::default(), 2, 4 * 1024 * 1024).unwrap();

    exporter.export(vec![test_log()]).await.unwrap();

    assert_standard_otlp_payload(&captured);
    assert!(captured.metadata.lock().unwrap()[0]
        .get("authorization")
        .is_none());
    stop_collector(shutdown, server).await;
}

#[tokio::test]
async fn sends_bearer_authorization_for_direct_coralogix_otlp() {
    use coralogix_aws_shipper::logs::exporter::{otlp::OtlpGrpcExporter, LogExporter};
    use cx_sdk_otlp::{auth::AuthData, ApiKey};

    let (endpoint, captured, shutdown, server) = start_collector().await;
    let api_key = ApiKey::from("direct-secret");
    let exporter =
        OtlpGrpcExporter::new(endpoint, AuthData::from(&api_key), 2, 4 * 1024 * 1024).unwrap();

    exporter.export(vec![test_log()]).await.unwrap();

    assert_standard_otlp_payload(&captured);
    let metadata = captured.metadata.lock().unwrap();
    let authorization: Vec<_> = metadata[0]
        .get_all("authorization")
        .iter()
        .map(|value| value.to_str().unwrap())
        .collect();
    assert_eq!(authorization, ["Bearer direct-secret"]);
    drop(metadata);
    stop_collector(shutdown, server).await;
}
