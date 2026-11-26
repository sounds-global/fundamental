//! A value written to or read from the key-value engine reaches no log line at any level.
//!
//! Every event of every crate is captured at `TRACE` and searched for the values. Needs Docker.

use std::io;
use std::sync::{Arc, Mutex, PoisonError};

use fundamental_config::settings::{VaultAuth, VaultSettings};
use fundamental_core::secrets::InMemorySecret;
use fundamental_secrets::{KV_BODY_LIMIT, KvCas, KvData, KvPath, KvVersion, VaultClient};
use fundamental_test_containers::container_name::OPENBAO_04_KV;
use fundamental_test_containers::openbao::OpenBaoServer;
use tracing::subscriber::with_default;
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::fmt::format::FmtSpan;

const MOUNT: &str = "kv-log-suite";
const WRITTEN: &str = "written-value-7f3a91c2";
const REWRITTEN: &str = "rewritten-value-b81e04d6";

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);

impl Capture {
    fn text(&self) -> String {
        let bytes = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        String::from_utf8_lossy(&bytes).into_owned()
    }
}

impl io::Write for Capture {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for Capture {
    type Writer = Self;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

#[test]
fn no_written_value_reaches_a_log_at_trace() {
    let capture = Capture::default();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .with_span_events(FmtSpan::FULL)
        .with_writer(capture.clone())
        .finish();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    with_default(subscriber, || runtime.block_on(exercise()));

    let logs = capture.text();
    assert!(
        logs.contains("key-value secret written"),
        "the capture saw the write"
    );
    for value in [WRITTEN, REWRITTEN, "too-large-x"] {
        assert!(!logs.contains(value), "{value} reached a log line");
    }
}

async fn exercise() {
    let server = OpenBaoServer::shared(OPENBAO_04_KV)
        .await
        .expect("an OpenBao container (is Docker running?)");
    server.enable_kv(MOUNT).await.expect("kv mounted");
    let vault = VaultClient::from_settings(&VaultSettings {
        enabled: true,
        address: server.address().to_owned(),
        auth: VaultAuth::Token {
            token: InMemorySecret::new(server.root_token().to_owned()),
        },
        ..VaultSettings::default()
    })
    .await
    .expect("client")
    .expect("enabled");
    let path =
        KvPath::new(MOUNT, &format!("log/{}", uuid::Uuid::new_v4().simple())).expect("valid");
    let one = KvVersion::new(1).expect("non-zero");

    let data = KvData::new(&[("password", WRITTEN)]).expect("valid");
    vault
        .write_kv(&path, &data, KvCas::Absent)
        .await
        .expect("written");
    let fields = vault
        .read_kv_fields(&path, one, &["password"])
        .await
        .expect("read");
    assert_eq!(fields.get("password"), Some(WRITTEN));

    let again = KvData::new(&[("password", REWRITTEN)]).expect("valid");
    assert!(
        vault.write_kv(&path, &again, KvCas::Absent).await.is_err(),
        "check-and-set refused"
    );

    let large = format!("too-large-{}", "x".repeat(KV_BODY_LIMIT));
    let large = KvData::new(&[("password", large.as_str())]).expect("valid");
    assert!(
        vault
            .write_kv(&path, &large, KvCas::Unchecked)
            .await
            .is_err(),
        "too large"
    );

    let missing = KvVersion::new(9).expect("non-zero");
    assert!(
        vault
            .read_kv_fields(&path, missing, &["password"])
            .await
            .is_err(),
        "gone"
    );
}
