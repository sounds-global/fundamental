use std::borrow::Cow;

use chrono::{DateTime, Utc};
use fundamental_core::model::{HasLeadingKey, SqlDb};
use fundamental_db::{Filter, FilterValue, OutboxRow, QueryBuilder};
use fundamental_macros::Model;
use uuid::Uuid;

use crate::{JobBucket, JobId, JobScope};

/// One durable job as stored: what it is (sealed) and when it is due, attempted and fired.
#[derive(Clone, Debug, PartialEq, Model)]
#[model(
    sql,
    outbox,
    table = "fundamental_jobs",
    index(name = "fundamental_jobs_claim", columns = ["bucket asc", "next_attempt_at asc"], partial = "fired_at IS NULL AND outbox_set_aside_at IS NULL AND outbox_skip_until IS NULL"),
    index(name = "fundamental_jobs_overdue", columns = ["bucket asc", "due_at asc"], partial = "fired_at IS NULL"),
    index(name = "fundamental_jobs_fired", columns = ["bucket asc", "fired_at asc"], partial = "fired_at IS NOT NULL"),
    index(name = "fundamental_jobs_skipped", columns = ["bucket asc", "outbox_skip_until asc"], partial = "fired_at IS NULL AND outbox_set_aside_at IS NULL AND outbox_skip_until IS NOT NULL"),
    index(name = "fundamental_jobs_set_aside", columns = ["bucket asc", "outbox_set_aside_at asc"], partial = "outbox_set_aside_at IS NOT NULL"),
    check(name = "fundamental_jobs_bucket", expr = r#"bucket BETWEEN 0 AND 63"#),
    check(name = "fundamental_jobs_fired_not_before_due", expr = r#"fired_at IS NULL OR fired_at >= due_at"#),
    check(name = "fundamental_jobs_kind", expr = r#"kind ~ '^[a-z][a-z0-9._-]{0,63}$'"#),
    check(name = "fundamental_jobs_last_error", expr = r#"last_error IS NULL OR char_length(last_error) <= 2000"#),
    check(name = "fundamental_jobs_subject", expr = r#"char_length(subject) BETWEEN 1 AND 256 AND subject ~ '^[A-Za-z0-9._:/-]+$'"#)
)]
pub struct ScheduledJob {
    // The stored UUID, not `JobId`: the derive infers a newtype's column as TEXT.
    #[model(id)]
    pub(crate) job_id: Uuid,
    pub(crate) bucket: i16,
    pub(crate) kind: String,
    pub(crate) subject: String,
    #[model(unbound)]
    pub(crate) due_at: DateTime<Utc>,
    #[model(unbound)]
    pub(crate) next_attempt_at: DateTime<Utc>,
    #[model(unbound)]
    pub(crate) attempts: i64,
    #[model(unbound)]
    pub(crate) last_error: Option<String>,
    #[model(unbound)]
    pub(crate) fired_at: Option<DateTime<Utc>>,
}

impl ScheduledJob {
    /// A pending job of `kind` about `subject`, due at `due`.
    pub(crate) fn pending(kind: &str, subject: &str, due: DateTime<Utc>) -> Self {
        Self {
            job_id: JobId::of_names(kind, subject).as_uuid(),
            bucket: JobBucket::of_name(subject).get(),
            kind: kind.to_owned(),
            subject: subject.to_owned(),
            due_at: due,
            next_attempt_at: due,
            attempts: 0,
            last_error: None,
            fired_at: None,
        }
    }

    /// The job's key.
    #[must_use]
    pub fn job_id(&self) -> JobId {
        JobId::from(self.job_id)
    }

    /// The job's kind, as stored.
    #[must_use]
    pub fn kind(&self) -> &str {
        &self.kind
    }

    /// The job's subject, as stored.
    #[must_use]
    pub fn subject(&self) -> &str {
        &self.subject
    }

    /// When the job is due.
    #[must_use]
    pub fn due_at(&self) -> DateTime<Utc> {
        self.due_at
    }

    /// When the job may next be attempted.
    #[must_use]
    pub fn next_attempt_at(&self) -> DateTime<Utc> {
        self.next_attempt_at
    }

    /// How many attempts to fire it failed.
    #[must_use]
    pub fn attempts(&self) -> i64 {
        self.attempts
    }

    /// The last failure, by its code.
    #[must_use]
    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    /// When it fired, by the database's clock; `None` while it is pending.
    #[must_use]
    pub fn fired_at(&self) -> Option<DateTime<Utc>> {
        self.fired_at
    }
}

impl OutboxRow<SqlDb> for ScheduledJob {
    type Scope = JobScope;

    fn msg_id(&self) -> Cow<'_, str> {
        Cow::Owned(self.job_id.to_string())
    }

    fn attempts(&self) -> i64 {
        self.attempts
    }

    fn pending(scope: &Self::Scope) -> QueryBuilder<Self, SqlDb, HasLeadingKey> {
        let kinds = scope
            .kinds()
            .iter()
            .map(|kind| FilterValue::from(*kind))
            .collect();
        let pending = QueryBuilder::new()
            .filter(Filter::is_null(Self::FIRED_AT))
            .filter(Filter::in_list(Self::KIND, kinds));
        #[cfg(feature = "test-support")]
        let pending = match scope.confinement() {
            Some(confinement) => pending.filter(confinement.subject_filter(Self::SUBJECT)),
            None => pending,
        };
        match scope.bucket() {
            Some(bucket) => pending.filter(Filter::eq(Self::BUCKET, i32::from(bucket.get()))),
            None => pending,
        }
        .already_addressed()
    }

    fn next_attempt_column() -> &'static str {
        Self::NEXT_ATTEMPT_AT
    }

    fn due_column() -> Option<&'static str> {
        Some(Self::DUE_AT)
    }

    fn defer_until(&mut self, at: DateTime<Utc>) {
        self.next_attempt_at = at;
    }

    fn note_failure(&mut self, error: &str) {
        self.attempts += 1;
        self.last_error = Some(error.chars().take(2_000).collect());
    }
}
