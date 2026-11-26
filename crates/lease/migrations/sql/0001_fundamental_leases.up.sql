-- One row per singleton, keyed by its name: who holds it, at which fencing epoch, and until
-- when by the database's own clock. A vacant row keeps its epoch, so the next holder's is
-- higher. Every timestamp is written from the database's hybrid clock, never a process clock,
-- so there is no DEFAULT now().
CREATE TABLE IF NOT EXISTS fundamental_leases (
    name        TEXT        NOT NULL,
    epoch       BIGINT      NOT NULL,
    holder      UUID,
    acquired_at TIMESTAMPTZ,
    expires_at  TIMESTAMPTZ,
    changed_at  TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (name HASH),
    CONSTRAINT fundamental_leases_epoch_positive CHECK (epoch >= 1),
    CONSTRAINT fundamental_leases_name_shape CHECK (name ~ '^[a-z][a-z0-9-]{0,62}$'),
    CONSTRAINT fundamental_leases_held_is_whole CHECK (
        (holder IS NULL AND acquired_at IS NULL AND expires_at IS NULL)
        OR (holder IS NOT NULL AND acquired_at IS NOT NULL AND expires_at IS NOT NULL
            AND acquired_at <= changed_at AND changed_at < expires_at)
    )
);
