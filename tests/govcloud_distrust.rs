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
pub const AFFECTED_INTERMEDIATE: &[u8] =
    include_bytes!("data/distrusted/emudhra-intermediate.der");

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
    Distrusted { typed: bool },
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
            let handle = tokio::spawn(async move {
                seen.handshakes_attempted.fetch_add(1, Ordering::SeqCst);
                // A rejected handshake ends here: nothing below runs, so no
                // application byte can be counted.
                let Ok(mut tls) = acceptor.accept(stream).await else {
                    return;
                };
                seen.handshakes_completed.fetch_add(1, Ordering::SeqCst);
                read_request(&mut tls, &seen).await;
                let _ = tls
                    .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                    .await;
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

pub fn control_pki() -> ControlPki {
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

    ControlPki {
        ca_pem: ca_cert.pem(),
        chain: vec![leaf.der().clone(), ca_cert.der().clone()],
        key: rustls::pki_types::PrivateKeyDer::try_from(leaf_key.serialize_der())
            .expect("leaf key der"),
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
    assert!(
        cfg!(target_os = "linux"),
        "these tests establish trust through SSL_CERT_FILE, which rustls-platform-verifier \
         honours only on Linux. Run them in the AL2023 container, as CI does."
    );
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
    check
        .add(stand_in.der().clone())
        .expect("stand-in parses");
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
        Outcome::OtherTlsError(detail) => panic!(
            "rejected, but not for distrust: {detail}\nfull error: {err:?}"
        ),
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
