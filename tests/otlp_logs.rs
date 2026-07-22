use std::{
    convert::Infallible,
    sync::{Arc, Mutex},
    task::{Context, Poll},
};

use opentelemetry_proto::tonic::collector::logs::v1::{
    logs_service_server::{LogsService, LogsServiceServer},
    ExportLogsServiceRequest, ExportLogsServiceResponse,
};
use opentelemetry_proto::tonic::{common::v1::any_value, logs::v1::SeverityNumber};
use tokio::sync::oneshot;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::{
    body::BoxBody, codec::CompressionEncoding, codegen::Service, metadata::MetadataMap,
    server::NamedService, Request, Response, Status,
};

const GRPC_ENCODING_HEADER: &str = "grpc-encoding";

#[derive(Default)]
struct Captured {
    requests: Mutex<Vec<ExportLogsServiceRequest>>,
    metadata: Mutex<Vec<MetadataMap>>,
    wire_grpc_encodings: Mutex<Vec<Option<String>>>,
}

/// Observes inbound `grpc-encoding` on the raw HTTP/2 request before Tonic decodes it.
#[derive(Clone)]
struct WireEncodingCaptureService<S> {
    inner: S,
    captured: Arc<Captured>,
}

impl<S> WireEncodingCaptureService<S> {
    fn new(inner: S, captured: Arc<Captured>) -> Self {
        Self { inner, captured }
    }
}

impl<S> NamedService for WireEncodingCaptureService<S>
where
    S: NamedService,
{
    const NAME: &'static str = S::NAME;
}

impl<S> Service<http::Request<BoxBody>> for WireEncodingCaptureService<S>
where
    S: Service<http::Request<BoxBody>, Response = http::Response<BoxBody>, Error = Infallible>
        + Clone
        + Send
        + 'static,
    S::Future: Send + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = S::Future;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: http::Request<BoxBody>) -> Self::Future {
        let encoding = req
            .headers()
            .get(GRPC_ENCODING_HEADER)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        self.captured
            .wire_grpc_encodings
            .lock()
            .unwrap()
            .push(encoding);
        self.inner.call(req)
    }
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

const SENTINEL_SECRET: &str = "sentinel-server-controlled-secret";

#[derive(Clone)]
struct FailingService;

#[tonic::async_trait]
impl LogsService for FailingService {
    async fn export(
        &self,
        _request: Request<ExportLogsServiceRequest>,
    ) -> Result<Response<ExportLogsServiceResponse>, Status> {
        Err(Status::permission_denied(SENTINEL_SECRET))
    }
}

async fn start_collector(
    accept_gzip: bool,
) -> (
    String,
    Arc<Captured>,
    oneshot::Sender<()>,
    tokio::task::JoinHandle<()>,
) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let incoming = TcpListenerStream::new(listener);
    let captured = Arc::new(Captured::default());
    let service = LogsServiceServer::new(CaptureService(captured.clone()));
    let service = if accept_gzip {
        service.accept_compressed(CompressionEncoding::Gzip)
    } else {
        service
    };
    let service = WireEncodingCaptureService::new(service, captured.clone());
    let (shutdown_tx, shutdown_rx) = oneshot::channel();

    let server = tokio::spawn(async move {
        tonic::transport::Server::builder()
            .add_service(service)
            .serve_with_incoming_shutdown(incoming, async {
                let _ = shutdown_rx.await;
            })
            .await
            .unwrap();
    });

    (format!("http://{address}"), captured, shutdown_tx, server)
}

async fn start_failing_collector() -> (String, oneshot::Sender<()>, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let incoming = TcpListenerStream::new(listener);
    let (shutdown_tx, shutdown_rx) = oneshot::channel();

    let server = tokio::spawn(async move {
        tonic::transport::Server::builder()
            .add_service(
                LogsServiceServer::new(FailingService).accept_compressed(CompressionEncoding::Gzip),
            )
            .serve_with_incoming_shutdown(incoming, async {
                let _ = shutdown_rx.await;
            })
            .await
            .unwrap();
    });

    (format!("http://{address}"), shutdown_tx, server)
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

fn exporter_config(
    export: coralogix_aws_shipper::logs::config::LogExportConfig,
) -> coralogix_aws_shipper::logs::config::Config {
    use coralogix_aws_shipper::logs::config::{Config, IntegrationType};

    Config {
        newline_pattern: String::new(),
        blocking_pattern: String::new(),
        log_stream_filter: None,
        sampling: 1,
        logs_per_batch: 500,
        integration_type: IntegrationType::S3,
        app_name: None,
        sub_name: None,
        export,
        max_elapsed_time: 2,
        csv_delimiter: ",".to_string(),
        batches_max_size: 4,
        batches_max_concurrency: 1,
        add_metadata: String::new(),
        dlq_arn: None,
        dlq_url: None,
        dlq_retry_limit: None,
        dlq_s3_bucket: None,
        lambda_assume_role: None,
        starlark_script: None,
        enable_log_group_tags: false,
        log_group_tags_cache_ttl_seconds: 300,
    }
}

fn assert_wire_grpc_encoding_gzip(captured: &Captured) {
    let encodings = captured.wire_grpc_encodings.lock().unwrap();
    assert_eq!(encodings.len(), 1);
    assert_eq!(
        encodings[0].as_deref(),
        Some("gzip"),
        "direct Coralogix route must send grpc-encoding: gzip before Tonic decodes the body"
    );
}

fn assert_wire_grpc_encoding_uncompressed(captured: &Captured) {
    let encodings = captured.wire_grpc_encodings.lock().unwrap();
    assert_eq!(encodings.len(), 1);
    match encodings[0].as_deref() {
        None | Some("identity") => {}
        Some(other) => panic!(
            "collector route must not send compressed grpc-encoding at the wire level, got: {other:?}"
        ),
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
async fn collector_route_succeeds_without_gzip_support() {
    use coralogix_aws_shipper::logs::{config::LogExportConfig, exporter::build_exporter};

    let (endpoint, captured, shutdown, server) = start_collector(false).await;
    let exporter = build_exporter(&exporter_config(LogExportConfig::CollectorOtlpGrpc {
        endpoint,
    }))
    .unwrap();

    exporter.export(vec![test_log()]).await.unwrap();

    assert_standard_otlp_payload(&captured);
    assert_wire_grpc_encoding_uncompressed(&captured);
    assert!(captured.metadata.lock().unwrap()[0]
        .get("authorization")
        .is_none());
    stop_collector(shutdown, server).await;
}

#[tokio::test]
async fn sends_bearer_authorization_for_direct_coralogix_otlp() {
    use coralogix_aws_shipper::logs::{config::LogExportConfig, exporter::build_exporter};

    let (endpoint, captured, shutdown, server) = start_collector(true).await;
    let exporter = build_exporter(&exporter_config(LogExportConfig::CoralogixOtlpGrpc {
        endpoint,
        api_key: "direct-secret".to_string().into(),
    }))
    .unwrap();

    exporter.export(vec![test_log()]).await.unwrap();

    assert_standard_otlp_payload(&captured);
    assert_wire_grpc_encoding_gzip(&captured);
    {
        let metadata = captured.metadata.lock().unwrap();
        let authorization: Vec<_> = metadata[0]
            .get_all("authorization")
            .iter()
            .map(|value| value.to_str().unwrap())
            .collect();
        assert_eq!(authorization, ["Bearer direct-secret"]);
    }
    stop_collector(shutdown, server).await;
}

#[tokio::test]
async fn propagated_otlp_failure_excludes_server_message() {
    use coralogix_aws_shipper::logs::exporter::{otlp::OtlpGrpcExporter, LogExporter};
    use cx_sdk_otlp::auth::AuthData;

    let (endpoint, shutdown, server) = start_failing_collector().await;
    let exporter =
        OtlpGrpcExporter::new(endpoint, AuthData::default(), 2, 4 * 1024 * 1024).unwrap();

    let error = exporter.export(vec![test_log()]).await.unwrap_err();
    let display = error.to_string();
    let debug = format!("{error:?}");

    assert!(display.contains("client_error"));
    assert!(display.contains("permission_denied"));
    assert!(debug.contains("client_error"));
    assert!(debug.contains("permission_denied"));
    assert!(!display.contains(SENTINEL_SECRET));
    assert!(!debug.contains(SENTINEL_SECRET));
    stop_collector(shutdown, server).await;
}
