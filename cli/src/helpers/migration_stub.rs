//! Stub contents for a generated migration pair.

/// The body of a generated `*.up.*` migration: a commented placeholder whose key is shardable.
pub(crate) fn up_stub() -> &'static str {
    "-- Up migration.\n\
     -- Write your forward schema change here. Every key element declares how it shards;\n\
     -- the engine chooses one otherwise, and the migration will not say which.\n\
     --\n\
     --   CREATE TABLE example (\n\
     --       id UUID NOT NULL,\n\
     --       PRIMARY KEY (id HASH)      -- HASH for a point lookup, ASC/DESC to scan\n\
     --   );\n"
}

pub(crate) fn down_stub() -> &'static str {
    "-- Down migration.\n-- Reverse the change made by the matching up migration, e.g.:\n-- DROP TABLE example;\n"
}
