// Linux only. Trust is established through SSL_CERT_FILE, which
// rustls-platform-verifier honours via rustls-native-certs on Linux; on macOS it uses
// Apple's trust evaluation and on Windows the system store, neither of which reads it.
// The deployment target is Linux, and CI runs these in the AL2023 container. Gating the
// file rather than panicking inside keeps the rest free of unreachable code.
#![cfg(target_os = "linux")]

//! Runtime evidence that the shipped clients enforce the backported certificate
//! distrust, on the GovCloud dependency set.
//!
//! Compilation and upstream's regression suite establish that the patched
//! rustls-webpki rejects distrusted keys. They do not establish that the clients we
//! actually ship reach that code. These tests do, by standing up a local TLS server
//! and driving the real SDK against it.
//!
//! `#[ignore]` on purpose: they are meaningful only with the GovCloud inputs in place
//! (ci/govcloud/config.toml as .cargo/config.toml, ci/govcloud/Cargo.lock as
//! Cargo.lock). Run on mainline they would exercise upstream's own fix and pass
//! without saying anything about the backport. ci/govcloud/verify-build-inputs.sh
//! runs first in CI and asserts the configuration, then these are invoked explicitly
//! with --ignored.
//!
//! Trust is established the way the deployed client establishes it: SSL_CERT_FILE,
//! which rustls-platform-verifier honours via rustls-native-certs. The SDK builder
//! and verifier are untouched -- substituting a verifier would test something other
//! than what ships.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

/// Bytes of decrypted application data the server saw, across every connection
/// including retries. A rejected handshake must leave this at zero; the control case
/// proves the counter is wired up and can be non-zero.
#[derive(Default)]
pub struct Observed {
    pub app_bytes: AtomicUsize,
    /// Complete HTTP requests read (headers plus any declared body), not merely a
    /// non-empty socket read.
    pub requests: AtomicUsize,
    pub handshakes_attempted: AtomicUsize,
    pub handshakes_completed: AtomicUsize,
    /// Everything decrypted, so the test can assert the credential never appears.
    pub received: Mutex<Vec<u8>>,
}

impl Observed {
    pub fn app_bytes(&self) -> usize {
        self.app_bytes.load(Ordering::SeqCst)
    }
    pub fn requests(&self) -> usize {
        self.requests.load(Ordering::SeqCst)
    }
    pub fn handshakes_attempted(&self) -> usize {
        self.handshakes_attempted.load(Ordering::SeqCst)
    }
    pub fn handshakes_completed(&self) -> usize {
        self.handshakes_completed.load(Ordering::SeqCst)
    }
    pub fn saw(&self, needle: &str) -> bool {
        let got = self.received.lock().expect("received buffer poisoned");
        got.windows(needle.len()).any(|w| w == needle.as_bytes())
    }
}

/// A TLS server presenting `chain`, counting what it decrypts.
///
/// Serves until dropped, so retried connections are counted too -- a rejection must
/// leave the counters at zero across every attempt, not just the first.
/// Credentials that must never leave the client. Distinctive so a byte-level search
/// of what the server received is meaningful.
pub const DUMMY_API_KEY: &str = "cx-distrust-test-key-MUST-NOT-BE-SENT";

/// The real eMudhra intermediate carrying a distrusted SPKI, extracted from the
/// published affected chain. See tests/data/distrusted/README.md for provenance.
pub const AFFECTED_INTERMEDIATE: &[u8] = include_bytes!("data/distrusted/emudhra-intermediate.der");

pub fn affected_intermediate_is_distrusted() -> bool {
    // Compare the certificate's actual SubjectPublicKeyInfo, parsed, against the eight
    // pinned keys. A byte search over the whole certificate would also match the same
    // bytes appearing in an extension, which would not mean the key is distrusted.
    let cert = rustls::pki_types::CertificateDer::from(AFFECTED_INTERMEDIATE);
    let mut store = rustls::RootCertStore::empty();
    store
        .add(cert)
        .expect("fixture is not a parseable certificate");
    let anchor = store.roots.first().expect("no trust anchor parsed");
    let spki = anchor.subject_public_key_info.as_ref();

    let spki_dir = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/vendor/rustls-webpki/src/data/distrusted"
    );
    std::fs::read_dir(spki_dir)
        .expect("vendored distrusted SPKIs are missing")
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().is_some_and(|x| x == "der"))
        .any(|e| std::fs::read(e.path()).is_ok_and(|pinned| pinned == spki))
}

/// Classify why a TLS attempt failed.
///
/// Preference order, because the stronger evidence may not survive the client stack:
///   1. a typed rustls InvalidCertificate(Other(..)) wrapping webpki's CertDistrusted
///   2. failing that, CertDistrusted appearing in the rendered chain
///
/// BadSignature must not satisfy this: the synthetic chain's signature is invalid
/// under the distrusted issuer, so accepting it would let the test pass without the
/// backport doing anything.
pub fn rejected_for_distrust(err: &(dyn std::error::Error + 'static)) -> Outcome {
    let mut rendered = String::new();
    let mut cursor: Option<&(dyn std::error::Error + 'static)> = Some(err);
    while let Some(e) = cursor {
        rendered.push_str(&format!("{e}\n"));
        if let Some(rustls_err) = e.downcast_ref::<rustls::Error>() {
            return match rustls_err {
                rustls::Error::InvalidCertificate(rustls::CertificateError::Other(other)) => {
                    if format!("{other:?}").contains("CertDistrusted") {
                        Outcome::Distrusted { typed: true }
                    } else {
                        Outcome::OtherTlsError(format!("{rustls_err:?}"))
                    }
                }
                rustls::Error::InvalidCertificate(rustls::CertificateError::BadSignature) => {
                    Outcome::BadSignature
                }
                other => Outcome::OtherTlsError(format!("{other:?}")),
            };
        }
        cursor = e.source();
    }

    // Order matters: reject first, accept last. Text mentioning both markers is
    // ambiguous and must not be read as a distrust rejection.
    if rendered.contains("BadSignature") {
        Outcome::BadSignature
    } else if rendered.contains("CertDistrusted") {
        Outcome::Distrusted { typed: false }
    } else {
        Outcome::OtherTlsError(rendered)
    }
}

#[derive(Debug)]
pub enum Outcome {
    /// Rejected for distrust. `typed` records whether the rustls error survived or
    /// only its rendered text did.
    Distrusted {
        typed: bool,
    },
    /// Rejected on the signature instead -- the synthetic chain tripped on its own
    /// invalidity rather than on the backport.
    BadSignature,
    OtherTlsError(String),
}

#[test]
fn fixture_still_carries_a_distrusted_key() {
    assert!(
        affected_intermediate_is_distrusted(),
        "tests/data/distrusted/emudhra-intermediate.der no longer contains any of the \
         SPKIs pinned in vendor/rustls-webpki/src/data/distrusted -- the distrust tests \
         would pass without testing anything"
    );
}

pub async fn spawn_tls_server(
    chain: Vec<rustls::pki_types::CertificateDer<'static>>,
    key: rustls::pki_types::PrivateKeyDer<'static>,
) -> Server {
    spawn_tls_server_with_body(chain, key, String::new()).await
}

/// As `spawn_tls_server`, but replying with `body` so a client that parses the
/// response can complete its call. The control must genuinely succeed; a response the
/// client rejects would leave "did the request arrive" entangled with "did it parse".
pub async fn spawn_tls_server_with_body(
    chain: Vec<rustls::pki_types::CertificateDer<'static>>,
    key: rustls::pki_types::PrivateKeyDer<'static>,
    body: String,
) -> Server {
    let config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(chain, key)
        .expect("server config");
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let port = listener.local_addr().expect("local addr").port();

    let observed = Arc::new(Observed::default());
    let seen = Arc::clone(&observed);
    let handlers: Arc<Mutex<Vec<tokio::task::JoinHandle<()>>>> = Arc::default();
    let track = Arc::clone(&handlers);
    let stop = Arc::new(tokio::sync::Notify::new());
    let stopped = Arc::clone(&stop);

    let accept_loop = tokio::spawn(async move {
        loop {
            let accepted = tokio::select! {
                _ = stopped.notified() => return,
                r = listener.accept() => r,
            };
            let Ok((stream, _)) = accepted else { return };
            let acceptor = acceptor.clone();
            let seen = Arc::clone(&seen);
            let body = body.clone();
            let handle = tokio::spawn(async move {
                seen.handshakes_attempted.fetch_add(1, Ordering::SeqCst);
                // A rejected handshake ends here: nothing below runs, so no
                // application byte can be counted.
                let Ok(mut tls) = acceptor.accept(stream).await else {
                    return;
                };
                seen.handshakes_completed.fetch_add(1, Ordering::SeqCst);
                read_request(&mut tls, &seen).await;
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/xml\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = tls.write_all(response.as_bytes()).await;
                let _ = tls.shutdown().await;
            });
            track.lock().expect("handler list poisoned").push(handle);
        }
    });

    Server {
        port,
        observed,
        accept_loop,
        handlers,
        stop,
    }
}

/// Read one complete HTTP request: headers, then any declared body. Counting a
/// non-empty socket read would overstate -- a partial read is not a request.
async fn read_request<S>(tls: &mut S, seen: &Arc<Observed>)
where
    S: tokio::io::AsyncRead + Unpin,
{
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    let headers_end = loop {
        match tls.read(&mut chunk).await {
            Ok(0) | Err(_) => return,
            Ok(n) => {
                seen.app_bytes.fetch_add(n, Ordering::SeqCst);
                buf.extend_from_slice(&chunk[..n]);
                if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                    break i + 4;
                }
                if buf.len() > 1024 * 1024 {
                    return;
                }
            }
        }
    };

    let header_text = String::from_utf8_lossy(&buf[..headers_end]).to_lowercase();
    let content_length = header_text
        .lines()
        .find_map(|l| l.strip_prefix("content-length:"))
        .and_then(|v| v.trim().parse::<usize>().ok())
        .unwrap_or(0);

    while buf.len() - headers_end < content_length {
        match tls.read(&mut chunk).await {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                seen.app_bytes.fetch_add(n, Ordering::SeqCst);
                buf.extend_from_slice(&chunk[..n]);
            }
        }
    }

    if buf.len() - headers_end >= content_length {
        seen.requests.fetch_add(1, Ordering::SeqCst);
    }
    seen.received
        .lock()
        .expect("received buffer poisoned")
        .extend_from_slice(&buf);
}

pub struct Server {
    pub port: u16,
    pub observed: Arc<Observed>,
    accept_loop: tokio::task::JoinHandle<()>,
    handlers: Arc<Mutex<Vec<tokio::task::JoinHandle<()>>>>,
    stop: Arc<tokio::sync::Notify>,
}

impl Server {
    /// Stop accepting and wait for every accepted connection to finish observing.
    ///
    /// Must be called before asserting zero bytes: a detached handler that has not
    /// finished yet would report zero for the wrong reason.
    pub async fn finish(self) -> Arc<Observed> {
        let Server {
            observed,
            accept_loop,
            handlers,
            stop,
            ..
        } = self;

        // Await the accept loop to completion before draining: abort() only requests
        // cancellation, so a loop still running could register another handler after
        // the list was taken, and that connection would go unobserved.
        stop.notify_waiters();
        accept_loop.abort();
        match accept_loop.await {
            Ok(()) => {}
            Err(e) if e.is_cancelled() => {}
            Err(e) => panic!("accept loop panicked: {e}"),
        }

        // Only now is the list final.
        let handles = std::mem::take(&mut *handlers.lock().expect("handler list poisoned"));
        for handle in handles {
            match tokio::time::timeout(std::time::Duration::from_secs(10), handle).await {
                // Only a handler that ran to completion has finished observing. A
                // cancelled one proves nothing, so unlike the accept loop -- which is
                // aborted deliberately -- cancellation is a failure here.
                Ok(Ok(())) => {}
                Ok(Err(e)) if e.is_cancelled() => {
                    panic!("a connection handler was cancelled before it finished observing")
                }
                Ok(Err(e)) => panic!("a connection handler panicked: {e}"),
                // Detaching here would hand back counters the handler had not
                // finished writing, so a zero could mean "not observed yet".
                Err(_) => panic!(
                    "a connection handler did not finish within 10s; the observed \
                     counters would be incomplete"
                ),
            }
        }

        observed
    }
}

/// A control CA and a leaf for `localhost`, trusted through the client's own root
/// loading rather than by substituting a verifier.
pub struct ControlPki {
    pub ca_pem: String,
    pub chain: Vec<rustls::pki_types::CertificateDer<'static>>,
    pub key: rustls::pki_types::PrivateKeyDer<'static>,
}

/// Generated once per process and shared by every case.
///
/// aws-smithy-http-client caches native roots in a LazyLock, loaded on first use and
/// never reloaded, so a later SSL_CERT_FILE change does not reach it. If each case
/// generated its own CA, whichever ran first would seed the cache and the others would
/// be verified against the wrong trust store -- a result that depends on test order.
/// Serialisation prevents overlap, not caching.
static CONTROL: std::sync::OnceLock<ControlMaterial> = std::sync::OnceLock::new();

struct ControlMaterial {
    ca_pem: String,
    leaf_der: Vec<u8>,
    ca_der: Vec<u8>,
    key_der: Vec<u8>,
}

pub fn control_pki() -> ControlPki {
    let material = CONTROL.get_or_init(generate_control_material);
    ControlPki {
        ca_pem: material.ca_pem.clone(),
        chain: vec![
            rustls::pki_types::CertificateDer::from(material.leaf_der.clone()),
            rustls::pki_types::CertificateDer::from(material.ca_der.clone()),
        ],
        key: rustls::pki_types::PrivateKeyDer::try_from(material.key_der.clone())
            .expect("leaf key der"),
    }
}

fn generate_control_material() -> ControlMaterial {
    use rcgen::{CertificateParams, DnType, KeyPair};

    let mut ca_params = CertificateParams::new(vec![]).expect("ca params");
    ca_params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
    ca_params
        .distinguished_name
        .push(DnType::CommonName, "cx distrust test CA");
    let ca_key = KeyPair::generate().expect("ca key");
    let ca_cert = ca_params
        .clone()
        .self_signed(&ca_key)
        .expect("self-signed ca");
    let ca_issuer = rcgen::Issuer::new(ca_params, &ca_key);

    let mut leaf_params =
        CertificateParams::new(vec!["localhost".to_string()]).expect("leaf params");
    leaf_params
        .distinguished_name
        .push(DnType::CommonName, "localhost");
    let leaf_key = KeyPair::generate().expect("leaf key");
    let leaf = leaf_params
        .signed_by(&leaf_key, &ca_issuer)
        .expect("leaf signed by ca");

    ControlMaterial {
        ca_pem: ca_cert.pem(),
        leaf_der: leaf.der().to_vec(),
        ca_der: ca_cert.der().to_vec(),
        key_der: leaf_key.serialize_der(),
    }
}

/// Point the process's root loading at `ca_pem` only.
///
/// Linux only. rustls-platform-verifier uses Apple's trust evaluation on macOS and
/// the Windows store on Windows; neither reads SSL_CERT_FILE, so these tests are
/// meaningful only on the deployment target. They assert that rather than failing
/// with an opaque platform trust error.
///
/// rustls-platform-verifier loads roots through rustls-native-certs on Linux, which
/// honours SSL_CERT_FILE and SSL_CERT_DIR. Setting the file and clearing the
/// directory gives the real client a trust store containing exactly our control CA,
/// without replacing the verifier the SDK builds.
///
/// Must run before any client is constructed: roots are read once, when the verifier
/// is first built.
pub fn trust_only(ca_pem: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("control-ca.pem");
    std::fs::write(&path, ca_pem).expect("write ca");
    std::env::set_var("SSL_CERT_FILE", &path);
    std::env::remove_var("SSL_CERT_DIR");
    dir
}

fn sample_log() -> coralogix_aws_shipper::logs::model::ProcessedLog {
    use coralogix_aws_shipper::logs::model::{LogSeverity, ProcessedLog};
    ProcessedLog {
        application_name: "cx-distrust-test".to_string(),
        subsystem_name: "control".to_string(),
        body: serde_json::json!({"msg": "distrust runtime test"}),
        severity: LogSeverity::Info,
        timestamp: time::OffsetDateTime::now_utc(),
    }
}

/// Control: the real REST SDK client completes a request against a chain it trusts.
///
/// This is what makes the rejection case meaningful. Without it, zero application
/// bytes would be indistinguishable from a server that never counts anything.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires the GovCloud build inputs; invoked explicitly in CI"]
#[serial_test::serial(govcloud_distrust)]
async fn rest_sdk_control_succeeds_and_is_observed() {
    use coralogix_aws_shipper::logs::exporter::LogExporter;

    let pki = control_pki();
    // Before any client exists: roots are read when the verifier is first built.
    // serial: trust_only sets process-wide environment, so cases must not overlap.
    let _trust = trust_only(&pki.ca_pem);

    let server = spawn_tls_server(pki.chain, pki.key).await;
    let port = server.port;

    // After construction, and the installed default rather than a fresh candidate:
    // the locked reqwest prefers the process-wide provider, which the server builder
    // above installs.
    assert!(
        rustls::crypto::CryptoProvider::get_default()
            .expect("no process-wide crypto provider installed")
            .fips(),
        "the selected crypto provider does not report FIPS -- this is not the \
         GovCloud configuration"
    );

    let exporter = coralogix_aws_shipper::logs::exporter::rest::CoralogixRestExporter::new(
        format!("https://localhost:{port}"),
        cx_sdk_rest_logs::auth::ApiKey::from(DUMMY_API_KEY.to_string()),
        1,
    )
    .expect("build rest exporter");

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        exporter.export(vec![sample_log()]),
    )
    .await
    .expect("control export timed out");

    let observed = server.finish().await;

    assert!(result.is_ok(), "control export failed: {result:?}");
    assert_eq!(
        observed.handshakes_completed(),
        1,
        "expected exactly one completed handshake"
    );
    assert!(
        observed.app_bytes() > 0,
        "server recorded no application bytes -- the counter is not wired up, so a \
         zero in the rejection case would prove nothing"
    );
    assert_eq!(
        observed.requests(),
        1,
        "expected exactly one complete HTTP request"
    );
    assert!(
        observed.saw(DUMMY_API_KEY),
        "the control did not carry the credential, so its absence in the rejection \
         case would prove nothing"
    );
}

/// A chain whose path building must reach the distrusted intermediate.
///
/// The leaf names the eMudhra intermediate as its issuer, and that real certificate
/// is supplied alongside it, so path building takes the intermediate as a candidate
/// and `build_chain` tests its SPKI before verifying any signature.
///
/// Limitation, stated because it bounds what this proves: we do not hold the
/// compromised private key, so the leaf is not validly signed by that issuer. If the
/// distrust check were removed, this chain would fail on the signature instead. The
/// test therefore requires `CertDistrusted` specifically and fails on `BadSignature`
/// -- that distinction is what makes it evidence about the backport rather than
/// about certificate validation generally.
pub fn affected_chain() -> (
    Vec<rustls::pki_types::CertificateDer<'static>>,
    rustls::pki_types::PrivateKeyDer<'static>,
) {
    use rcgen::{CertificateParams, DistinguishedName, DnType, DnValue, Issuer, KeyPair};

    let intermediate = rustls::pki_types::CertificateDer::from(AFFECTED_INTERMEDIATE);
    let mut store = rustls::RootCertStore::empty();
    store
        .add(intermediate.clone())
        .expect("affected intermediate parses");
    let want_subject = store
        .roots
        .first()
        .expect("anchor")
        .subject
        .as_ref()
        .to_vec();

    // Reconstruct the intermediate's subject exactly -- C=NL PrintableString,
    // O and CN UTF8String, in that order. Path building matches an issuer by
    // comparing these DER bytes, so an encoding difference means the intermediate is
    // never considered and the distrust check is never reached. Asserted below.
    let mut dn = DistinguishedName::new();
    dn.push(
        DnType::CountryName,
        DnValue::PrintableString("NL".try_into().expect("country")),
    );
    dn.push(
        DnType::OrganizationName,
        DnValue::Utf8String("eMudhra BV".to_string()),
    );
    dn.push(
        DnType::CommonName,
        DnValue::Utf8String("EM DV TLS CA - G3B".to_string()),
    );

    let mut issuer_params = CertificateParams::default();
    issuer_params.distinguished_name = dn;
    issuer_params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
    // A stand-in key: we do not hold the compromised private key. The leaf therefore
    // carries the right issuer NAME but a signature that would not verify under the
    // real intermediate -- see the limitation note above.
    let stand_in_key = KeyPair::generate().expect("stand-in key");
    let stand_in = issuer_params
        .clone()
        .self_signed(&stand_in_key)
        .expect("stand-in cert");

    let mut check = rustls::RootCertStore::empty();
    check.add(stand_in.der().clone()).expect("stand-in parses");
    let got_subject = check
        .roots
        .first()
        .expect("anchor")
        .subject
        .as_ref()
        .to_vec();
    assert_eq!(
        got_subject, want_subject,
        "the reconstructed issuer DN does not match the affected intermediate's \
         subject, so path building would never consider it and the test would \
         report UnknownIssuer instead of distrust"
    );

    let issuer = Issuer::new(issuer_params, &stand_in_key);
    let mut leaf_params =
        CertificateParams::new(vec!["localhost".to_string()]).expect("leaf params");
    leaf_params
        .distinguished_name
        .push(DnType::CommonName, "localhost");
    let leaf_key = KeyPair::generate().expect("leaf key");
    let leaf = leaf_params
        .signed_by(&leaf_key, &issuer)
        .expect("leaf named by the distrusted issuer");

    (
        vec![leaf.der().clone(), intermediate],
        rustls::pki_types::PrivateKeyDer::try_from(leaf_key.serialize_der()).expect("key"),
    )
}

/// The rejection case: the same client and trust setup, an affected chain, and
/// nothing sent.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires the GovCloud build inputs; invoked explicitly in CI"]
#[serial_test::serial(govcloud_distrust)]
async fn rest_sdk_rejects_distrusted_chain_before_sending_anything() {
    use coralogix_aws_shipper::logs::exporter::LogExporter;

    let pki = control_pki();
    let _trust = trust_only(&pki.ca_pem);

    let (chain, key) = affected_chain();
    let server = spawn_tls_server(chain, key).await;
    let port = server.port;

    let exporter = coralogix_aws_shipper::logs::exporter::rest::CoralogixRestExporter::new(
        format!("https://localhost:{port}"),
        cx_sdk_rest_logs::auth::ApiKey::from(DUMMY_API_KEY.to_string()),
        1,
    )
    .expect("build rest exporter");

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(60),
        exporter.export(vec![sample_log()]),
    )
    .await
    .expect("rejection case timed out");

    let err = result.expect_err("the affected chain was accepted");
    let outcome = rejected_for_distrust(&err);

    // Await every connection, including retries, before reading the counters.
    let observed = server.finish().await;

    match outcome {
        Outcome::Distrusted { typed } => {
            if !typed {
                eprintln!(
                    "note: matched CertDistrusted in rendered text; the typed \
                     rustls::Error did not survive the client stack"
                );
            }
        }
        Outcome::BadSignature => panic!(
            "rejected on the signature, not distrust -- the synthetic chain tripped \
             on its own invalidity, so this says nothing about the backport: {err:?}"
        ),
        Outcome::OtherTlsError(detail) => {
            panic!("rejected, but not for distrust: {detail}\nfull error: {err:?}")
        }
    }

    assert!(
        observed.handshakes_attempted() > 0,
        "the client never reached the server"
    );
    assert_eq!(
        observed.handshakes_completed(),
        0,
        "a handshake completed against an affected chain"
    );
    assert_eq!(
        observed.app_bytes(),
        0,
        "application data reached the server across {} connection attempt(s)",
        observed.handshakes_attempted()
    );
    assert_eq!(observed.requests(), 0, "a request reached the server");
    assert!(
        !observed.saw(DUMMY_API_KEY),
        "the credential reached the server"
    );
}

/// Minimal response the S3 client will accept, so the control genuinely succeeds.
const LIST_BUCKETS_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<ListAllMyBucketsResult xmlns="http://s3.amazonaws.com/doc/2006-03-01/">
  <Owner><ID>cx-distrust-test</ID><DisplayName>cx</DisplayName></Owner>
  <Buckets></Buckets>
</ListAllMyBucketsResult>"#;

/// Credential the AWS SDK signs with. Must never leave the client on rejection.
const DUMMY_ACCESS_KEY_ID: &str = "AKIACXDISTRUSTTESTKEY";

/// An S3 client over the shipper's own HTTPS stack.
///
/// Builds on `aws_runtime::load_sdk_config`, the function the Lambda itself calls, so
/// the TLS provider is the one that ships rather than one reconstructed here. With
/// ENABLE_AWS_FIPS set it selects CryptoMode::AwsLcFips, which is the GovCloud
/// configuration. Only the endpoint and credentials are overridden.
async fn s3_client(port: u16) -> aws_sdk_s3::Client {
    std::env::set_var("ENABLE_AWS_FIPS", "true");
    // Keep configuration loading local and predictable: no IMDS round trip, and a
    // region that does not depend on the environment the test runs in.
    std::env::set_var("AWS_EC2_METADATA_DISABLED", "true");
    std::env::set_var("AWS_REGION", "us-gov-west-1");
    assert!(
        coralogix_aws_shipper::aws_runtime::aws_fips_enabled(),
        "the shipper does not consider FIPS enabled, so this would not be the \
         GovCloud TLS configuration"
    );
    let sdk_config = coralogix_aws_shipper::aws_runtime::load_sdk_config().await;

    let creds = aws_sdk_s3::config::Credentials::new(
        DUMMY_ACCESS_KEY_ID,
        "cx-distrust-test-secret-MUST-NOT-BE-SENT",
        None,
        None,
        "cx-distrust-test",
    );
    let config = aws_sdk_s3::config::Builder::from(&sdk_config)
        .region(aws_sdk_s3::config::Region::new("us-gov-west-1"))
        .credentials_provider(creds)
        .endpoint_url(format!("https://localhost:{port}"))
        .force_path_style(true)
        .build();
    aws_sdk_s3::Client::from_conf(config)
}

/// Control for the AWS SDK path: the client completes a call it trusts.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires the GovCloud build inputs; invoked explicitly in CI"]
#[serial_test::serial(govcloud_distrust)]
async fn aws_sdk_control_succeeds_and_is_observed() {
    let pki = control_pki();
    let _trust = trust_only(&pki.ca_pem);

    let server = spawn_tls_server_with_body(pki.chain, pki.key, LIST_BUCKETS_XML.to_string()).await;
    let port = server.port;

    let result = tokio::time::timeout(std::time::Duration::from_secs(30), async move {
        s3_client(port).await.list_buckets().send().await
    })
    .await
    .expect("aws sdk control timed out");

    let observed = server.finish().await;

    assert!(result.is_ok(), "aws sdk control failed: {result:?}");
    assert_eq!(observed.handshakes_completed(), 1);
    assert!(
        observed.app_bytes() > 0,
        "server recorded no application bytes -- a zero in the rejection case would \
         prove nothing"
    );
    assert_eq!(observed.requests(), 1);
    assert!(
        observed.saw(DUMMY_ACCESS_KEY_ID),
        "the control did not carry the credential, so its absence in the rejection \
         case would prove nothing"
    );
}

/// The AWS SDK path rejects an affected chain and sends nothing.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires the GovCloud build inputs; invoked explicitly in CI"]
#[serial_test::serial(govcloud_distrust)]
async fn aws_sdk_rejects_distrusted_chain_before_sending_anything() {
    let pki = control_pki();
    let _trust = trust_only(&pki.ca_pem);

    let (chain, key) = affected_chain();
    let server = spawn_tls_server_with_body(chain, key, LIST_BUCKETS_XML.to_string()).await;
    let port = server.port;

    let result = tokio::time::timeout(std::time::Duration::from_secs(60), async move {
        s3_client(port).await.list_buckets().send().await
    })
    .await
    .expect("aws sdk rejection case timed out");

    let err = result.expect_err("the affected chain was accepted");
    let outcome = rejected_for_distrust(&err);

    let observed = server.finish().await;

    match outcome {
        Outcome::Distrusted { typed } => {
            if !typed {
                eprintln!(
                    "note: matched CertDistrusted in rendered text; the typed \
                     rustls::Error did not survive the client stack"
                );
            }
        }
        Outcome::BadSignature => panic!(
            "rejected on the signature, not distrust -- says nothing about the \
             backport: {err:?}"
        ),
        Outcome::OtherTlsError(detail) => {
            panic!("rejected, but not for distrust: {detail}\nfull error: {err:?}")
        }
    }

    assert!(
        observed.handshakes_attempted() > 0,
        "never reached the server"
    );
    assert_eq!(observed.handshakes_completed(), 0);
    assert_eq!(
        observed.app_bytes(),
        0,
        "application data reached the server across {} attempt(s)",
        observed.handshakes_attempted()
    );
    assert_eq!(observed.requests(), 0);
    assert!(
        !observed.saw(DUMMY_ACCESS_KEY_ID),
        "the credential reached the server"
    );
}

// ---------------------------------------------------------------------------------
// OTLP configuration
//
// Unlike REST and AWS, OTLP trusts the compiled-in webpki root bundle rather than the
// system store (src/logs/exporter/otlp.rs -> ChannelConfig::with_webpki_roots), so
// SSL_CERT_FILE does not reach it. The two cases therefore differ deliberately:
//
//   rejection: the production configuration, unmodified. The distrust check runs
//              before issuer resolution -- upstream asserts this with empty trust
//              anchors -- so no added trust is needed for the chain to be rejected
//              for the right reason.
//   control:   the SDK's own endpoint construction, with the test CA added alongside
//              the bundled roots. tls_config replaces rather than merges, so the
//              bundled roots are re-applied explicitly. This trust difference is
//              deliberate and is stated in the PR.

/// gRPC metadata the server decoded, per request.
///
/// HTTP/2 header compression makes a byte search of the decrypted stream unreliable,
/// so the credential is checked here rather than through `Observed::saw`.
#[derive(Default)]
pub struct GrpcObserved {
    pub requests: AtomicUsize,
    pub metadata: Mutex<Vec<String>>,
}

impl GrpcObserved {
    pub fn requests(&self) -> usize {
        self.requests.load(Ordering::SeqCst)
    }
    pub fn saw_metadata(&self, needle: &str) -> bool {
        self.metadata
            .lock()
            .expect("metadata poisoned")
            .iter()
            .any(|m| m.contains(needle))
    }
}

/// Records transport errors before the SDK converts or the shipper sanitises them.
///
/// `MapErrLayer` maps errors from both `poll_ready` and the request future, so a
/// failure in either is observed, and the error is returned unchanged -- the export
/// still fails exactly as it would in production.
///
/// Needed because `sanitized_otlp_error` reduces a transport failure to a
/// classification and a gRPC status name, so `CertDistrusted` never reaches
/// `LogExportError`. Observing inside the SDK's layer stack keeps credentials, retries
/// and the production channel configuration in the export under test.
pub fn observing_layer(
    seen: Arc<Mutex<Vec<Outcome>>>,
) -> tower::util::MapErrLayer<impl Fn(tonic::transport::Error) -> tonic::transport::Error + Clone> {
    tower::util::MapErrLayer::new(move |err: tonic::transport::Error| {
        seen.lock()
            .expect("observations poisoned")
            .push(rejected_for_distrust(&err));
        err
    })
}

/// Counts decrypted bytes read off a TLS stream.
///
/// The HTTP/1 helper counts in `read_request`, but tonic owns the gRPC stream and
/// reads it itself, so without this the OTLP `app_bytes` assertion would be vacuous:
/// zero because nothing ever counted, not because nothing was sent. The control
/// asserts a non-zero count, which is what makes the rejection's zero mean something.
pub struct Counting<S> {
    inner: S,
    seen: Arc<Observed>,
}

impl<S: tokio::io::AsyncRead + Unpin> tokio::io::AsyncRead for Counting<S> {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        let me = self.get_mut();
        let before = buf.filled().len();
        let polled = std::pin::Pin::new(&mut me.inner).poll_read(cx, buf);
        if let std::task::Poll::Ready(Ok(())) = &polled {
            let read = buf.filled().len() - before;
            me.seen.app_bytes.fetch_add(read, Ordering::SeqCst);
        }
        polled
    }
}

impl<S: tokio::io::AsyncWrite + Unpin> tokio::io::AsyncWrite for Counting<S> {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        std::pin::Pin::new(&mut self.get_mut().inner).poll_write(cx, buf)
    }
    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.get_mut().inner).poll_flush(cx)
    }
    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.get_mut().inner).poll_shutdown(cx)
    }
}

impl<S> tonic::transport::server::Connected for Counting<S> {
    type ConnectInfo = ();
    fn connect_info(&self) -> Self::ConnectInfo {}
}

/// A real OTLP logs endpoint over TLS, with ALPN h2.
///
/// The HTTP/1 helper above cannot establish a gRPC control, so this serves the
/// generated LogsService. It shares the same TLS acceptor pattern, so a rejected
/// handshake still never reaches the service and the byte counters stay at zero.
pub struct GrpcServer {
    pub port: u16,
    pub observed: Arc<Observed>,
    pub grpc: Arc<GrpcObserved>,
    /// Stops the accept loop. Separate from tonic's own shutdown so that a handshake
    /// already in flight is never dropped: the loop only checks this between accepts.
    stop_accepting: tokio::sync::oneshot::Sender<()>,
    shutdown: tokio::sync::oneshot::Sender<()>,
    accepting: tokio::task::JoinHandle<()>,
    serving: tokio::task::JoinHandle<Result<(), tonic::transport::Error>>,
}

impl GrpcServer {
    /// Finalises observations.
    ///
    /// The accept loop is stopped and joined *first*, so every handshake it began has
    /// resolved -- counted as completed or rejected -- before any counter is read. Only
    /// then is tonic told to shut down. Cancellation is not accepted on either task: a
    /// cancelled task has observed an unknown amount, which would silently weaken the
    /// rejection case's zero assertions.
    pub async fn finish(self) -> (Arc<Observed>, Arc<GrpcObserved>) {
        let _ = self.stop_accepting.send(());
        match tokio::time::timeout(std::time::Duration::from_secs(10), self.accepting).await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => panic!("the accept loop did not finish cleanly: {e}"),
            Err(_) => panic!("the accept loop did not stop within 10s"),
        }

        let _ = self.shutdown.send(());
        match tokio::time::timeout(std::time::Duration::from_secs(10), self.serving).await {
            Ok(Ok(Ok(()))) => {}
            Ok(Ok(Err(e))) => panic!("the grpc server failed: {e}"),
            Ok(Err(e)) => panic!("the grpc server did not finish cleanly: {e}"),
            Err(_) => panic!("the grpc server did not stop within 10s"),
        }
        (self.observed, self.grpc)
    }
}

struct LogsEndpoint {
    grpc: Arc<GrpcObserved>,
}

#[tonic::async_trait]
impl opentelemetry_proto::tonic::collector::logs::v1::logs_service_server::LogsService
    for LogsEndpoint
{
    async fn export(
        &self,
        request: tonic::Request<
            opentelemetry_proto::tonic::collector::logs::v1::ExportLogsServiceRequest,
        >,
    ) -> Result<
        tonic::Response<opentelemetry_proto::tonic::collector::logs::v1::ExportLogsServiceResponse>,
        tonic::Status,
    > {
        // Decoded metadata, not raw bytes: HPACK makes a byte search unreliable.
        let rendered = format!("{:?}", request.metadata());
        self.grpc
            .metadata
            .lock()
            .expect("metadata poisoned")
            .push(rendered);
        self.grpc.requests.fetch_add(1, Ordering::SeqCst);
        Ok(tonic::Response::new(
            opentelemetry_proto::tonic::collector::logs::v1::ExportLogsServiceResponse {
                partial_success: None,
            },
        ))
    }
}

pub async fn spawn_grpc_server(
    chain: Vec<rustls::pki_types::CertificateDer<'static>>,
    key: rustls::pki_types::PrivateKeyDer<'static>,
) -> GrpcServer {
    let mut config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(chain, key)
        .expect("server config");
    // gRPC requires HTTP/2, which requires ALPN agreement.
    config.alpn_protocols = vec![b"h2".to_vec()];
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let port = listener.local_addr().expect("local addr").port();

    let observed = Arc::new(Observed::default());
    let grpc = Arc::new(GrpcObserved::default());
    let seen = Arc::clone(&observed);
    let grpc_seen = Arc::clone(&grpc);
    let (shutdown, shutdown_rx) = tokio::sync::oneshot::channel();
    let (stop_accepting, mut stop_rx) = tokio::sync::oneshot::channel();
    let (conns_tx, mut conns_rx) = tokio::sync::mpsc::channel(8);

    // Accepting runs in its own task rather than inside tonic's incoming stream.
    // tonic drops that stream the moment it shuts down, which could discard a
    // handshake still in progress and leave `handshakes_completed` understating what
    // actually happened. Here the shutdown signal is only observed between accepts,
    // so every handshake that starts runs to completion and is counted.
    let accepting = tokio::spawn(async move {
        loop {
            let stream = tokio::select! {
                biased;
                _ = &mut stop_rx => break,
                accepted = listener.accept() => match accepted {
                    Ok((stream, _)) => stream,
                    Err(_) => break,
                },
            };
            seen.handshakes_attempted.fetch_add(1, Ordering::SeqCst);
            // A rejected handshake ends here: the service is never reached, so no
            // request or metadata can be recorded.
            match acceptor.accept(stream).await {
                Ok(tls) => {
                    seen.handshakes_completed.fetch_add(1, Ordering::SeqCst);
                    let counted = Counting {
                        inner: tls,
                        seen: Arc::clone(&seen),
                    };
                    if conns_tx.send(counted).await.is_err() {
                        break;
                    }
                }
                Err(_) => continue,
            }
        }
    });

    let serving = tokio::spawn(async move {
        let incoming = async_stream::stream! {
            while let Some(tls) = conns_rx.recv().await {
                yield Ok::<_, std::io::Error>(tls);
            }
        };

        let service = opentelemetry_proto::tonic::collector::logs::v1::logs_service_server::LogsServiceServer::new(
            LogsEndpoint { grpc: grpc_seen },
        )
        // The SDK's exporter compresses; the real endpoint accepts gzip, so the
        // control must too, or it fails on encoding rather than on trust.
        .accept_compressed(tonic::codec::CompressionEncoding::Gzip);
        tonic::transport::Server::builder()
            .add_service(service)
            .serve_with_incoming_shutdown(incoming, async {
                let _ = shutdown_rx.await;
            })
            .await
    });

    GrpcServer {
        port,
        observed,
        grpc,
        stop_accepting,
        shutdown,
        accepting,
        serving,
    }
}

fn otlp_request() -> cx_sdk_otlp::otlp::proto::collector::logs::v1::ExportLogsServiceRequest {
    // The shipper's own request construction, and the SDK's re-exported proto types --
    // the shipper's direct opentelemetry-proto is a different crate instance.
    coralogix_aws_shipper::logs::exporter::otlp::build_export_request(&[sample_log()])
}

/// The credential travels the way it does in production: AuthData renders it into
/// gRPC metadata, rather than being injected as extra metadata by the test.
fn otlp_auth() -> cx_sdk_core::auth::AuthData {
    cx_sdk_core::auth::AuthData::from(&cx_sdk_core::auth::ApiKey::from(DUMMY_API_KEY.to_string()))
}

/// Control for the OTLP path.
///
/// Uses the SDK's own endpoint construction and exporter. The only deviation from
/// production is trust: the test CA is added alongside the bundled webpki roots,
/// because OTLP does not read SSL_CERT_FILE. tls_config replaces rather than merges,
/// so the bundled roots are re-applied explicitly.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires the GovCloud build inputs; invoked explicitly in CI"]
#[serial_test::serial(govcloud_distrust)]
async fn otlp_control_succeeds_and_is_observed() {
    use cx_sdk_otlp::OtlpExporter as _;
    let pki = control_pki();
    let server = spawn_grpc_server(pki.chain, pki.key).await;
    let port = server.port;

    let channel_config =
        cx_sdk_core::channel::ChannelConfig::new(format!("https://localhost:{port}"))
            .with_webpki_roots();
    let endpoint = channel_config
        .to_endpoint()
        .expect("sdk endpoint")
        .tls_config(
            tonic::transport::ClientTlsConfig::new()
                .with_webpki_roots()
                .ca_certificate(tonic::transport::Certificate::from_pem(&pki.ca_pem))
                .domain_name("localhost"),
        )
        .expect("tls config");
    let channel = endpoint.connect().await.expect("connect to test endpoint");

    let exporter = cx_sdk_otlp::logs::OtlpLogExporterGrpc::builder()
        .with_channel(channel)
        .with_backoff_config(cx_sdk_core::backoff::BackoffConfig {
            initial_delay: std::time::Duration::from_millis(100),
            max_delay: std::time::Duration::from_secs(1),
            max_elapsed_time: std::time::Duration::from_secs(2),
        })
        .try_build()
        .expect("otlp exporter");

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        exporter.export(otlp_request(), &otlp_auth()),
    )
    .await
    .expect("otlp control timed out");

    let (observed, grpc) = server.finish().await;

    assert!(result.is_ok(), "otlp control failed: {result:?}");
    assert_eq!(observed.handshakes_completed(), 1);
    assert!(
        observed.app_bytes() > 0,
        "no decrypted bytes were counted, so the rejection case's zero would prove \
         nothing about the counter"
    );
    assert_eq!(grpc.requests(), 1, "expected one gRPC export");
    assert!(
        grpc.saw_metadata(DUMMY_API_KEY),
        "the control did not carry the credential, so its absence in the rejection \
         case would prove nothing"
    );
}

/// The OTLP path rejects an affected chain and sends nothing.
///
/// Unlike the control, this uses the production configuration unmodified:
/// ChannelConfig::new(endpoint).with_webpki_roots(), no added CA. The distrust check
/// runs before issuer resolution, so the chain is rejected for distrust rather than
/// for an untrusted root.
///
/// The shipper's wrapper sanitises transport failures into a classification and a gRPC
/// status name, so CertDistrusted never reaches LogExportError. The observing layer
/// records it inside the SDK's own stack, leaving credentials, retries and the channel
/// configuration intact.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires the GovCloud build inputs; invoked explicitly in CI"]
#[serial_test::serial(govcloud_distrust)]
async fn otlp_rejects_distrusted_chain_before_sending_anything() {
    use cx_sdk_otlp::OtlpExporter as _;
    let (chain, key) = affected_chain();
    let server = spawn_grpc_server(chain, key).await;
    let port = server.port;

    let observations: Arc<Mutex<Vec<Outcome>>> = Arc::default();

    let exporter = cx_sdk_otlp::logs::OtlpLogExporterGrpc::builder()
        .with_channel_config(
            cx_sdk_core::channel::ChannelConfig::new(format!("https://localhost:{port}"))
                .with_webpki_roots(),
        )
        .layer(observing_layer(Arc::clone(&observations)))
        .with_backoff_config(cx_sdk_core::backoff::BackoffConfig {
            initial_delay: std::time::Duration::from_millis(100),
            max_delay: std::time::Duration::from_secs(1),
            max_elapsed_time: std::time::Duration::from_secs(2),
        })
        .try_build()
        .expect("otlp exporter");

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(60),
        exporter.export(otlp_request(), &otlp_auth()),
    )
    .await
    .expect("otlp rejection case timed out");

    let (observed, grpc) = server.finish().await;

    assert!(result.is_err(), "the affected chain was accepted");

    let seen = observations.lock().expect("observations poisoned");
    assert!(
        !seen.is_empty(),
        "no transport error was observed, so nothing establishes why the export \
         failed -- the layer may not be in the path"
    );
    let distrusted = seen
        .iter()
        .find(|o| matches!(o, Outcome::Distrusted { .. }));
    let Some(Outcome::Distrusted { typed }) = distrusted else {
        panic!("the transport failed, but not for distrust: {seen:?}");
    };
    if !typed {
        eprintln!(
            "note: matched CertDistrusted in rendered text; the typed \
             rustls::Error did not survive the client stack"
        );
    }

    assert!(
        observed.handshakes_attempted() > 0,
        "never reached the server"
    );
    assert_eq!(observed.handshakes_completed(), 0);
    assert_eq!(observed.app_bytes(), 0);
    assert_eq!(grpc.requests(), 0, "a gRPC export reached the server");
    assert!(
        !grpc.saw_metadata(DUMMY_API_KEY),
        "the credential reached the server"
    );
}
