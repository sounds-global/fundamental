//! Dynamic database credentials and background lease renewal.

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use fundamental_core::Error;
use fundamental_core::secrets::SecretString;
use serde_json::Value;
use tokio::sync::{Notify, watch};
use tracing::{debug, error, info, warn};

use crate::client::VaultClient;
use crate::error::{SecretError, SecretResult};

/// The first wait after a failed rotation, doubled on each further failure.
const RETRY_FLOOR: Duration = Duration::from_secs(5);

/// The longest wait between two rotation attempts.
const RETRY_CEILING: Duration = Duration::from_secs(60);

/// A short-lived database credential minted by Vault's database secrets engine.
#[derive(Clone)]
pub struct LeasedCredential {
    /// The minted username.
    pub username: SecretString,
    /// The minted password.
    pub password: SecretString,
    /// The Vault lease id.
    pub lease_id: String,
    /// Lease duration in seconds.
    pub lease_duration: u64,
    /// Whether the lease can be renewed.
    pub renewable: bool,
    /// The client's login count at mint; the lease dies with that login's token.
    pub(crate) minted_under: u64,
}

impl LeasedCredential {
    /// Revokes the lease of a credential never put to use, logging the outcome with `why`.
    pub async fn revoke(self, vault: &VaultClient, why: &str) {
        revoke(vault, &self.lease_id, why).await;
    }

    /// The credential Vault answered at `path`, refusing an answer that leaves any part of the
    /// lease out.
    ///
    /// # Errors
    /// [`SecretError::UnexpectedResponse`] (`SECRETS_005`) naming the missing field.
    pub(crate) fn from_response(
        path: &str,
        response: &Value,
        minted_under: u64,
    ) -> SecretResult<Self> {
        let missing = |field: &str| SecretError::UnexpectedResponse {
            path: path.to_owned(),
            message: format!("missing {field}"),
        };
        let data = |name: &str| {
            response
                .pointer(&format!("/data/{name}"))
                .and_then(Value::as_str)
                .map(|value| SecretString::new(value.to_owned()))
                .ok_or_else(|| missing(&format!("data.{name}")))
        };
        Ok(Self {
            username: data("username")?,
            password: data("password")?,
            lease_id: response
                .get("lease_id")
                .and_then(Value::as_str)
                .filter(|lease_id| !lease_id.is_empty())
                .ok_or_else(|| missing("lease_id"))?
                .to_owned(),
            lease_duration: response
                .get("lease_duration")
                .and_then(Value::as_u64)
                .ok_or_else(|| missing("lease_duration"))?,
            renewable: response
                .get("renewable")
                .and_then(Value::as_bool)
                .ok_or_else(|| missing("renewable"))?,
            minted_under,
        })
    }
}

impl std::fmt::Debug for LeasedCredential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LeasedCredential")
            .field("username", &"[REDACTED]")
            .field("password", &"[REDACTED]")
            .field("lease_id", &self.lease_id)
            .field("lease_duration", &self.lease_duration)
            .field("renewable", &self.renewable)
            .finish()
    }
}

/// How long [`RenewerHandle::shutdown`] waits for the lease in use to be revoked.
const SHUTDOWN_BUDGET: Duration = Duration::from_secs(10);

/// Owns a background lease-renewal task.
///
/// Dropping the handle only aborts the task and leaves the lease valid until its TTL.
#[derive(Debug)]
pub struct RenewerHandle {
    handle: tokio::task::JoinHandle<()>,
    stop: Arc<Notify>,
    stopped: watch::Receiver<bool>,
}

impl RenewerHandle {
    /// Stops renewing and revokes the lease in use, waiting at most ten seconds for it.
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    pub async fn shutdown(&self) {
        self.stop.notify_one();
        let mut stopped = self.stopped.clone();
        match tokio::time::timeout(SHUTDOWN_BUDGET, stopped.wait_for(|done| *done)).await {
            Ok(Ok(_)) => {}
            Ok(Err(_)) => warn!(
                "the lease renewer ended before it was asked to stop; the lease in use was not \
                 revoked and stays valid until its TTL"
            ),
            Err(_) => warn!(
                budget_secs = SHUTDOWN_BUDGET.as_secs(),
                "the lease in use was not revoked within the shutdown budget; it stays valid \
                 until its TTL"
            ),
        }
    }
}

impl Drop for RenewerHandle {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

/// Renew at about two thirds of the TTL.
fn renew_delay(ttl: u64) -> Duration {
    let secs = (ttl.max(1) * 2) / 3;
    Duration::from_secs(secs.max(1))
}

/// The lease the pool is running on.
struct CurrentLease {
    lease_id: String,
    /// The duration it was minted with; every renewal asks for it again.
    requested: u64,
    ttl: u64,
    renewable: bool,
    /// Vault granted less than asked: the lease is at its maximum TTL.
    at_max: bool,
    /// The client's login count at mint; a later login means the parent token is expiring.
    minted_under: u64,
}

impl CurrentLease {
    fn of(credential: &LeasedCredential) -> Self {
        Self {
            lease_id: credential.lease_id.clone(),
            requested: credential.lease_duration.max(1),
            ttl: credential.lease_duration.max(1),
            renewable: credential.renewable,
            at_max: false,
            minted_under: credential.minted_under,
        }
    }

    /// Whether renewing can still extend this lease; a lease minted before `login` moves instead.
    ///
    /// Asked during a pending rotation too, since the lease in use is the only one the pool has.
    fn extendable(&self, login: u64) -> bool {
        self.renewable && !self.at_max && self.minted_under == login
    }
}

/// Spawns a background task that keeps a leased database credential fresh.
///
/// It renews at about two thirds of the TTL and rotates (mints from `mount`/`role` and calls
/// `rebuild`) when the lease is not renewable, a renewal fails or grants less than asked,
/// or `vault` logs in again.
///
/// - `Ok(())` from `rebuild`: the new lease becomes current and the old one is revoked.
/// - `Err(_)`: the new lease is revoked and the rotation retried on a backoff from five
///   seconds to a minute.
#[expect(
    clippy::too_many_lines,
    clippy::excessive_nesting,
    reason = "readability baseline"
)]
pub fn spawn_db_lease_renewer<F, Fut, E>(
    vault: VaultClient,
    mount: String,
    role: String,
    initial: LeasedCredential,
    mut rebuild: F,
) -> RenewerHandle
where
    F: FnMut(LeasedCredential) -> Fut + Send + 'static,
    Fut: Future<Output = Result<(), E>> + Send + 'static,
    E: Error,
{
    let stop = Arc::new(Notify::new());
    let (stopped_tx, stopped) = watch::channel(false);
    let stop_requested = Arc::clone(&stop);
    let handle = tokio::spawn(async move {
        let mut current = CurrentLease::of(&initial);
        drop(initial);
        let mut wait = renew_delay(current.ttl);
        let mut retry = RETRY_FLOOR;
        let mut logins = vault.logins();

        loop {
            tokio::select! {
                () = stop_requested.notified() => {
                    revoke(
                        &vault,
                        &current.lease_id,
                        "the credential in use at shutdown",
                    )
                    .await;
                    stopped_tx.send_replace(true);
                    return;
                }
                Ok(()) = logins.changed() => {}
                () = tokio::time::sleep(wait) => {}
            }
            let login = *logins.borrow_and_update();
            if current.minted_under != login {
                info!(
                    lease_id = %current.lease_id,
                    "vault client logged in again; rotating the db credential onto the new token"
                );
            }

            if current.extendable(login) {
                match vault
                    .renew_lease(&current.lease_id, current.requested)
                    .await
                {
                    Ok(granted) if granted >= current.requested => {
                        current.ttl = granted.max(1);
                        debug!(lease_id = %current.lease_id, ttl = current.ttl, "renewed vault db lease");
                        retry = RETRY_FLOOR;
                        wait = renew_delay(current.ttl);
                        continue;
                    }
                    Ok(granted) => {
                        info!(
                            lease_id = %current.lease_id,
                            granted,
                            requested = current.requested,
                            "vault db lease is near its maximum TTL; rotating before it expires"
                        );
                        current.ttl = granted.max(1);
                        current.at_max = true;
                    }
                    Err(failure) => {
                        warn!(
                            lease_id = %current.lease_id,
                            code = failure.error_code(),
                            error = %failure,
                            "vault db lease renewal failed; rotating"
                        );
                    }
                }
            }

            let fresh = match vault.dynamic_db_creds(&mount, &role).await {
                Ok(fresh) => fresh,
                Err(failure) => {
                    error!(
                        role,
                        code = failure.error_code(),
                        error = %failure,
                        retry_secs = retry.as_secs(),
                        "failed to mint a new db credential; the current one stays in use"
                    );
                    wait = retry;
                    retry = (retry * 2).min(RETRY_CEILING);
                    continue;
                }
            };
            let minted = CurrentLease::of(&fresh);
            match rebuild(fresh).await {
                Ok(()) => {
                    let replaced = std::mem::replace(&mut current, minted);
                    info!(
                        role,
                        lease_id = %current.lease_id,
                        ttl = current.ttl,
                        "rotated vault db credential"
                    );
                    revoke(&vault, &replaced.lease_id, "the credential it replaced").await;
                    retry = RETRY_FLOOR;
                    wait = renew_delay(current.ttl);
                }
                Err(failure) => {
                    error!(
                        role,
                        code = failure.error_code(),
                        error = %failure,
                        retry_secs = retry.as_secs(),
                        "the pool was not rebuilt on the rotated credential; the current one stays in use"
                    );
                    revoke(&vault, &minted.lease_id, "the credential no pool is using").await;
                    wait = retry;
                    retry = (retry * 2).min(RETRY_CEILING);
                }
            }
        }
    });

    RenewerHandle {
        handle,
        stop,
        stopped,
    }
}

/// Revokes `lease_id`, logging the outcome.
#[expect(clippy::cognitive_complexity, reason = "readability baseline")]
async fn revoke(vault: &VaultClient, lease_id: &str, which: &str) {
    match vault.revoke_lease(lease_id).await {
        Ok(()) => info!(lease_id, which, "revoked vault db lease"),
        Err(failure) => warn!(
            lease_id,
            which,
            code = failure.error_code(),
            error = %failure,
            "could not revoke a vault db lease; it stays valid until its maximum TTL"
        ),
    }
}

#[cfg(test)]
mod tests {
    use fundamental_core::Error as _;
    use serde_json::json;

    use super::*;

    fn answer() -> Value {
        json!({
            "lease_id": "database/creds/app/AbC123",
            "lease_duration": 3600,
            "renewable": true,
            "data": { "username": "v-app-1", "password": "pw" },
        })
    }

    #[test]
    fn a_lease_is_renewed_until_vault_grants_less_than_asked() {
        let credential =
            LeasedCredential::from_response("database/creds/app", &answer(), 0).expect("whole");
        let mut lease = CurrentLease::of(&credential);
        assert!(lease.extendable(0), "a fresh renewable lease is renewed");
        assert!(
            !lease.extendable(1),
            "a lease minted under an earlier login is moved, not renewed"
        );

        lease.at_max = true;
        assert!(
            !lease.extendable(0),
            "a lease at its maximum TTL is rotated, not renewed"
        );

        let mut fixed = CurrentLease::of(&credential);
        fixed.renewable = false;
        assert!(
            !fixed.extendable(0),
            "a lease Vault will not renew is rotated"
        );
    }

    #[test]
    fn a_whole_answer_is_a_credential() {
        let credential =
            LeasedCredential::from_response("database/creds/app", &answer(), 0).expect("whole");
        assert_eq!(credential.lease_id, "database/creds/app/AbC123");
        assert_eq!(credential.lease_duration, 3600);
        assert!(credential.renewable);
        assert_eq!(credential.username.expose(), "v-app-1");
    }

    #[test]
    fn a_lease_nobody_could_manage_is_refused_rather_than_defaulted() {
        for (field, value) in [
            ("lease_id", Value::Null),
            ("lease_id", json!("")),
            ("lease_duration", Value::Null),
            ("renewable", Value::Null),
        ] {
            let mut answer = answer();
            answer[field] = value;
            let refused = LeasedCredential::from_response("database/creds/app", &answer, 0)
                .expect_err("an incomplete lease is refused");
            assert_eq!(refused.error_code(), "SECRETS_005");
            assert!(refused.to_string().contains(field), "{refused}");
        }
    }

    #[test]
    fn renewal_waits_two_thirds_of_the_lease() {
        assert_eq!(renew_delay(30), Duration::from_secs(20));
        assert_eq!(renew_delay(0), Duration::from_secs(1));
    }
}
