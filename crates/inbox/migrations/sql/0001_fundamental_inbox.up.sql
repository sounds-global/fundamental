-- Every event an effect has applied, by the effect's declared name and the event's own id.
-- The row is written in the same transaction as the effect and before it, so a row present
-- means the effect committed, and a second delivery conflicts on the key and commits nothing.
--
-- Keyed by the hash of (effect, event_id): one busy effect's rows spread across tablets. The
-- age index is bucketed by the event id's random bits, so one effect's index writes spread
-- over sixty-four ranges rather than all landing on the newest end of one. A row is swept
-- once older than the retention window; an event older than the window less a margin is
-- never applied, so no copy of it can arrive after its row is gone.
CREATE TABLE IF NOT EXISTS fundamental_inbox (
    effect          TEXT        NOT NULL,
    event_id        UUID        NOT NULL,
    sweep_bucket    INT         NOT NULL,
    payload_digest  BYTEA       NOT NULL,
    stream          TEXT        NOT NULL,
    consumer        TEXT        NOT NULL,
    stream_sequence BIGINT      NOT NULL,
    processed_at    TIMESTAMPTZ NOT NULL,
    PRIMARY KEY ((effect, event_id) HASH),
    CONSTRAINT fundamental_inbox_effect_length CHECK (char_length(effect) BETWEEN 1 AND 100),
    CONSTRAINT fundamental_inbox_bucket_range CHECK (sweep_bucket BETWEEN 0 AND 63),
    CONSTRAINT fundamental_inbox_digest_length CHECK (octet_length(payload_digest) = 32)
);

CREATE INDEX IF NOT EXISTS fundamental_inbox_by_age
    ON fundamental_inbox ((effect, sweep_bucket) HASH, processed_at ASC);
