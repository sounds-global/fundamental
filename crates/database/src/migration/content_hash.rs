//! The content hash that makes an applied migration's text unforgeable.

/// A migration's content hash: lowercase hex of BLAKE3 over its statements.
///
/// Recorded so an applied migration edited in place is a named error. Comments are not hashed;
/// any other edit, reformatting included, is a change.
#[must_use]
pub fn content_hash(sql: &str) -> String {
    let mut hasher = blake3::Hasher::new();
    for statement in super::split_statements(sql) {
        hasher.update(statement.as_bytes());
        // Injective: the separator cannot occur inside a statement.
        hasher.update(b"\0");
    }
    hasher.finalize().to_hex().to_string()
}

#[cfg(test)]
mod tests {
    use super::content_hash;

    #[test]
    fn the_same_statements_hash_the_same() {
        assert_eq!(
            content_hash("CREATE TABLE t (id UUID PRIMARY KEY);"),
            content_hash("CREATE TABLE t (id UUID PRIMARY KEY);")
        );
    }

    #[test]
    fn changing_a_key_changes_the_hash() {
        assert_ne!(
            content_hash("CREATE TABLE t (id STRING, PRIMARY KEY (id));"),
            content_hash("CREATE TABLE t (zone STRING, id STRING, PRIMARY KEY (zone, id));")
        );
    }

    #[test]
    fn a_reworded_comment_is_not_a_changed_migration() {
        assert_eq!(
            content_hash("-- why this table exists\nCREATE TABLE t (id UUID);"),
            content_hash("-- a better sentence\n\nCREATE TABLE t (id UUID);  ")
        );
    }

    #[test]
    fn reindenting_an_applied_statement_is_a_change() {
        assert_ne!(
            content_hash("CREATE TABLE t (id UUID NOT NULL, PRIMARY KEY (id));"),
            content_hash("CREATE TABLE t (\n    id UUID NOT NULL,\n    PRIMARY KEY (id)\n);\n")
        );
    }

    #[test]
    fn two_statements_do_not_hash_as_their_concatenation() {
        assert_ne!(
            content_hash("CREATE TABLE a (id UUID); CREATE TABLE b (id UUID);"),
            content_hash("CREATE TABLE a (id UUID); CREATE TABLE b (id UUID); DROP TABLE b;")
        );
    }

    #[test]
    fn an_empty_migration_still_has_a_hash() {
        assert_ne!(content_hash(""), content_hash("SELECT 1;"));
    }
}
