use core::fmt;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use fundamental_db::ephemeral::{HeldName, stale_cutoff_millis};
use sqlx::postgres::PgConnection;
use tokio::sync::oneshot;
use tokio::time::timeout;
use tracing::{Span, debug, instrument, warn};

use super::connect_dedicated::{close_dedicated, connect_dedicated};
use super::drop_schema_on::drop_schema_on;
use super::drop_stale_ephemeral_schemas::sweep_on;
use super::first_sweep_in::first_sweep_in;
use super::sweep_schemas::log_sweep;
use super::try_advisory_lock::try_advisory_lock;
use super::{ReclaimedSchemaError, SweepBudget, drop_test_schema};

/// A PostgreSQL identifier's limit, which a schema name shares with a database name.
const MAX_SCHEMA_NAME: usize = 63;

/// How many fresh names a claim tries; with 60 random bits, a second attempt already means trouble.
const CLAIM_ATTEMPTS: u32 = 3;

/// How long the holder may take to connect and take its lock before the claim reads it as unreachable.
const HOLD_BUDGET: Duration = Duration::from_secs(30);

/// How long the holder's `DROP SCHEMA` may take at teardown, retries included.
const TEARDOWN_BUDGET: Duration = Duration::from_secs(120);

/// A held schema name: reclaimed when it drops, and provably in use until then.
///
/// A holder thread's own connection keeps a session advisory lock keyed by the name until after
/// the schema is dropped, so a sweep that finds the lock free knows the run is gone, even after
/// a kill.
///
/// # Example
///
/// ```no_run
/// # use fundamental_sql::test_support::ReclaimedSchema;
/// # async fn provision(app_url: &str) -> Result<(), Box<dyn std::error::Error>> {
/// // Nothing exists in the database yet; the name is accounted for and already held.
/// let reclaimed = ReclaimedSchema::claim(app_url, "layer5_test_").await?;
///
/// // Every failure from here on reclaims, including a panic — and a kill is reclaimed by the
/// // next fixture's sweep as soon as this process is gone.
/// create_schema(reclaimed.name()).await?;
/// migrate(reclaimed.name()).await?;
/// # Ok(())
/// # }
/// # async fn create_schema(_: &str) -> Result<(), Box<dyn std::error::Error>> { Ok(()) }
/// # async fn migrate(_: &str) -> Result<(), Box<dyn std::error::Error>> { Ok(()) }
/// ```
pub struct ReclaimedSchema {
    name: HeldName,
    /// A string, never a live pool, which would be carried across runtimes into `Drop`.
    database_url: String,
    holder: Option<Holder>,
}

struct Holder {
    release: Sender<()>,
    thread: JoinHandle<()>,
}

enum HoldAnswer {
    Held,
    Contended,
    Unreachable(String),
    Refused(String),
}

impl ReclaimedSchema {
    /// Mints a held name, takes its lock, and takes ownership of removing whatever is created
    /// under it; the first claim in a database also sweeps it.
    ///
    /// # Errors
    /// [`ReclaimedSchemaError::InvalidName`] if the prefix cannot mint a held name;
    /// [`Unreachable`](ReclaimedSchemaError::Unreachable) if the holder cannot connect;
    /// [`NotHeld`](ReclaimedSchemaError::NotHeld) if the lock could not be taken.
    #[instrument(skip_all, fields(prefix = prefix))]
    pub async fn claim(
        database_url: impl Into<String>,
        prefix: &str,
    ) -> Result<Self, ReclaimedSchemaError> {
        let database_url = database_url.into();
        let mut name = HeldName::mint(prefix, MAX_SCHEMA_NAME)?;
        let mut sweep = first_sweep_in(&database_url);
        for attempt in 1..=CLAIM_ATTEMPTS {
            let (answer, holder) = hold(&database_url, &name, sweep).await;
            sweep = false;
            match answer {
                HoldAnswer::Held => {
                    debug!(schema = %name, "test support: schema name claimed and held");
                    return Ok(Self {
                        name,
                        database_url,
                        holder,
                    });
                }
                HoldAnswer::Unreachable(detail) => {
                    return Err(ReclaimedSchemaError::Unreachable {
                        name: name.to_string(),
                        detail,
                    });
                }
                HoldAnswer::Refused(detail) => {
                    return Err(ReclaimedSchemaError::NotHeld {
                        name: name.to_string(),
                        detail,
                    });
                }
                HoldAnswer::Contended if attempt < CLAIM_ATTEMPTS => {
                    warn!(
                        schema = %name, attempt, of = CLAIM_ATTEMPTS,
                        "test support: a freshly minted schema key was already held; minting another"
                    );
                    name = HeldName::mint(prefix, MAX_SCHEMA_NAME)?;
                }
                HoldAnswer::Contended => {}
            }
        }
        Err(ReclaimedSchemaError::NotHeld {
            name: name.to_string(),
            detail: format!("{CLAIM_ATTEMPTS} fresh keys were each already held"),
        })
    }

    /// The schema name, for the DDL that is about to create it.
    #[must_use]
    pub fn name(&self) -> &str {
        self.name.as_str()
    }

    /// The database this schema is created in and removed from.
    #[must_use]
    pub fn database_url(&self) -> &str {
        &self.database_url
    }
}

/// Starts the holder thread for `name`; returns the holder only when it holds the lock.
async fn hold(database_url: &str, name: &HeldName, sweep: bool) -> (HoldAnswer, Option<Holder>) {
    let (answer_tx, answer_rx) = oneshot::channel();
    let (release, released) = mpsc::channel();
    let url = database_url.to_owned();
    let held = name.clone();
    let span = Span::current();
    let spawned = std::thread::Builder::new()
        .name("schema-holder".to_owned())
        .spawn(move || {
            let _claim = span.entered();
            hold_until_released(&url, &held, sweep, answer_tx, &released);
        });
    let thread = match spawned {
        Ok(thread) => thread,
        Err(error) => {
            return (
                HoldAnswer::Refused(format!("the holder thread could not start: {error}")),
                None,
            );
        }
    };
    match answer_rx.await {
        Ok(HoldAnswer::Held) => (HoldAnswer::Held, Some(Holder { release, thread })),
        Ok(other) => {
            join_holder(thread, name);
            (other, None)
        }
        Err(_) => {
            join_holder(thread, name);
            (
                HoldAnswer::Refused("the holder thread ended without answering".to_owned()),
                None,
            )
        }
    }
}

/// The holder thread; a closed release channel counts as a release, so it never outlives its guard.
#[expect(
    clippy::too_many_lines,
    clippy::cognitive_complexity,
    reason = "readability baseline"
)]
fn hold_until_released(
    database_url: &str,
    name: &HeldName,
    sweep: bool,
    answer: oneshot::Sender<HoldAnswer>,
    released: &Receiver<()>,
) {
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            answer_abandoned(
                answer.send(HoldAnswer::Refused(format!(
                    "the holder runtime could not be built: {error}"
                ))),
                name,
            );
            return;
        }
    };

    let connection = runtime.block_on(async {
        let mut connection =
            match timeout(HOLD_BUDGET, connect_dedicated(database_url, name.as_str())).await {
                Ok(Ok(connection)) => connection,
                Ok(Err(error)) => return Err(HoldAnswer::Unreachable(error.to_string())),
                Err(_) => {
                    return Err(HoldAnswer::Unreachable(format!(
                        "no connection within {HOLD_BUDGET:?}"
                    )));
                }
            };
        if sweep {
            sweep_here(&mut connection, database_url).await;
        }
        let held = timeout(
            HOLD_BUDGET,
            try_advisory_lock(&mut connection, name.advisory_key()),
        )
        .await
        .map(|taken| (connection, taken))
        .map_err(|_| HoldAnswer::Unreachable(format!("no lock within {HOLD_BUDGET:?}")));
        match held {
            Err(refusal) => Err(refusal),
            Ok((connection, Ok(true))) => Ok(connection),
            Ok((connection, Ok(false))) => {
                close_dedicated(connection).await;
                Err(HoldAnswer::Contended)
            }
            Ok((connection, Err(error))) => {
                close_dedicated(connection).await;
                Err(HoldAnswer::Refused(error.to_string()))
            }
        }
    });
    let connection = match connection {
        Ok(connection) => connection,
        Err(refusal) => {
            answer_abandoned(answer.send(refusal), name);
            return;
        }
    };
    answer_abandoned(answer.send(HoldAnswer::Held), name);

    match released.recv() {
        Ok(()) => debug!(schema = %name, "test support: the schema's guard released it"),
        Err(_) => debug!(
            schema = %name,
            "test support: the schema's guard is gone without a release, which is a release"
        ),
    }

    // The schema goes first, while the lock still says its run is alive.
    let dropped = runtime.block_on(async {
        let mut connection = connection;
        let dropped = drop_schema_on(&mut connection, name.as_str(), TEARDOWN_BUDGET).await;
        close_dedicated(connection).await;
        dropped
    });
    if let Err(error) = dropped {
        debug!(
            schema = %name, %error,
            "test support: the holder could not drop its schema on its own session; dropping it on a fresh one"
        );
        drop_test_schema(database_url, name.as_str());
    }
}

/// Sweeps the database, logs what it did, and releases every lock the sweep took; never fails the claim.
async fn sweep_here(connection: &mut PgConnection, database_url: &str) {
    let started = Instant::now();
    let swept = sweep_on(
        connection,
        database_url,
        stale_cutoff_millis(),
        SweepBudget::ON_PROVISION,
    )
    .await;
    log_sweep(database_url, swept, started);
    if let Err(error) = sqlx::query("SELECT pg_advisory_unlock_all()")
        .execute(&mut *connection)
        .await
    {
        warn!(%error, "test support: the sweep's locks could not be released on the holder's session");
    }
}

/// Logs an answer the claim was no longer waiting for; the holder still drops the schema itself.
#[expect(clippy::cognitive_complexity, reason = "readability baseline")]
fn answer_abandoned(sent: Result<(), HoldAnswer>, name: &HeldName) {
    match sent {
        Ok(()) => {}
        Err(HoldAnswer::Held) => debug!(
            schema = %name,
            "test support: the claim was abandoned before it was answered; releasing"
        ),
        Err(HoldAnswer::Contended) => debug!(
            schema = %name,
            "test support: the claim was abandoned before hearing its key was already held"
        ),
        Err(HoldAnswer::Unreachable(detail) | HoldAnswer::Refused(detail)) => debug!(
            schema = %name, %detail,
            "test support: the claim was abandoned before its refusal was read"
        ),
    }
}

fn join_holder(thread: JoinHandle<()>, name: &HeldName) {
    if thread.join().is_err() {
        warn!(
            schema = %name,
            "test support: the schema holder thread panicked; the next sweep reclaims the schema once its lock is free"
        );
    }
}

/// Prints the name and never the URL, which carries credentials.
impl fmt::Debug for ReclaimedSchema {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ReclaimedSchema")
            .field("name", &self.name.as_str())
            .finish_non_exhaustive()
    }
}

impl Drop for ReclaimedSchema {
    fn drop(&mut self) {
        // Joined, so the drop is the schema's removal; bounded, and never panics while unwinding.
        if let Some(Holder { release, thread }) = self.holder.take() {
            if release.send(()).is_err() {
                debug!(
                    schema = %self.name,
                    "test support: the schema holder had already ended before its release"
                );
            }
            join_holder(thread, &self.name);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOWHERE: &str = "postgresql://yugabyte@127.0.0.1:1/base?sslmode=disable";

    #[tokio::test]
    async fn an_unsafe_prefix_is_refused_before_anything_is_contacted() {
        let error = ReclaimedSchema::claim(NOWHERE, "x; DROP SCHEMA prod; --")
            .await
            .expect_err("the prefix is refused");
        assert!(
            matches!(error, ReclaimedSchemaError::InvalidName { .. }),
            "an `Unreachable` here would mean the name was built after a connection: {error:?}"
        );
    }

    #[tokio::test]
    async fn a_prefix_too_long_for_a_held_name_is_refused_before_any_ddl() {
        let prefix = format!("{}_", "a".repeat(60));
        let error = ReclaimedSchema::claim(NOWHERE, &prefix)
            .await
            .expect_err("the name would exceed the identifier limit");
        assert!(matches!(error, ReclaimedSchemaError::InvalidName { .. }));
    }

    #[tokio::test]
    async fn a_prefix_that_would_not_be_sweepable_is_refused() {
        let error = ReclaimedSchema::claim(NOWHERE, "layer5_test")
            .await
            .expect_err("a prefix must carry its separator");
        assert!(matches!(error, ReclaimedSchemaError::InvalidName { .. }));
    }

    #[tokio::test]
    async fn an_unreachable_database_is_reported_as_unreachable() {
        let error = ReclaimedSchema::claim(NOWHERE, "layer5_test_")
            .await
            .expect_err("nothing listens there");
        assert!(error.is_unreachable(), "{error:?}");
    }
}
