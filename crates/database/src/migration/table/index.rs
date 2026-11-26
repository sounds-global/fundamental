/// Index definition for schema.
#[derive(Debug, Clone, PartialEq)]
pub struct TableIndex {
    /// Index name.
    pub name: String,
    /// Columns.
    pub columns: Vec<String>,
    /// Whether unique.
    pub unique: bool,
    /// Partial index condition.
    pub condition: Option<String>,
    /// Its key's parts, each with how it is split.
    pub key: Vec<crate::migration::KeyPart>,
    /// The columns it carries beside its key.
    pub include: Vec<String>,
    /// Whether a table's `CONSTRAINT … UNIQUE` made it, so it goes as that constraint: the engine
    /// refuses to drop an index a constraint needs.
    pub constraint: bool,
}

impl TableIndex {
    /// Whether `other` is this index; a condition is compared as the engine means it, whatever
    /// its parentheses, spacing or keyword case.
    #[must_use]
    pub fn same_as(&self, other: &Self) -> bool {
        self.key == other.key
            && self.unique == other.unique
            && self.include == other.include
            && self.constraint == other.constraint
            && self.condition.as_deref().map(condition_words)
                == other.condition.as_deref().map(condition_words)
    }
}

/// A condition's words, normalised to the form the engine reads it back in.
pub(crate) fn condition_words(condition: &str) -> String {
    let mut uncast = String::with_capacity(condition.len());
    let mut rest = condition;
    while let Some(at) = rest.find("::") {
        uncast.push_str(&rest[..at]);
        rest = rest[at + 2..].trim_start_matches(|c: char| c.is_ascii_alphanumeric() || c == '_');
        rest = rest.strip_prefix("[]").unwrap_or(rest);
    }
    uncast.push_str(rest);
    let words = uncast
        .replace(['(', ')'], " ")
        .split_whitespace()
        .map(str::to_ascii_lowercase)
        .collect::<Vec<_>>()
        .join(" ");
    words
        .replace("<> all array[", "not in ")
        .replace("= any array[", "in ")
        .replace(']', " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::TableIndex;
    use crate::migration::KeyPart;
    use fundamental_core::model::sql::ShardKind;

    fn index(condition: Option<&str>) -> TableIndex {
        TableIndex {
            name: "claim".to_owned(),
            columns: vec!["bucket".to_owned(), "next_attempt_at".to_owned()],
            unique: false,
            condition: condition.map(str::to_owned),
            key: KeyPart::parts(&[("bucket", Some(ShardKind::Hash)), ("next_attempt_at", None)]),
            include: Vec::new(),
            constraint: false,
        }
    }

    #[test]
    fn a_condition_is_the_same_however_the_engine_renders_it() {
        assert!(
            index(Some("closed_at IS NULL AND outbox_skip_until IS NULL")).same_as(&index(Some(
                "((closed_at IS NULL) AND (outbox_skip_until IS NULL))"
            )))
        );
        assert!(!index(Some("closed_at IS NULL")).same_as(&index(Some("closed_at IS NOT NULL"))));
        assert!(!index(None).same_as(&index(Some("closed_at IS NULL"))));
        assert!(
            index(Some("state = 'clean'")).same_as(&index(Some("(state = 'clean'::text)"))),
            "a cast the engine adds"
        );
        assert!(
            index(Some("state IN ('open', 'pending')")).same_as(&index(Some(
                "(state = ANY (ARRAY['open'::text, 'pending'::text]))"
            ))),
            "a list the engine writes as an array"
        );
        assert!(
            index(Some("state NOT IN ('done')"))
                .same_as(&index(Some("(state <> ALL (ARRAY['done'::text]))"))),
            "a list left out, written as an array"
        );
        assert!(!index(Some("state IN ('open')")).same_as(&index(Some("state NOT IN ('open')"))));
    }

    #[test]
    fn a_key_split_otherwise_is_another_index() {
        let mut descending = index(None);
        descending.key = KeyPart::parts(&[
            ("bucket", Some(ShardKind::Hash)),
            ("next_attempt_at", Some(ShardKind::Desc)),
        ]);
        assert!(!index(None).same_as(&descending));
    }
}
