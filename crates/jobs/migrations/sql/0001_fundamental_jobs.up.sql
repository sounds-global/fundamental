-- Every durable job: one row per (kind, subject), due at a time, fired once.
--
-- YSQL, because the row is written inside the caller's transaction — a deadline commits or rolls
-- back with the state that caused it — and fired inside the transaction that writes its effect,
-- and every transition is a conditional update. Neither holds on the Cassandra wire.
--
-- Keyed by `job_id`, a digest of (kind, subject), hashed, so writes spread over every tablet and
-- two schedulers of one job converge on one row. Every index leads with `bucket`, a digest of the
-- subject in 0..64, as a range column: 64 independent heads instead of one hot "now" point, and
-- every job of one subject in one bucket, so a subject's jobs are claimed in due order.
--
-- A fired row stays as a tombstone until the purge, so scheduling an occurrence that already fired
-- reports it rather than firing it again; `fired_at >= due_at` makes a fire recorded before its due
-- time a constraint violation rather than a bug to find.
CREATE TABLE IF NOT EXISTS fundamental_jobs (
    job_id                  UUID        NOT NULL,
    bucket                  SMALLINT    NOT NULL,
    kind                    TEXT        NOT NULL,
    subject                 TEXT        NOT NULL,
    due_at                  TIMESTAMPTZ NOT NULL,
    next_attempt_at         TIMESTAMPTZ NOT NULL,
    attempts                BIGINT      NOT NULL,
    last_error              TEXT,
    fired_at                TIMESTAMPTZ,
    outbox_seal             BYTEA       NOT NULL,
    outbox_set_aside_at     TIMESTAMPTZ,
    outbox_set_aside_reason TEXT,
    outbox_set_aside_seal   BYTEA,
    outbox_skip_until       TIMESTAMPTZ,
    PRIMARY KEY (job_id HASH),
    CONSTRAINT fundamental_jobs_bucket CHECK (bucket BETWEEN 0 AND 63),
    CONSTRAINT fundamental_jobs_kind CHECK (kind ~ '^[a-z][a-z0-9._-]{0,63}$'),
    CONSTRAINT fundamental_jobs_subject CHECK (char_length(subject) BETWEEN 1 AND 256 AND subject ~ '^[A-Za-z0-9._:/-]+$'),
    CONSTRAINT fundamental_jobs_last_error CHECK (last_error IS NULL OR char_length(last_error) <= 2000),
    CONSTRAINT fundamental_jobs_fired_not_before_due CHECK (fired_at IS NULL OR fired_at >= due_at)
);

-- The claim: a bucket's pending jobs by next attempt, which for a job never attempted is its due.
CREATE INDEX IF NOT EXISTS fundamental_jobs_claim
    ON fundamental_jobs (bucket ASC, next_attempt_at ASC)
    WHERE fired_at IS NULL AND outbox_set_aside_at IS NULL AND outbox_skip_until IS NULL;

-- The claim's second pass: jobs a replica without the key left, by when it may be tried again.
CREATE INDEX IF NOT EXISTS fundamental_jobs_skipped
    ON fundamental_jobs (bucket ASC, outbox_skip_until ASC)
    WHERE fired_at IS NULL AND outbox_set_aside_at IS NULL AND outbox_skip_until IS NOT NULL;

-- The inventory: pending jobs by due time, so "already due and still pushed out" is a bounded read.
CREATE INDEX IF NOT EXISTS fundamental_jobs_overdue
    ON fundamental_jobs (bucket ASC, due_at ASC)
    WHERE fired_at IS NULL;

-- The purge: tombstones by when they fired.
CREATE INDEX IF NOT EXISTS fundamental_jobs_fired
    ON fundamental_jobs (bucket ASC, fired_at ASC)
    WHERE fired_at IS NOT NULL;

-- The inventory's other reads: jobs a claim set aside, by when.
CREATE INDEX IF NOT EXISTS fundamental_jobs_set_aside
    ON fundamental_jobs (bucket ASC, outbox_set_aside_at ASC)
    WHERE outbox_set_aside_at IS NOT NULL;
