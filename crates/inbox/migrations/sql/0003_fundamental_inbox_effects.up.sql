-- One row per effect: which handler applies it, and how far back its ledger is complete.
--
-- `handler` is claimed by the first listener and checked by every later one, across processes
-- and services, so two different handlers cannot share one ledger by sharing a name.
--
-- `covered_from` is the watermark: an event older than it may have lost its row to the sweep,
-- so it is never applied. It only rises, and the sweep deletes only below `delete_below` — the
-- previous watermark, promoted once every replica has had a grace period to learn it. A replica
-- therefore never applies an event whose row the sweep may already have removed, whatever
-- retention each process is configured with. NULL means nothing has ever been swept.
CREATE TABLE IF NOT EXISTS fundamental_inbox_effects (
    effect              TEXT        NOT NULL,
    handler             TEXT        NOT NULL,
    covered_from        TIMESTAMPTZ,
    covered_from_set_at TIMESTAMPTZ,
    delete_below        TIMESTAMPTZ,
    registered_at       TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (effect HASH),
    CONSTRAINT fundamental_inbox_effects_effect_length CHECK (char_length(effect) BETWEEN 1 AND 100)
);
