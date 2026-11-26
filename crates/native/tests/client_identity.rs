//! A `RemoteBridge` presenting a device's `ClientIdentity` to a server that requires one.
//!
//! Real TLS both ways, the certificate issued from the request the device key signed.

#![cfg(feature = "client-identity")]

use std::str::FromStr as _;
use std::sync::Arc;
use std::time::Duration;

use der::asn1::OctetString;
use der::{DecodePem as _, Encode as _, EncodePem as _};
use fundamental_core::Error as _;
use fundamental_keys::{DeviceKey as _, MemorySecretStore, SealedDeviceKey, certificate_request};
use fundamental_native::{
    ActionBridge as _, BridgeRequest, ClientIdentity, NativeError, RemoteBridge, ServerTrust,
};
use fundamental_server::Method;
use p256::ecdsa::{DerSignature, SigningKey};
use p256::pkcs8::EncodePrivateKey as _;
use rustls::pki_types::pem::PemObject as _;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::server::WebPkiClientVerifier;
use spki::SubjectPublicKeyInfoOwned;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio_rustls::TlsAcceptor;
use x509_cert::Certificate;
use x509_cert::builder::{Builder as _, CertificateBuilder, Profile};
use x509_cert::ext::pkix::SubjectAltName;
use x509_cert::ext::pkix::name::GeneralName;
use x509_cert::name::Name;
use x509_cert::request::CertReq;
use x509_cert::serial_number::SerialNumber;
use x509_cert::time::Validity;

#[derive(Debug)]
enum Seen {
    /// The handshake failed: no certificate, or one that did not chain.
    Refused,
    /// A request arrived over a verified connection.
    Served {
        /// The client certificate's `SubjectPublicKeyInfo`.
        client_key: Vec<u8>,
        /// The client certificate's serial, to tell a renewal from the first.
        serial: Vec<u8>,
        /// The `Cookie` header the request carried.
        cookie: Option<String>,
    },
}

struct TestCa {
    key: SigningKey,
    name: Name,
    pem: String,
}

impl TestCa {
    fn new() -> Self {
        let key = SigningKey::random(&mut p256::elliptic_curve::rand_core::OsRng);
        let name = Name::from_str("CN=Test operations CA").expect("name");
        let certificate = CertificateBuilder::new(
            Profile::Root,
            SerialNumber::from(1_u32),
            Validity::from_now(Duration::from_secs(3600)).expect("validity"),
            name.clone(),
            spki_of(&key),
            &key,
        )
        .expect("builder")
        .build::<DerSignature>()
        .expect("root");
        let pem = certificate.to_pem(der::pem::LineEnding::LF).expect("pem");
        Self { key, name, pem }
    }

    fn issue(
        &self,
        serial: u32,
        subject: Name,
        spki: SubjectPublicKeyInfoOwned,
        address: Option<[u8; 4]>,
    ) -> Certificate {
        let mut builder = CertificateBuilder::new(
            Profile::Leaf {
                issuer: self.name.clone(),
                enable_key_agreement: false,
                enable_key_encipherment: false,
            },
            SerialNumber::from(serial),
            Validity::from_now(Duration::from_secs(3600)).expect("validity"),
            subject,
            spki,
            &self.key,
        )
        .expect("builder");
        if let Some(address) = address {
            builder
                .add_extension(&SubjectAltName(vec![GeneralName::IpAddress(
                    OctetString::new(address.to_vec()).expect("octets"),
                )]))
                .expect("SAN");
        }
        builder.build::<DerSignature>().expect("leaf")
    }

    /// The chain a device installs, issued from the request its key signed.
    fn issue_from_request(&self, serial: u32, request_pem: &str) -> String {
        let request = CertReq::from_pem(request_pem.as_bytes()).expect("PKCS #10");
        let leaf = self.issue(serial, request.info.subject, request.info.public_key, None);
        leaf.to_pem(der::pem::LineEnding::LF).expect("pem")
    }
}

fn spki_of(key: &SigningKey) -> SubjectPublicKeyInfoOwned {
    SubjectPublicKeyInfoOwned::from_key(*key.verifying_key()).expect("SPKI")
}

fn der_of(pem: &str) -> Vec<CertificateDer<'static>> {
    CertificateDer::pem_slice_iter(pem.as_bytes())
        .collect::<Result<Vec<_>, _>>()
        .expect("PEM")
}

/// A loopback server requiring a client certificate from `ca`, closing after each answer so
/// each call is a new handshake.
#[expect(clippy::too_many_lines, reason = "readability baseline")]
async fn mtls_server(ca: &TestCa) -> (String, mpsc::UnboundedReceiver<Seen>) {
    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let server_key = SigningKey::random(&mut p256::elliptic_curve::rand_core::OsRng);
    let server_cert = ca.issue(
        2,
        Name::from_str("CN=127.0.0.1").expect("name"),
        spki_of(&server_key),
        Some([127, 0, 0, 1]),
    );
    let mut roots = rustls::RootCertStore::empty();
    roots.add_parsable_certificates(der_of(&ca.pem));
    let verifier = WebPkiClientVerifier::builder_with_provider(Arc::new(roots), provider.clone())
        .build()
        .expect("client verifier");
    let key_der = server_key
        .to_pkcs8_der()
        .expect("pkcs8")
        .as_bytes()
        .to_vec();
    let config = rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .expect("versions")
        .with_client_cert_verifier(verifier)
        .with_single_cert(
            vec![CertificateDer::from(server_cert.to_der().expect("der"))],
            PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key_der)),
        )
        .expect("server config");
    let acceptor = TlsAcceptor::from(Arc::new(config));
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let address = listener.local_addr().expect("address");
    let (seen, observations) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        loop {
            let Ok((socket, _)) = listener.accept().await else {
                return;
            };
            let Ok(mut stream) = acceptor.accept(socket).await else {
                let _ = seen.send(Seen::Refused);
                continue;
            };
            let mut request = Vec::new();
            let mut buffer = [0_u8; 4096];
            while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                match stream.read(&mut buffer).await {
                    Ok(0) | Err(_) => break,
                    Ok(read) => request.extend_from_slice(&buffer[..read]),
                }
            }
            let text = String::from_utf8_lossy(&request).into_owned();
            let cookie = text
                .lines()
                .find_map(|line| line.strip_prefix("cookie: ").map(str::to_owned));
            let leaf = stream
                .get_ref()
                .1
                .peer_certificates()
                .and_then(|chain| chain.first())
                .map(|certificate| {
                    der::Decode::from_der(certificate.as_ref()).expect("client certificate")
                });
            let Some(leaf): Option<Certificate> = leaf else {
                let _ = seen.send(Seen::Refused);
                continue;
            };
            let _ = seen.send(Seen::Served {
                client_key: leaf
                    .tbs_certificate
                    .subject_public_key_info
                    .to_der()
                    .expect("SPKI"),
                serial: leaf.tbs_certificate.serial_number.as_bytes().to_vec(),
                cookie,
            });
            let _ = stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\ncontent-type: application/json\r\n\
                      set-cookie: session=device-bound; Path=/; Secure; HttpOnly\r\n\
                      content-length: 2\r\nconnection: close\r\n\r\n{}",
                )
                .await;
            let _ = stream.shutdown().await;
        }
    });
    (
        format!("https://127.0.0.1:{}", address.port()),
        observations,
    )
}

fn device() -> (MemorySecretStore, SealedDeviceKey) {
    let store = MemorySecretStore::new();
    let key = SealedDeviceKey::open_or_create(&store, "computer").expect("device key");
    (store, key)
}

async fn call(bridge: &RemoteBridge) -> Result<fundamental_native::BridgeResponse, NativeError> {
    bridge
        .call(BridgeRequest::new(Method::Get, "/api/me"))
        .await
}

#[tokio::test]
async fn the_server_sees_the_certificate_issued_to_the_devices_own_key() {
    let ca = TestCa::new();
    let (url, mut seen) = mtls_server(&ca).await;
    let (_store, key) = device();
    let public = key.public_key_der().to_vec();
    let identity = Arc::new(ClientIdentity::new(key));
    let request = certificate_request(identity.key(), "operator computer", None).expect("CSR");
    identity
        .install(&ca.issue_from_request(10, &request))
        .expect("installed");
    let bridge = RemoteBridge::new(&url)
        .expect("url")
        .with_client_identity(Arc::clone(&identity), &ServerTrust::Anchors(ca.pem.clone()))
        .expect("bridge");

    let response = call(&bridge).await.expect("served");
    assert_eq!(response.status, 200);
    match seen.recv().await.expect("observed") {
        Seen::Served { client_key, .. } => assert_eq!(client_key, public),
        other => panic!("the server did not serve the device: {other:?}"),
    }
}

#[tokio::test]
async fn without_a_certificate_the_handshake_is_refused() {
    let ca = TestCa::new();
    let (url, mut seen) = mtls_server(&ca).await;
    let (_store, key) = device();
    let bridge = RemoteBridge::new(&url)
        .expect("url")
        .with_client_identity(
            Arc::new(ClientIdentity::new(key)),
            &ServerTrust::Anchors(ca.pem.clone()),
        )
        .expect("bridge");

    let error = call(&bridge).await.expect_err("refused");
    assert_eq!(error.error_code(), "NATIVE_002");
    assert!(matches!(seen.recv().await, Some(Seen::Refused)));
}

#[tokio::test]
async fn a_renewed_certificate_is_presented_on_the_next_connection() {
    let ca = TestCa::new();
    let (url, mut seen) = mtls_server(&ca).await;
    let (_store, key) = device();
    let identity = Arc::new(ClientIdentity::new(key));
    let request = certificate_request(identity.key(), "operator computer", None).expect("CSR");
    identity
        .install(&ca.issue_from_request(10, &request))
        .expect("first");
    let bridge = RemoteBridge::new(&url)
        .expect("url")
        .with_client_identity(Arc::clone(&identity), &ServerTrust::Anchors(ca.pem.clone()))
        .expect("bridge");
    call(&bridge).await.expect("first call");
    identity
        .install(&ca.issue_from_request(11, &request))
        .expect("renewed");
    call(&bridge).await.expect("second call");

    let serials: Vec<Vec<u8>> = [seen.recv().await, seen.recv().await]
        .into_iter()
        .map(|observed| match observed {
            Some(Seen::Served { serial, .. }) => serial,
            other => panic!("not served: {other:?}"),
        })
        .collect();
    assert_eq!(serials, vec![vec![10], vec![11]]);
}

#[tokio::test]
async fn a_cleared_identity_is_refused_on_the_next_connection() {
    let ca = TestCa::new();
    let (url, mut seen) = mtls_server(&ca).await;
    let (_store, key) = device();
    let identity = Arc::new(ClientIdentity::new(key));
    let request = certificate_request(identity.key(), "operator computer", None).expect("CSR");
    identity
        .install(&ca.issue_from_request(10, &request))
        .expect("installed");
    let bridge = RemoteBridge::new(&url)
        .expect("url")
        .with_client_identity(Arc::clone(&identity), &ServerTrust::Anchors(ca.pem.clone()))
        .expect("bridge");
    call(&bridge).await.expect("served while enrolled");
    identity.clear();

    let error = call(&bridge).await.expect_err("refused once cleared");
    assert_eq!(error.error_code(), "NATIVE_002");
    assert!(matches!(seen.recv().await, Some(Seen::Served { .. })));
    assert!(
        matches!(seen.recv().await, Some(Seen::Refused)),
        "a resumed session went on authenticating the cleared certificate"
    );
}

#[tokio::test]
async fn the_session_stays_in_the_core_process_and_never_reaches_the_page() {
    let ca = TestCa::new();
    let (url, mut seen) = mtls_server(&ca).await;
    let (_store, key) = device();
    let identity = Arc::new(ClientIdentity::new(key));
    let request = certificate_request(identity.key(), "operator computer", None).expect("CSR");
    identity
        .install(&ca.issue_from_request(10, &request))
        .expect("installed");
    let bridge = RemoteBridge::new(&url)
        .expect("url")
        .with_client_identity(identity, &ServerTrust::Anchors(ca.pem.clone()))
        .expect("bridge");

    let first = call(&bridge).await.expect("first");
    assert!(
        first
            .headers
            .iter()
            .all(|(name, _)| !name.eq_ignore_ascii_case("set-cookie")),
        "the session cookie reached the webview's response"
    );
    call(&bridge).await.expect("second");
    let cookies: Vec<Option<String>> = [seen.recv().await, seen.recv().await]
        .into_iter()
        .map(|observed| match observed {
            Some(Seen::Served { cookie, .. }) => cookie,
            other => panic!("not served: {other:?}"),
        })
        .collect();
    assert_eq!(
        cookies,
        vec![None, Some("session=device-bound".to_owned())],
        "the bridge's own jar carries the session to the next call"
    );
}

#[tokio::test]
async fn a_server_the_pinned_anchors_did_not_issue_is_not_believed() {
    let ca = TestCa::new();
    let (url, mut seen) = mtls_server(&ca).await;
    let stranger = TestCa::new();
    let (_store, key) = device();
    let identity = Arc::new(ClientIdentity::new(key));
    let request = certificate_request(identity.key(), "operator computer", None).expect("CSR");
    identity
        .install(&ca.issue_from_request(10, &request))
        .expect("installed");
    let bridge = RemoteBridge::new(&url)
        .expect("url")
        .with_client_identity(identity, &ServerTrust::Anchors(stranger.pem.clone()))
        .expect("bridge");

    let error = call(&bridge).await.expect_err("the server is not believed");
    assert_eq!(error.error_code(), "NATIVE_002");
    assert!(matches!(seen.recv().await, Some(Seen::Refused)));
}

#[test]
fn a_certificate_for_another_key_is_refused_and_the_installed_one_kept() {
    let ca = TestCa::new();
    let (_store, key) = device();
    let (_other_store, other) = device();
    let identity = ClientIdentity::new(key);
    let own = certificate_request(identity.key(), "operator computer", None).expect("CSR");
    identity
        .install(&ca.issue_from_request(10, &own))
        .expect("own");
    let foreign = certificate_request(&other, "someone else", None).expect("CSR");

    let error = identity
        .install(&ca.issue_from_request(11, &foreign))
        .expect_err("refused");
    assert!(matches!(error, NativeError::ClientCertificate { .. }));
    assert_eq!(error.error_code(), "NATIVE_027");
    assert!(identity.has_certificate(), "the refusal replaced nothing");
}

#[test]
fn a_chain_with_no_certificate_is_refused() {
    let (_store, key) = device();
    let identity = ClientIdentity::new(key);
    let error = identity.install("").expect_err("refused");
    assert_eq!(error.error_code(), "NATIVE_027");
    assert!(!identity.has_certificate());
}

#[test]
fn pinned_anchors_that_hold_no_root_are_refused() {
    let (_store, key) = device();
    let error = RemoteBridge::new("https://127.0.0.1:1")
        .expect("url")
        .with_client_identity(
            Arc::new(ClientIdentity::new(key)),
            &ServerTrust::Anchors(String::new()),
        )
        .expect_err("refused");
    assert_eq!(error.error_code(), "NATIVE_028");
}
