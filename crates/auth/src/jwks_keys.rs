//! [`JwksKeys`] — a cached, background-refreshed JWKS key set fetched over HTTPS.

use std::sync::{Arc, Mutex, PoisonError, Weak};
use std::time::{Duration, Instant};

use arc_swap::ArcSwap;
use jsonwebtoken::TokenData;
use serde::de::DeserializeOwned;
use tokio::sync::watch;
use tokio::time::timeout;
use tracing::{Span, info, warn};

use crate::claim_pins::ClaimPins;
use crate::jwks_endpoint::{FETCH_TIMEOUT, JwksEndpoint};
use crate::jwks_snapshot::JwksSnapshot;
use crate::{AuthError, InvalidToken};

/// A remote JWKS, fetched once and refreshed in the background; an unknown `kid` causes at most
/// one fetch per `min_refetch`.
pub(crate) struct JwksKeys {
    endpoint: JwksEndpoint,
    pins: ClaimPins,
    snapshot: ArcSwap<JwksSnapshot>,
    min_refetch: Duration,
    refetch: Mutex<RefetchState>,
}

#[derive(Default)]
struct RefetchState {
    /// When the last fetch for an unknown key started; stamped before it runs.
    last_started: Option<Instant>,
    in_flight: Option<watch::Receiver<()>>,
}

/// A fetch for an unknown key, running; dropping it wakes every waiter, even on panic.
struct FetchInFlight {
    keys: Arc<JwksKeys>,
    finished: watch::Sender<()>,
}

impl Drop for FetchInFlight {
    fn drop(&mut self) {
        self.keys
            .refetch
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .in_flight = None;
        self.finished.send_replace(());
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum UnknownKeyRefetch {
    Joined,
    Started,
    /// A fetch ran within the interval; the token was checked against the keys held.
    CoolingDown,
}

impl UnknownKeyRefetch {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Joined => "joined",
            Self::Started => "started",
            Self::CoolingDown => "cooling_down",
        }
    }
}

#[derive(Clone, Copy)]
enum FetchReason {
    Schedule,
    UnknownKey,
}

impl FetchReason {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Schedule => "schedule",
            Self::UnknownKey => "unknown_key",
        }
    }
}

impl JwksKeys {
    /// Fetches the JWKS from `url` once, then spawns a refresher every `refresh`.
    ///
    /// # Errors
    /// [`AuthError::Config`] if `url` is not `https` to a non-loopback host, the client cannot
    /// be built, or `min_refetch` is under a second or not below a non-zero `refresh`;
    /// [`AuthError::Jwks`] if the initial fetch fails.
    pub(crate) async fn connect(
        url: &str,
        pins: ClaimPins,
        refresh: Duration,
        min_refetch: Duration,
    ) -> Result<Arc<Self>, AuthError> {
        check_intervals(refresh, min_refetch)?;
        let endpoint = JwksEndpoint::new(url)?;
        let started = Instant::now();
        let snapshot = endpoint.fetch(&pins).await?;
        info!(
            url = %endpoint.url(),
            reason = "startup",
            keys = snapshot.len(),
            elapsed_ms = elapsed_ms(started),
            "fetched JWKS"
        );
        let keys = Arc::new(Self {
            endpoint,
            pins,
            snapshot: ArcSwap::from_pointee(snapshot),
            min_refetch,
            refetch: Mutex::default(),
        });
        keys.spawn_refresher(refresh);
        Ok(keys)
    }

    /// Decodes `token` with the key named by `kid`, fetching the key set again first if the
    /// `kid` is unknown and the budget allows.
    ///
    /// # Errors
    /// [`AuthError::Invalid`] if no key the set holds verifies the token.
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    pub(crate) async fn decode<T: DeserializeOwned>(
        self: &Arc<Self>,
        token: &str,
        kid: Option<&str>,
    ) -> Result<TokenData<T>, AuthError> {
        if let Some(result) = self.try_decode(token, kid) {
            return result;
        }
        let (refetch, done) = self.refetch_for_unknown_key();
        Span::current().record("refetch", refetch.as_str());
        if let Some(mut done) = done {
            match timeout(FETCH_TIMEOUT + Duration::from_secs(1), done.changed()).await {
                Ok(Ok(())) => {}
                Ok(Err(_)) => warn!(
                    url = %self.endpoint.url(),
                    "JWKS fetch ended without reporting; checking the token against the keys held"
                ),
                Err(_) => warn!(
                    url = %self.endpoint.url(),
                    "JWKS fetch outlived its timeout; checking the token against the keys held"
                ),
            }
        }
        self.try_decode(token, kid)
            .unwrap_or(Err(AuthError::Invalid(InvalidToken::UnknownKey)))
    }

    /// Starts a fetch for an unknown key if the budget allows, and returns what resolves when it ends.
    fn refetch_for_unknown_key(
        self: &Arc<Self>,
    ) -> (UnknownKeyRefetch, Option<watch::Receiver<()>>) {
        let mut state = self.refetch.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(done) = &state.in_flight {
            return (UnknownKeyRefetch::Joined, Some(done.clone()));
        }
        let now = Instant::now();
        if !refetch_is_due(state.last_started, now, self.min_refetch) {
            return (UnknownKeyRefetch::CoolingDown, None);
        }
        let (finished, done) = watch::channel(());
        state.last_started = Some(now);
        state.in_flight = Some(done.clone());
        drop(state);

        let fetch = FetchInFlight {
            keys: Arc::clone(self),
            finished,
        };
        tokio::spawn(async move {
            fetch.keys.refresh(FetchReason::UnknownKey).await;
            drop(fetch);
        });
        (UnknownKeyRefetch::Started, Some(done))
    }

    /// Re-fetches the JWKS and swaps it in, keeping the last-good keys on failure.
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    async fn refresh(&self, reason: FetchReason) {
        let started = Instant::now();
        match self.endpoint.fetch(&self.pins).await {
            Ok(snapshot) => {
                let keys = snapshot.len();
                self.snapshot.store(Arc::new(snapshot));
                info!(
                    url = %self.endpoint.url(),
                    reason = reason.as_str(),
                    keys,
                    elapsed_ms = elapsed_ms(started),
                    "fetched JWKS"
                );
            }
            Err(error) => warn!(
                url = %self.endpoint.url(),
                reason = reason.as_str(),
                error = %error,
                elapsed_ms = elapsed_ms(started),
                "JWKS fetch failed; keeping last-good keys"
            ),
        }
    }

    /// Decodes against the current snapshot; `None` only when the `kid` is absent.
    fn try_decode<T: DeserializeOwned>(
        &self,
        token: &str,
        kid: Option<&str>,
    ) -> Option<Result<TokenData<T>, AuthError>> {
        let snapshot = self.snapshot.load();
        Some(snapshot.key_for(kid)?.decode::<T>(token))
    }

    /// Spawns a refresher every `interval`, ending when the last [`Arc`] is dropped.
    #[expect(clippy::excessive_nesting, reason = "readability baseline")]
    fn spawn_refresher(self: &Arc<Self>, interval: Duration) {
        if interval.is_zero() {
            return;
        }
        let weak: Weak<Self> = Arc::downgrade(self);
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            ticker.tick().await; // the first tick fires immediately; skip it
            loop {
                ticker.tick().await;
                let Some(keys) = weak.upgrade() else {
                    break;
                };
                keys.refresh(FetchReason::Schedule).await;
            }
        });
    }
}

fn refetch_is_due(last_started: Option<Instant>, now: Instant, min_refetch: Duration) -> bool {
    last_started.is_none_or(|last| now.saturating_duration_since(last) >= min_refetch)
}

/// Refuses a refetch interval under a second or one the scheduled refresh always pre-empts.
fn check_intervals(refresh: Duration, min_refetch: Duration) -> Result<(), AuthError> {
    if min_refetch < Duration::from_secs(1) {
        return Err(AuthError::Config(
            "token_jwks_min_refetch_secs must be at least 1".to_owned(),
        ));
    }
    if !refresh.is_zero() && min_refetch >= refresh {
        return Err(AuthError::Config(format!(
            "token_jwks_min_refetch_secs ({}) must be below token_jwks_refresh_secs ({})",
            min_refetch.as_secs(),
            refresh.as_secs()
        )));
    }
    Ok(())
}

fn elapsed_ms(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use jsonwebtoken::Algorithm;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    use super::*;
    use crate::test_keys::{es256_key, public_jwk};

    async fn serve_one_key() -> String {
        let jwk = public_jwk(&es256_key(1).encoding, Algorithm::ES256, Some("k1"));
        let body = serde_json::json!({ "keys": [jwk] }).to_string();
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let mut scratch = [0u8; 1024];
                let _ = socket.read(&mut scratch).await;
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(response.as_bytes()).await;
            }
        });
        format!("http://{addr}/jwks")
    }

    #[tokio::test]
    async fn a_finished_fetch_leaves_nothing_in_flight() {
        let keys = JwksKeys::connect(
            &serve_one_key().await,
            ClaimPins::default(),
            Duration::from_secs(3600),
            Duration::from_secs(60),
        )
        .await
        .expect("connects");

        let (first, done) = keys.refetch_for_unknown_key();
        assert_eq!(first, UnknownKeyRefetch::Started);
        done.expect("a fetch to wait on")
            .changed()
            .await
            .expect("the fetch reports its end");

        let (second, done) = keys.refetch_for_unknown_key();
        assert_eq!(
            second,
            UnknownKeyRefetch::CoolingDown,
            "a fetch that ended is not still running"
        );
        assert!(done.is_none());
    }

    #[test]
    fn the_first_unknown_key_may_fetch() {
        assert!(refetch_is_due(
            None,
            Instant::now(),
            Duration::from_secs(60)
        ));
    }

    #[test]
    fn a_second_unknown_key_within_the_interval_may_not_fetch() {
        let last = Instant::now();
        assert!(!refetch_is_due(
            Some(last),
            last + Duration::from_secs(59),
            Duration::from_secs(60)
        ));
        assert!(refetch_is_due(
            Some(last),
            last + Duration::from_secs(60),
            Duration::from_secs(60)
        ));
    }

    #[test]
    fn a_refetch_interval_the_schedule_pre_empts_is_refused() {
        let hour = Duration::from_secs(3600);
        assert!(matches!(
            check_intervals(hour, hour),
            Err(AuthError::Config(_))
        ));
        assert!(matches!(
            check_intervals(hour, Duration::ZERO),
            Err(AuthError::Config(_))
        ));
        assert!(check_intervals(hour, Duration::from_secs(60)).is_ok());
        assert!(check_intervals(Duration::ZERO, Duration::from_secs(60)).is_ok());
    }
}
