//! [`StoredSessionVerifier`] — a session is live while its row is.

use std::time::{SystemTime, UNIX_EPOCH};

use std::sync::Arc;

use fundamental_core::secrets::SecretString;
use fundamental_encryption::Keyring;
use tracing::{debug, instrument, warn};

use crate::{
    AuthError, AuthenticatedPrincipal, PrincipalVerifier, SessionHandle, SessionHandleKey,
    SessionStore,
};

/// Verifies an opaque session handle against the application's [`SessionStore`]: the row is the
/// session.
#[derive(Debug, Clone)]
pub struct StoredSessionVerifier<S> {
    store: S,
    keys: Arc<Keyring<SessionHandleKey>>,
}

impl<S> StoredSessionVerifier<S> {
    /// A verifier reading `store`, with the handles' digests keyed by `keys`.
    #[must_use]
    pub fn new(store: S, keys: Arc<Keyring<SessionHandleKey>>) -> Self {
        Self { store, keys }
    }
}

impl<S> PrincipalVerifier for StoredSessionVerifier<S>
where
    S: SessionStore + Send + Sync,
{
    #[instrument(
        name = "fundamental.auth.stored_session",
        skip_all,
        fields(session, outcome)
    )]
    async fn verify(&self, token: &SecretString) -> Result<AuthenticatedPrincipal, AuthError> {
        let span = tracing::Span::current();
        let Some(digests) = SessionHandle::digests_of(&self.keys, token) else {
            span.record("outcome", "malformed");
            debug!("a value that cannot be a session handle was presented");
            return Err(AuthError::SessionEnded);
        };
        if let Some(current) = digests.first() {
            span.record("session", hex(&current.as_bytes()[..4]));
        }

        let mut found = None;
        for digest in &digests {
            match self.store.find(digest).await {
                Ok(Some(record)) => {
                    if digest.version() != self.keys.current_version() {
                        debug!(version = ?digest.version(), "a session found under a retained key version");
                    }
                    found = Some(record);
                    break;
                }
                Ok(None) => {}
                Err(error) => {
                    span.record("outcome", "store_unavailable");
                    warn!(%error, "the session store could not be read");
                    return Err(error);
                }
            }
        }
        let Some(record) = found else {
            span.record("outcome", "absent");
            debug!("no live session answers to this handle");
            return Err(AuthError::SessionEnded);
        };

        if record.not_after <= unix_now() {
            span.record("outcome", "expired");
            debug!(subject = %record.subject.as_str(), "the session's lifetime has run out");
            return Err(AuthError::SessionEnded);
        }

        span.record("outcome", "live");
        Ok(AuthenticatedPrincipal {
            user_id: record.subject,
            device_id: record.device,
            scopes: record.scopes,
            expires_at: record.not_after,
        })
    }
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| {
            i64::try_from(elapsed.as_secs()).unwrap_or(i64::MAX)
        })
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use fundamental_core::{DeviceId, Error as _, UserId};

    use super::*;
    use crate::session_handle::tests::keys;
    use crate::{SessionDigest, SessionRecord};

    struct OneSession {
        digest: [u8; 32],
        record: SessionRecord,
        reads: AtomicUsize,
        broken: bool,
    }

    impl SessionStore for OneSession {
        async fn find(&self, digest: &SessionDigest) -> Result<Option<SessionRecord>, AuthError> {
            self.reads.fetch_add(1, Ordering::SeqCst);
            if self.broken {
                return Err(AuthError::SessionStoreUnavailable(
                    "connection refused".to_owned(),
                ));
            }
            Ok((digest.as_bytes() == &self.digest).then(|| self.record.clone()))
        }
    }

    fn store(not_after: i64, broken: bool) -> (SecretString, StoredSessionVerifier<OneSession>) {
        let handle = SessionHandle::mint(&keys(1, &[1])).expect("entropy");
        let presented = SecretString::new(handle.secret().expose().to_owned());
        let store = OneSession {
            digest: *handle.digest().as_bytes(),
            record: SessionRecord {
                subject: UserId::new("operator-1"),
                device: Some(DeviceId::new("device-1")),
                scopes: vec!["ops".to_owned()],
                not_after,
            },
            reads: AtomicUsize::new(0),
            broken,
        };
        (
            presented,
            StoredSessionVerifier::new(store, Arc::new(keys(1, &[1]))),
        )
    }

    fn later() -> i64 {
        unix_now() + 600
    }

    #[tokio::test]
    async fn a_live_row_is_the_principal() {
        let (handle, verifier) = store(later(), false);
        let principal = verifier.verify(&handle).await.expect("live");
        assert_eq!(principal.user_id.as_str(), "operator-1");
        assert_eq!(
            principal.device_id.as_ref().map(DeviceId::as_str),
            Some("device-1")
        );
        assert!(principal.has_scope("ops"));
    }

    #[tokio::test]
    async fn an_absent_row_is_a_401() {
        let (_, verifier) = store(later(), false);
        let other = SessionHandle::mint(&keys(1, &[1])).expect("entropy");
        let err = verifier
            .verify(&SecretString::new(other.secret().expose().to_owned()))
            .await
            .expect_err("never issued here");
        assert!(matches!(err, AuthError::SessionEnded));
        assert_eq!(err.error_code(), "AUTH_011");
        assert_eq!(err.http_status_code(), 401);
    }

    #[tokio::test]
    async fn an_expired_row_the_store_has_not_reaped_is_refused() {
        let (handle, verifier) = store(unix_now() - 1, false);
        let err = verifier.verify(&handle).await.expect_err("expired");
        assert!(matches!(err, AuthError::SessionEnded));
    }

    #[tokio::test]
    async fn an_unreachable_store_is_a_503_never_an_allow() {
        let (handle, verifier) = store(later(), true);
        let err = verifier.verify(&handle).await.expect_err("store down");
        assert_eq!(err.error_code(), "AUTH_012");
        assert_eq!(err.http_status_code(), 503);
        assert!(err.is_retryable());
    }

    #[tokio::test]
    async fn a_malformed_handle_costs_no_store_read() {
        let (_, verifier) = store(later(), false);
        let err = verifier
            .verify(&SecretString::new("not-a-handle".to_owned()))
            .await
            .expect_err("malformed");
        assert!(matches!(err, AuthError::SessionEnded));
        assert_eq!(verifier.store.reads.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn a_row_planted_without_the_key_is_never_found() {
        let planted = SessionHandle::mint(&keys(9, &[9])).expect("entropy");
        let store = OneSession {
            digest: *planted.digest().as_bytes(),
            record: SessionRecord {
                subject: UserId::new("founder"),
                device: None,
                scopes: Vec::new(),
                not_after: later(),
            },
            reads: AtomicUsize::new(0),
            broken: false,
        };
        let verifier = StoredSessionVerifier::new(store, Arc::new(keys(1, &[1])));
        let err = verifier
            .verify(&SecretString::new(planted.secret().expose().to_owned()))
            .await
            .expect_err("the planted row is not this handle's");
        assert!(matches!(err, AuthError::SessionEnded));
    }

    #[tokio::test]
    async fn a_session_opened_before_a_rotation_stays_live() {
        let (handle, before) = store(later(), false);
        let rotated = StoredSessionVerifier::new(before.store, Arc::new(keys(2, &[1, 2])));
        let principal = rotated
            .verify(&handle)
            .await
            .expect("found under the retained version");
        assert_eq!(principal.user_id.as_str(), "operator-1");
        assert_eq!(
            rotated.store.reads.load(Ordering::SeqCst),
            2,
            "current first, then v1"
        );
    }
}
