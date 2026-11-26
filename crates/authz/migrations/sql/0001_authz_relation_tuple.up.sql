-- One row per relation tuple. The six-part identity is collapsed into `tuple_key`, the
-- injective key the store derives, so a write is idempotent on the primary key; the parts are
-- kept as their own columns so the two read paths below can filter on them. A caveat, if the
-- edge carries one, is its name and its context as JSON.
--
-- `tuple_key` is hashed: every read by it is an exact match on an opaque value whose
-- cardinality is the row count, and ordering it would put every write on one tablet's tail.
CREATE TABLE IF NOT EXISTS authz_relation_tuple (
    tuple_key        TEXT NOT NULL,
    object_ns        TEXT NOT NULL,
    object_id        TEXT NOT NULL,
    relation         TEXT NOT NULL,
    subject_ns       TEXT NOT NULL,
    subject_id       TEXT NOT NULL,
    subject_relation TEXT NOT NULL,
    caveat_name      TEXT,
    caveat_context   TEXT,
    PRIMARY KEY (tuple_key HASH)
);

-- The forward path: an object#relation's edges, walked by every check. The read matches all
-- three exactly, so the whole triple is one hash component and lands on one tablet; hashing the
-- namespace alone would put every object of a namespace in one place.
CREATE INDEX IF NOT EXISTS authz_relation_tuple_forward_idx
    ON authz_relation_tuple ((object_ns, object_id, relation) HASH);

-- The reverse path: what a subject holds, for list-objects and reverse traversal. The subject
-- is matched exactly and hashed; the object side is what the read ranges over, so it is ordered.
CREATE INDEX IF NOT EXISTS authz_relation_tuple_reverse_idx
    ON authz_relation_tuple (
        (subject_ns, subject_id, subject_relation) HASH,
        object_ns ASC,
        relation ASC
    );
