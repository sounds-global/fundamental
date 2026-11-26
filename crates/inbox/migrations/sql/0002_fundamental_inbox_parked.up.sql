-- Every delivery the inbox refused to apply, kept for an operator: why, when, and where the
-- message sits in its stream. No payload is kept — the stream holds it.
--
-- A stream's sequences restart when it is recreated, so its creation time is part of the key.
-- A delivery parked twice (a consumer reset) finds its record and is terminated again.
CREATE TABLE IF NOT EXISTS fundamental_inbox_parked (
    effect            TEXT        NOT NULL,
    stream            TEXT        NOT NULL,
    stream_created_at TIMESTAMPTZ NOT NULL,
    stream_sequence   BIGINT      NOT NULL,
    event_id          UUID,
    reason            TEXT        NOT NULL,
    error_code        TEXT,
    delivered         BIGINT      NOT NULL,
    parked_at         TIMESTAMPTZ NOT NULL,
    PRIMARY KEY ((effect, stream, stream_created_at, stream_sequence) HASH),
    CONSTRAINT fundamental_inbox_parked_reason CHECK (reason IN (
        'undecodable', 'invalid_event_id', 'event_id_reused', 'beyond_horizon',
        'from_the_future', 'attempts_exhausted', 'handler_panicked'
    ))
);

CREATE INDEX IF NOT EXISTS fundamental_inbox_parked_by_age
    ON fundamental_inbox_parked (effect HASH, parked_at ASC);
