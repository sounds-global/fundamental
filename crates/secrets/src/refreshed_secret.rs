//! [`RefreshedSecret`] — a secret read from the store, kept current in the background.

use std::collections::HashMap;
use std::sync::{Arc, Weak};
use std::time::Duration;

use arc_swap::ArcSwap;
use fundamental_config::settings::{SecretRef, VaultKvRef};
use fundamental_core::Error as _;
use fundamental_core::secrets::SecretString;
use parking_lot::Mutex;
use subtle::ConstantTimeEq;
use tokio::task::JoinHandle;
use tokio::time::Instant;
use tracing::{debug, info, warn};

use crate::client::VaultClient;

/// A key-value secret re-read every interval, so a rotation reaches the process without a restart.
///
/// [`current`](Self::current) never waits; a failed read keeps the value in use. Clones share
/// one value and one task, aborted when the last clone is dropped.
#[derive(Clone)]
pub struct RefreshedSecret {
    state: Arc<State>,
}

/// Every secret a client is refreshing, held weakly so an unused one is dropped with its task.
pub(crate) type Registry = Mutex<HashMap<VaultKvRef, Weak<State>>>;

pub(crate) struct State {
    reference: VaultKvRef,
    value: ArcSwap<SecretString>,
    read_at: ArcSwap<Instant>,
    _refresher: AbortOnDrop,
}

/// Aborts the refresh task with the state it serves.
struct AbortOnDrop(JoinHandle<()>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

impl std::fmt::Debug for RefreshedSecret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RefreshedSecret")
            .field("reference", &self.state.reference)
            .field("value", &"[REDACTED]")
            .finish()
    }
}

impl RefreshedSecret {
    /// Reads `reference` and starts refreshing it every `interval`.
    ///
    /// # Errors
    /// Whatever [`VaultClient::read_secret`] returns for the first read.
    pub(crate) async fn start(
        vault: VaultClient,
        reference: VaultKvRef,
        interval: Duration,
    ) -> crate::SecretResult<Self> {
        let first = vault
            .read_secret(&SecretRef::VaultKv(reference.clone()))
            .await?;
        // The first upgrade comes a whole interval (at least a second) after this returns.
        let state = Arc::new_cyclic(|weak| State {
            reference,
            value: ArcSwap::from_pointee(first),
            read_at: ArcSwap::from_pointee(Instant::now()),
            _refresher: AbortOnDrop(tokio::spawn(refresh(vault, weak.clone(), interval))),
        });
        info!(
            secret.source = %state.reference,
            interval_seconds = interval.as_secs(),
            "secret read; refreshing it in the background"
        );
        Ok(Self { state })
    }

    /// The value last read.
    #[must_use]
    pub fn current(&self) -> SecretString {
        SecretString::clone(&self.state.value.load())
    }

    /// How long ago the value in use was read.
    #[must_use]
    pub fn age(&self) -> Duration {
        self.state.read_at.load().elapsed()
    }

    /// Where the secret is read from.
    #[must_use]
    pub fn reference(&self) -> &VaultKvRef {
        &self.state.reference
    }

    /// The one refreshed secret `registry` holds for `reference`, if it is still in use.
    pub(crate) fn registered(registry: &Registry, reference: &VaultKvRef) -> Option<Self> {
        registry
            .lock()
            .get(reference)
            .and_then(Weak::upgrade)
            .map(|state| Self { state })
    }

    /// Records this secret in `registry`, or returns the one another caller registered first.
    pub(crate) fn register(self, registry: &Registry) -> Self {
        let mut held = registry.lock();
        held.retain(|_, state| state.strong_count() > 0);
        if let Some(state) = held.get(&self.state.reference).and_then(Weak::upgrade) {
            return Self { state };
        }
        held.insert(self.state.reference.clone(), Arc::downgrade(&self.state));
        self
    }
}

/// Reads the secret again every `interval` until its state is dropped.
#[expect(clippy::cognitive_complexity, reason = "readability baseline")]
async fn refresh(vault: VaultClient, state: Weak<State>, interval: Duration) {
    loop {
        tokio::time::sleep(interval).await;
        let Some(state) = state.upgrade() else {
            debug!("secret no longer in use; its refresher ends");
            return;
        };
        match vault
            .read_secret(&SecretRef::VaultKv(state.reference.clone()))
            .await
        {
            Ok(read) => {
                let rotated: bool = !bool::from(
                    state
                        .value
                        .load()
                        .expose()
                        .as_bytes()
                        .ct_eq(read.expose().as_bytes()),
                );
                if rotated {
                    state.value.store(Arc::new(read));
                    info!(secret.source = %state.reference, "secret rotated; the next connection uses the new value");
                } else {
                    debug!(secret.source = %state.reference, "secret unchanged");
                }
                state.read_at.store(Arc::new(Instant::now()));
            }
            Err(failure) => {
                warn!(
                    error_code = failure.error_code(),
                    error = %failure,
                    secret.source = %state.reference,
                    stale_seconds = state.read_at.load().elapsed().as_secs(),
                    retryable = failure.is_retryable(),
                    "the secret could not be read again; the value in use is kept"
                );
            }
        }
    }
}
