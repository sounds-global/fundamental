-- Every piece of fenced work a lease has committed, by its work key. The row is written in the
-- same transaction as the work, so a key present means the work committed: a transaction re-run
-- after a lost commit acknowledgement finds it and does nothing.
--
-- Keyed by the hash of (lease, work_key), not by the lease alone, so one busy singleton's
-- ledger spreads across tablets instead of piling into the one its name hashes to. Rows older
-- than the lease's retention window are pruned through the age index by the holder; a key
-- presented again after that window runs again, which is the replay window's contract.
CREATE TABLE IF NOT EXISTS fundamental_lease_work (
    lease    TEXT        NOT NULL,
    work_key TEXT        NOT NULL,
    epoch    BIGINT      NOT NULL,
    done_at  TIMESTAMPTZ NOT NULL,
    PRIMARY KEY ((lease, work_key) HASH),
    CONSTRAINT fundamental_lease_work_key_length CHECK (char_length(work_key) BETWEEN 1 AND 200)
);

CREATE INDEX IF NOT EXISTS fundamental_lease_work_by_age
    ON fundamental_lease_work (lease ASC, done_at ASC);
