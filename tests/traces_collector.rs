//! Collector-route delivery for Transaction Search spans: the metadata parsed from
//! OTLP_HEADERS must reach the collector, and the direct Coralogix route must keep
//! sending only its own credentials.

use std::sync::{Arc, Mutex};

use coralogix_aws_shipper::traces::{build_exporter, Config, Destination};
use cx_sdk_otlp::otlp::proto::collector::trace::v1::ExportTraceServiceRequest;
use cx_sdk_otlp::AuthorizedOtlpExporter;
use opentelemetry_proto::tonic::collector::trace::v1::trace_service_server::{
    TraceService, TraceServiceServer,
};
use tokio::sync::oneshot;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::{codec::CompressionEncoding, metadata::MetadataMap, Request, Response, Status};

#[derive(Default)]
struct Captured {
    metadata: Mutex<Vec<MetadataMap>>,
}

#[derive(Clone)]
struct CaptureService(Arc<Captured>);

#[tonic::async_trait]
impl TraceService for CaptureService {
    async fn export(
        &self,
        request: Request<
            opentelemetry_proto::tonic::collector::trace::v1::ExportTraceServiceRequest,
        >,
    ) -> Result<
        Response<opentelemetry_proto::tonic::collector::trace::v1::ExportTraceServiceResponse>,
        Status,
    > {
        self.0
            .metadata
            .lock()
            .unwrap()
            .push(request.metadata().clone());
        Ok(Response::new(
            opentelemetry_proto::tonic::collector::trace::v1::ExportTraceServiceResponse::default(),
        ))
    }
}

async fn start_trace_collector() -> (
    String,
    Arc<Captured>,
    oneshot::Sender<()>,
    tokio::task::JoinHandle<()>,
) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let incoming = TcpListenerStream::new(listener);
    let captured = Arc::new(Captured::default());
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let service = TraceServiceServer::new(CaptureService(captured.clone()))
        .accept_compressed(CompressionEncoding::Gzip);

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

async fn stop_collector(shutdown: oneshot::Sender<()>, server: tokio::task::JoinHandle<()>) {
    shutdown.send(()).unwrap();
    server.await.unwrap();
}

fn trace_config(destination: Destination, endpoint: String) -> Config {
    Config {
        destination,
        endpoint,
        app_name: "application".to_string(),
        sub_name: Some("subsystem".to_string()),
    }
}

#[tokio::test]
async fn collector_route_sends_custom_headers() {
    use coralogix_aws_shipper::logs::config::OtlpHeaders;

    let (endpoint, captured, shutdown, server) = start_trace_collector().await;
    let config = trace_config(
        Destination::Collector {
            headers: OtlpHeaders::parse("X-API-Key=gateway-secret;tenant=acme,prod").unwrap(),
        },
        endpoint,
    );
    let exporter = build_exporter(&config).unwrap();

    exporter
        .export(ExportTraceServiceRequest::default())
        .await
        .unwrap();

    {
        let metadata = captured.metadata.lock().unwrap();
        assert_eq!(metadata.len(), 1);
        assert_eq!(
            metadata[0].get("x-api-key").unwrap().to_str().unwrap(),
            "gateway-secret"
        );
        assert_eq!(
            metadata[0].get("tenant").unwrap().to_str().unwrap(),
            "acme,prod"
        );
        assert!(metadata[0].get("authorization").is_none());
    }
    stop_collector(shutdown, server).await;
}

#[tokio::test]
async fn direct_coralogix_route_sends_only_its_credentials() {
    let (endpoint, captured, shutdown, server) = start_trace_collector().await;
    let config = trace_config(
        Destination::Coralogix {
            api_key: "direct-secret".to_string().into(),
        },
        endpoint,
    );
    let exporter = build_exporter(&config).unwrap();

    exporter
        .export(ExportTraceServiceRequest::default())
        .await
        .unwrap();

    {
        let metadata = captured.metadata.lock().unwrap();
        assert_eq!(metadata.len(), 1);
        assert_eq!(
            metadata[0].get("authorization").unwrap().to_str().unwrap(),
            "Bearer direct-secret"
        );
        assert!(metadata[0].get("x-api-key").is_none());
        assert!(metadata[0].get("tenant").is_none());
    }
    stop_collector(shutdown, server).await;
}
