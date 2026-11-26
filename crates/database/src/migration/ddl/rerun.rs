//! [`why_not_rerunnable`]: whether a migration statement can be run a second time.

use super::split_top_level;

/// Why `statement` cannot be run again over its own effect, or `None` when it can.
///
/// Asked before a failed migration is retried, and judged by the statement's shape alone: an
/// `IF NOT EXISTS` or `IF EXISTS` form, `CREATE OR REPLACE`, `GRANT`, `REVOKE`, `SET`, a
/// `SELECT`, an `INSERT … ON CONFLICT` and an `ALTER TABLE` whose actions all are, run again;
/// anything else does not.
#[must_use]
pub fn why_not_rerunnable(statement: &str) -> Option<&'static str> {
    let trimmed = statement.trim().trim_end_matches(';').trim();
    let spaced = trimmed.split_whitespace().collect::<Vec<_>>().join(" ");
    let upper = spaced.to_ascii_uppercase();
    let words: Vec<&str> = upper.split(' ').collect();

    match words.as_slice() {
        ["ALTER", "TABLE", ..] => alter_table(&spaced),
        ["CREATE", "OR", "REPLACE", ..] => None,
        ["CREATE", ..] => (!has_words(&words, &["IF", "NOT", "EXISTS"]))
            .then_some("a CREATE without IF NOT EXISTS"),
        ["DROP", ..] => {
            (!has_words(&words, &["IF", "EXISTS"])).then_some("a DROP without IF EXISTS")
        }
        ["INSERT", ..] => {
            (!has_words(&words, &["ON", "CONFLICT"])).then_some("an INSERT without ON CONFLICT")
        }
        ["UPDATE", ..] | ["DELETE", ..] | ["TRUNCATE", ..] | ["COPY", ..] => {
            Some("a data statement whose second run is not its first")
        }
        ["GRANT", ..] | ["REVOKE", ..] | ["COMMENT", "ON", ..] | ["SET", ..] | ["SELECT", ..] => {
            None
        }
        _ => Some("a statement whose second run cannot be judged"),
    }
}

fn has_words(words: &[&str], phrase: &[&str]) -> bool {
    words.windows(phrase.len()).any(|window| window == phrase)
}

fn alter_table(spaced: &str) -> Option<&'static str> {
    let mut rest = spaced["ALTER TABLE".len()..].trim_start();
    for keyword in ["IF EXISTS ", "ONLY "] {
        if rest.to_ascii_uppercase().starts_with(keyword) {
            rest = rest[keyword.len()..].trim_start();
        }
    }
    let actions = rest.split_once(' ').map_or("", |(_, actions)| actions);
    let actions = split_top_level(actions);
    if actions.is_empty() {
        return Some("an ALTER TABLE whose actions cannot be read");
    }
    actions.iter().find_map(|action| alter_action(action))
}

fn alter_action(action: &str) -> Option<&'static str> {
    let upper = action.to_ascii_uppercase();
    let words: Vec<&str> = upper.split_whitespace().collect();
    match words.as_slice() {
        ["ADD", "COLUMN", "IF", "NOT", "EXISTS", ..]
        | ["DROP", "COLUMN", "IF", "EXISTS", ..]
        | ["DROP", "CONSTRAINT", "IF", "EXISTS", ..]
        | ["DROP", "IF", "EXISTS", ..] => None,
        ["ALTER", "COLUMN", _, verb, ..] | ["ALTER", _, verb, ..]
            if matches!(*verb, "SET" | "DROP" | "TYPE") =>
        {
            None
        }
        ["RENAME", ..] => Some("a RENAME, which the second run finds already done"),
        ["ADD", "CONSTRAINT", ..] => Some("an ADD CONSTRAINT, which has no IF NOT EXISTS"),
        ["ADD", ..] => Some("an ADD without IF NOT EXISTS"),
        ["DROP", ..] => Some("a DROP without IF EXISTS"),
        _ => Some("an ALTER TABLE action whose second run cannot be judged"),
    }
}

#[cfg(test)]
mod tests {
    use super::why_not_rerunnable;

    #[test]
    fn what_the_generator_writes_runs_again() {
        for statement in [
            "CREATE TABLE IF NOT EXISTS orders (id UUID NOT NULL, PRIMARY KEY (id HASH));",
            "CREATE INDEX IF NOT EXISTS orders_by_customer ON orders (customer_id HASH);",
            "CREATE UNIQUE INDEX IF NOT EXISTS orders_number ON orders (number ASC);",
            "ALTER TABLE orders ADD COLUMN IF NOT EXISTS paid_at TIMESTAMPTZ;",
            "ALTER TABLE orders DROP COLUMN IF EXISTS legacy, DROP CONSTRAINT IF EXISTS orders_kind;",
            "ALTER TABLE orders ALTER COLUMN note SET DEFAULT '', ALTER COLUMN note SET NOT NULL;",
            "DROP INDEX IF EXISTS orders_by_note;",
            "DROP TABLE IF EXISTS drafts;",
            "INSERT INTO kinds (name) VALUES ('news') ON CONFLICT DO NOTHING;",
            "CREATE OR REPLACE FUNCTION touch() RETURNS trigger AS $$ BEGIN RETURN NEW; END $$ LANGUAGE plpgsql;",
            "GRANT SELECT ON orders TO reader;",
            "COMMENT ON TABLE orders IS 'what was bought';",
            "SELECT 1;",
        ] {
            assert_eq!(why_not_rerunnable(statement), None, "{statement}");
        }
    }

    #[test]
    fn a_statement_whose_second_run_differs_says_why() {
        for (statement, why) in [
            (
                "ALTER TABLE orders RENAME COLUMN note TO memo;",
                "a RENAME, which the second run finds already done",
            ),
            (
                "ALTER TABLE orders ADD CONSTRAINT orders_total CHECK (total >= 0);",
                "an ADD CONSTRAINT, which has no IF NOT EXISTS",
            ),
            (
                "ALTER TABLE orders ADD COLUMN paid_at TIMESTAMPTZ;",
                "an ADD without IF NOT EXISTS",
            ),
            (
                "ALTER TABLE orders ADD COLUMN IF NOT EXISTS a INT, RENAME COLUMN b TO c;",
                "a RENAME, which the second run finds already done",
            ),
            (
                "CREATE TABLE orders (id UUID PRIMARY KEY);",
                "a CREATE without IF NOT EXISTS",
            ),
            ("DROP TABLE drafts;", "a DROP without IF EXISTS"),
            (
                "INSERT INTO kinds (name) VALUES ('news');",
                "an INSERT without ON CONFLICT",
            ),
            (
                "UPDATE orders SET total = total + 1;",
                "a data statement whose second run is not its first",
            ),
            (
                "DO $$ BEGIN CREATE ROLE reader; END $$;",
                "a statement whose second run cannot be judged",
            ),
        ] {
            assert_eq!(why_not_rerunnable(statement), Some(why), "{statement}");
        }
    }

    #[test]
    fn a_statement_is_judged_however_its_lines_are_broken() {
        assert_eq!(
            why_not_rerunnable(
                "create index\n  if not exists by_customer\n  on orders (customer_id hash)"
            ),
            None
        );
    }
}
