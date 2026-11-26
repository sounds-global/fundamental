//! [`Inbox`] — idempotent consumers over one database.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use chrono::{DateTime, TimeDelta, Utc};
use fundamental_core::model::{BoundTo, HasLeadingKey, LeadingKeyAddressed, Satisfied, SqlDb};
use fundamental_db::{
    DatabaseError, Filter, FilterValue, IsolationLevel, KeyedRepositoryTransaction, OrderBy,
    QueryBuilder, QueryExecutor as _, Repository as _, RepositoryTransaction as _,
    RepositoryTransactionQuery,
};
use fundamental_nats::{AckPolicy, Event, JetStream, Listener};
use fundamental_sql::{ClusterTime, SqlPool, SqlTransaction};
use tracing::{info, instrument, warn};
use uuid::Uuid;

use crate::dispatch::Dispatch;
use crate::effect_row::EffectRow;
use crate::inbox_row::InboxRow;
use crate::parked_row::ParkedRow;
use crate::watermark::{self, Watermark};
use crate::{
    Binding, EffectName, InboxConfig, InboxError, ParkedAddress, ParkedEvent, Step, SweepReport,
};

/// How many parked records one listing returns at most.
pub const PARKED_PAGE: u64 = 500;

/// Idempotent consumers over one database: every effect a listener applies commits in one
/// transaction with the record that it ran, so a redelivered, replayed or concurrent copy of
/// an event never has a second effect.
#[derive(Clone)]
pub struct Inbox {
    pool: SqlPool,
    config: InboxConfig,
    effects: Arc<Mutex<BTreeSet<EffectName>>>,
}

impl Inbox {
    /// An inbox over `pool`'s database, after checking its tables are there.
    ///
    /// # Errors
    ///
    /// [`InboxError::TableMissing`] without the inbox tables, or [`InboxError::Unavailable`].
    #[instrument(name = "inbox.open", skip_all, err(Debug))]
    pub async fn open(pool: SqlPool, config: InboxConfig) -> Result<Self, InboxError> {
        let probe = pool.repository::<InboxRow>().query().pin_key("-").limit(1);
        pool.query_executor::<InboxRow>()
            .execute(&probe)
            .await
            .map_err(InboxError::from_store)?;
        let probe = pool.repository::<ParkedRow>().query().pin_key("-").limit(1);
        pool.query_executor::<ParkedRow>()
            .execute(&probe)
            .await
            .map_err(InboxError::from_store)?;
        Ok(Self {
            pool,
            config,
            effects: Arc::new(Mutex::new(BTreeSet::new())),
        })
    }

    /// The settings this inbox runs with.
    #[must_use]
    pub fn config(&self) -> &InboxConfig {
        &self.config
    }

    /// Starts applying `binding`'s effect to every delivery of its consumer, through `step`.
    ///
    /// Refuses a consumer that does not acknowledge each delivery explicitly, or that has a
    /// delivery limit: the inbox owns the attempt budget.
    ///
    /// # Errors
    ///
    /// [`InboxError::EffectDeclaredTwice`], [`InboxError::UnsafeConsumer`], or
    /// [`InboxError::Broker`] when the stream or consumer cannot be read.
    #[instrument(
        name = "inbox.listen",
        skip_all,
        fields(effect = %binding.effect(), stream = binding.stream(), consumer = binding.consumer()),
        err(Debug)
    )]
    pub async fn listen<M, E>(
        &self,
        jetstream: &JetStream,
        binding: Binding,
        step: Step<M, E>,
    ) -> Result<Listener, InboxError>
    where
        M: Event + Send + Sync + 'static,
        E: fundamental_core::Error + From<DatabaseError> + Send + 'static,
    {
        let effect = binding.effect();
        if !self.effects().insert(effect) {
            return Err(InboxError::EffectDeclaredTwice {
                effect: effect.to_string(),
                detail: "this inbox already has a listener for it".to_owned(),
            });
        }
        let started = match self.claim(&binding).await {
            Ok(()) => self.start(jetstream, binding, step).await,
            Err(error) => Err(error),
        };
        if started.is_err() {
            self.effects().remove(&effect);
        }
        started
    }

    async fn start<M, E>(
        &self,
        jetstream: &JetStream,
        binding: Binding,
        step: Step<M, E>,
    ) -> Result<Listener, InboxError>
    where
        M: Event + Send + Sync + 'static,
        E: fundamental_core::Error + From<DatabaseError> + Send + 'static,
    {
        let broker = |cause| InboxError::Broker { cause };
        let mut stream = jetstream
            .get_stream(binding.stream())
            .await
            .map_err(broker)?;
        let stream_info = stream.info().await.map_err(broker)?;
        let mut consumer = stream
            .get_consumer(binding.consumer())
            .await
            .map_err(broker)?;
        let info = consumer.info().await.map_err(broker)?;
        let mut problems = Vec::new();
        if info.ack_policy != AckPolicy::Explicit {
            problems.push(format!(
                "its ack policy is {:?}; the inbox needs Explicit",
                info.ack_policy
            ));
        }
        if info.max_deliver > 0 {
            problems.push(format!(
                "it gives up after {} deliveries; the inbox owns the attempt budget, so max_deliver must be unlimited",
                info.max_deliver
            ));
        }
        if !problems.is_empty() {
            return Err(InboxError::UnsafeConsumer {
                stream: binding.stream().to_owned(),
                consumer: binding.consumer().to_owned(),
                problems: problems.join("; "),
            });
        }
        let deliveries = consumer.deliveries().await.map_err(broker)?;
        let heartbeat = (info.ack_wait / 3).max(Duration::from_millis(100));
        info!(
            event = "inbox.listener_started",
            retention_secs = self.config.retention().as_secs(),
            concurrency = self.config.concurrency(),
            max_attempts = self.config.max_attempts(),
            effect_deadline_secs = self.config.effect_deadline().as_secs(),
            ack_wait_ms = u64::try_from(info.ack_wait.as_millis()).unwrap_or(u64::MAX),
            isolation = binding.isolation().as_sql(),
            "inbox listener started"
        );
        let dispatch = Arc::new(Dispatch {
            pool: self.pool.clone(),
            config: self.config,
            binding,
            step,
            stream_created_at: stream_info.created,
            heartbeat,
            watermark: Mutex::new(None),
        });
        Ok(Listener::spawn(move |drain| {
            dispatch.run(deliveries, drain)
        }))
    }

    /// Deletes, for every effect listened for, the inbox rows and parked records past their
    /// windows, each statement bounded to one batch.
    ///
    /// # Errors
    ///
    /// [`InboxError::Unavailable`] or [`InboxError::TableMissing`]; rows already deleted stay
    /// deleted.
    #[instrument(name = "inbox.sweep", skip_all, err(Debug))]
    #[expect(clippy::too_many_lines, reason = "readability baseline")]
    pub async fn sweep(&self) -> Result<SweepReport, InboxError> {
        let effects: Vec<EffectName> = self.effects().iter().copied().collect();
        let budget = self.config.sweep_every() / 2;
        let started = Instant::now();
        let now = ClusterTime::read(&self.pool)
            .await
            .map_err(|error| InboxError::from_store(DatabaseError::from(error)))?
            .to_utc();
        let parked_cutoff = before(now, self.config.parked_retention());
        let mut report = SweepReport {
            effects: effects.len(),
            ..SweepReport::default()
        };
        for effect in effects {
            let effect_started = Instant::now();
            let mut deleted = 0;
            let mut batches = 0;
            let mut behind = false;
            // Rows are deleted only below the watermark every replica has had a grace period to
            // learn — never below the one this pass may have just published.
            let delete_below = self.advance_watermark(effect).await?;
            let cutoff = delete_below.unwrap_or(DateTime::<Utc>::MIN_UTC);
            let first = usize::from(Uuid::new_v4().as_bytes()[0] % 64);
            'buckets: for offset in 0..64 {
                if delete_below.is_none() {
                    break;
                }
                let bucket = i32::try_from((first + offset) % 64).unwrap_or_default();
                loop {
                    if started.elapsed() >= budget {
                        behind = true;
                        break 'buckets;
                    }
                    let batch = self
                        .pool
                        .repository::<InboxRow>()
                        .query()
                        .pin_key(effect.as_str())
                        .filter(Filter::eq(InboxRow::SWEEP_BUCKET, bucket))
                        .filter(Filter::lt(
                            InboxRow::PROCESSED_AT,
                            FilterValue::TimestampTz(cutoff),
                        ))
                        .order_by(OrderBy::asc(InboxRow::PROCESSED_AT))
                        .limit(u64::from(self.config.sweep_batch()));
                    let removed = self.delete(&batch).await?;
                    deleted += removed;
                    batches += 1;
                    if removed < u64::from(self.config.sweep_batch()) {
                        break;
                    }
                }
            }
            let mut parked_deleted = 0;
            loop {
                if started.elapsed() >= budget {
                    behind = true;
                    break;
                }
                let batch = self
                    .pool
                    .repository::<ParkedRow>()
                    .query()
                    .pin_key(effect.as_str())
                    .filter(Filter::lt(
                        ParkedRow::PARKED_AT,
                        FilterValue::TimestampTz(parked_cutoff),
                    ))
                    .order_by(OrderBy::asc(ParkedRow::PARKED_AT))
                    .limit(u64::from(self.config.sweep_batch()));
                let removed = self.delete(&batch).await?;
                parked_deleted += removed;
                batches += 1;
                if removed < u64::from(self.config.sweep_batch()) {
                    break;
                }
            }
            let elapsed_ms =
                u64::try_from(effect_started.elapsed().as_millis()).unwrap_or(u64::MAX);
            if behind {
                warn!(event = "inbox.swept", effect = %effect, deleted, parked_deleted, batches, %cutoff, elapsed_ms, behind, "the sweep's time budget ran out with rows left; the next pass continues");
            } else {
                info!(event = "inbox.swept", effect = %effect, deleted, parked_deleted, batches, %cutoff, elapsed_ms, behind, "swept");
            }
            report.deleted += deleted;
            report.parked_deleted += parked_deleted;
            report.batches += batches;
            report.behind |= behind;
        }
        Ok(report)
    }

    async fn delete<M>(
        &self,
        batch: &QueryBuilder<M, SqlDb, HasLeadingKey>,
    ) -> Result<u64, InboxError>
    where
        M: BoundTo<SqlDb> + Send + Sync,
        SqlTransaction: RepositoryTransactionQuery<M, SqlDb>,
        LeadingKeyAddressed<M, SqlDb, HasLeadingKey>: Satisfied,
    {
        self.pool
            .transaction(IsolationLevel::ReadCommitted, async |tx| {
                tx.delete_where(batch).await
            })
            .await
            .map_err(InboxError::from_store)
    }

    /// The oldest parked records for `effect`, at most [`PARKED_PAGE`].
    ///
    /// # Errors
    ///
    /// [`InboxError::InvalidEffectName`], or [`InboxError::Unavailable`].
    #[instrument(name = "inbox.parked", skip(self), err(Debug))]
    pub async fn parked(&self, effect: &str) -> Result<Vec<ParkedEvent>, InboxError> {
        if !EffectName::is_valid(effect) {
            return Err(InboxError::InvalidEffectName {
                name: effect.to_owned(),
            });
        }
        let query = self
            .pool
            .repository::<ParkedRow>()
            .query()
            .pin_key(effect)
            .order_by(OrderBy::asc(ParkedRow::PARKED_AT))
            .limit(PARKED_PAGE);
        let rows = self
            .pool
            .query_executor::<ParkedRow>()
            .execute(&query)
            .await
            .map_err(InboxError::from_store)?;
        Ok(rows.into_iter().map(ParkedEvent::from).collect())
    }

    /// Removes the parked record at `address`, once an operator has acted on it.
    ///
    /// # Errors
    ///
    /// [`InboxError::ParkedNotFound`] when nothing is parked there, or
    /// [`InboxError::Unavailable`].
    #[instrument(name = "inbox.resolve", skip(self), fields(address = %address), err(Debug))]
    pub async fn resolve(&self, address: &ParkedAddress) -> Result<(), InboxError> {
        let key = (
            address.effect.clone(),
            address.stream.clone(),
            address.stream_created_at,
            i64::try_from(address.stream_sequence).unwrap_or(i64::MAX),
        );
        let query = self
            .pool
            .repository::<ParkedRow>()
            .query()
            .pin_key(address.effect.as_str())
            .filter(Filter::eq(ParkedRow::STREAM, key.1.as_str()))
            .filter(Filter::eq(
                ParkedRow::STREAM_CREATED_AT,
                FilterValue::TimestampTz(key.2),
            ))
            .filter(Filter::eq(ParkedRow::STREAM_SEQUENCE, key.3));
        let removed = self
            .pool
            .transaction(IsolationLevel::ReadCommitted, async |tx| {
                tx.delete_where(&query).await
            })
            .await
            .map_err(InboxError::from_store)?;
        if removed == 0 {
            return Err(InboxError::ParkedNotFound {
                address: address.to_string(),
            });
        }
        info!(event = "inbox.resolved", "a parked record was resolved");
        Ok(())
    }

    /// Claims `binding`'s effect name for its handler; a different handler holding it is refused.
    async fn claim(&self, binding: &Binding) -> Result<(), InboxError> {
        let effect = binding.effect();
        let handler = binding.handler();
        let holder = self
            .pool
            .transaction(IsolationLevel::ReadCommitted, async |tx| {
                let now = ClusterTime::read_in(tx)
                    .await
                    .map_err(DatabaseError::from)?
                    .to_utc();
                tx.insert_if_absent(&EffectRow {
                    effect: effect.as_str().to_owned(),
                    handler: handler.to_owned(),
                    covered_from: None,
                    covered_from_set_at: None,
                    delete_below: None,
                    registered_at: now,
                })
                .await?;
                let row = KeyedRepositoryTransaction::<EffectRow, SqlDb>::find(
                    tx,
                    &effect.as_str().to_owned(),
                )
                .await?;
                Ok::<_, DatabaseError>(row.map(|row| row.handler))
            })
            .await
            .map_err(InboxError::from_store)?;
        match holder {
            Some(holder) if holder == handler => Ok(()),
            Some(holder) => Err(InboxError::EffectDeclaredTwice {
                effect: effect.to_string(),
                detail: format!(
                    "it is applied by `{holder}`, not `{handler}`; give this handler its own effect name, or, if it replaces that one, run `fun run inbox:rehome -- {effect} {handler}`"
                ),
            }),
            None => Err(InboxError::Unavailable {
                cause: DatabaseError::internal("the effect row was not found after it was claimed"),
            }),
        }
    }

    /// Moves `effect`'s watermark on: the previous one becomes the delete line once it has stood
    /// for [`GRACE`](watermark::GRACE). Returns the line to delete below, if any.
    #[expect(clippy::excessive_nesting, reason = "readability baseline")]
    async fn advance_watermark(
        &self,
        effect: EffectName,
    ) -> Result<Option<DateTime<Utc>>, InboxError> {
        let retention = self.config.retention();
        self.pool
            .transaction(IsolationLevel::ReadCommitted, async |tx| {
                let now = ClusterTime::read_in(tx)
                    .await
                    .map_err(DatabaseError::from)?
                    .to_utc();
                let Some(mut row) = tx
                    .find_for_update::<EffectRow>(&effect.as_str().to_owned())
                    .await?
                else {
                    return Ok::<_, DatabaseError>(None);
                };
                let settled = row
                    .covered_from_set_at
                    .is_none_or(|set_at| now.signed_duration_since(set_at) >= watermark::GRACE);
                if settled {
                    if row.covered_from > row.delete_below {
                        row.delete_below = row.covered_from;
                    }
                    let target = before(now, retention);
                    if row.covered_from.is_none_or(|covered| target > covered) {
                        row.covered_from = Some(target);
                        row.covered_from_set_at = Some(now);
                    }
                    tx.update(&row).await?;
                }
                Ok(row.delete_below)
            })
            .await
            .map_err(InboxError::from_store)
    }

    /// Hands `effect` to `handler`, after a handler that applied it was renamed or moved.
    ///
    /// # Errors
    ///
    /// [`InboxError::EffectNotFound`] when no effect has that name, or
    /// [`InboxError::Unavailable`].
    #[instrument(name = "inbox.rehome", skip(self), err(Debug))]
    pub async fn rehome(&self, effect: &str, handler: &str) -> Result<(), InboxError> {
        let updated = self
            .pool
            .transaction(IsolationLevel::ReadCommitted, async |tx| {
                let Some(mut row) = tx.find_for_update::<EffectRow>(&effect.to_owned()).await?
                else {
                    return Ok::<_, DatabaseError>(None);
                };
                let previous = std::mem::replace(&mut row.handler, handler.to_owned());
                tx.update(&row).await?;
                Ok(Some(previous))
            })
            .await
            .map_err(InboxError::from_store)?;
        match updated {
            Some(previous) => {
                info!(
                    event = "inbox.rehomed",
                    effect, previous, handler, "an effect was handed to another handler"
                );
                Ok(())
            }
            None => Err(InboxError::EffectNotFound {
                effect: effect.to_owned(),
            }),
        }
    }

    fn effects(&self) -> std::sync::MutexGuard<'_, BTreeSet<EffectName>> {
        self.effects.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl std::fmt::Debug for Inbox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Inbox")
            .field("config", &self.config)
            .field("effects", &*self.effects())
            .finish_non_exhaustive()
    }
}

fn before(now: DateTime<Utc>, window: Duration) -> DateTime<Utc> {
    now - TimeDelta::from_std(window).unwrap_or(TimeDelta::MAX)
}

/// Reads `effect`'s watermark, noting the database's clock and this process's just before.
pub(crate) async fn read_watermark(
    pool: &SqlPool,
    effect: EffectName,
) -> Result<Watermark, InboxError> {
    let read_locally = Instant::now();
    pool.transaction(IsolationLevel::ReadCommitted, async |tx| {
        let read_at = ClusterTime::read_in(tx)
            .await
            .map_err(DatabaseError::from)?
            .to_utc();
        let row =
            KeyedRepositoryTransaction::<EffectRow, SqlDb>::find(tx, &effect.as_str().to_owned())
                .await?;
        Ok::<_, DatabaseError>(Watermark {
            covered_from: row.and_then(|row| row.covered_from),
            read_at,
            read_locally,
        })
    })
    .await
    .map_err(InboxError::from_store)
}
