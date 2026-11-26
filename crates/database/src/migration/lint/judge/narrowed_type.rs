//! `narrowed_type`: a column retyped to hold less than it held.

use super::Before;
use crate::migration::ddl::DdlStatement;
use crate::migration::lint::{Lint, LintFinding};

/// Fires on `ALTER COLUMN … TYPE` to a type that holds less than the one before.
pub(crate) fn judge(before: &Before<'_>, statement: &DdlStatement) -> Option<LintFinding> {
    let DdlStatement::AlterColumnType { table, column, to } = statement else {
        return None;
    };
    let from = &before.facts.tables.get(table)?.column(column)?.data_type;
    narrows(from, to).then(|| {
        before
            .finding(
                Lint::NarrowedType,
                statement,
                "ALTER COLUMN TYPE",
                format!("{table}.{column} narrows from {from} to {to}"),
            )
            .naming(format!("{table}.{column}"))
    })
}

pub(crate) fn narrows(from: &str, to: &str) -> bool {
    let integer = |name: &str| match name {
        "INT2" | "SMALLINT" => Some(2),
        "INT4" | "INT" | "INTEGER" => Some(4),
        "INT8" | "BIGINT" => Some(8),
        _ => None,
    };
    if let (Some(from), Some(to)) = (integer(from), integer(to)) {
        return to < from;
    }
    let float = |name: &str| match name {
        "FLOAT4" | "REAL" => Some(4),
        "FLOAT8" | "DOUBLE PRECISION" => Some(8),
        _ => None,
    };
    if let (Some(from), Some(to)) = (float(from), float(to)) {
        return to < from;
    }
    let (from_name, from_args) = split(from);
    let (to_name, to_args) = split(to);
    match (from_name, to_name) {
        ("TEXT", "CHAR" | "CHARACTER") => true,
        ("TEXT" | "VARCHAR" | "CHARACTER VARYING", "VARCHAR" | "CHARACTER VARYING") => {
            match (from_args.first(), to_args.first()) {
                (None, Some(_)) => true,
                (Some(before), Some(after)) => after < before,
                _ => false,
            }
        }
        ("NUMERIC" | "DECIMAL", "NUMERIC" | "DECIMAL") => numeric_narrows(&from_args, &to_args),
        ("TIMESTAMPTZ" | "TIMESTAMP", "DATE") => true,
        _ => false,
    }
}

/// Whether `NUMERIC(to)` holds less than `NUMERIC(from)`; no arguments is unbounded.
fn numeric_narrows(from: &[u32], to: &[u32]) -> bool {
    let Some(&to_precision) = to.first() else {
        return false;
    };
    let Some(&from_precision) = from.first() else {
        return true;
    };
    let scale = |args: &[u32]| args.get(1).copied().unwrap_or(0);
    let whole = |precision: u32, args: &[u32]| precision.saturating_sub(scale(args));
    whole(to_precision, to) < whole(from_precision, from) || scale(to) < scale(from)
}

fn split(written: &str) -> (&str, Vec<u32>) {
    let Some((name, rest)) = written.split_once('(') else {
        return (written.trim(), Vec::new());
    };
    let args = rest
        .trim_end_matches(')')
        .split(',')
        .filter_map(|arg| arg.trim().parse().ok())
        .collect();
    (name.trim(), args)
}

#[cfg(test)]
mod tests {
    use super::narrows;
    use crate::migration::lint::Lint;
    use crate::migration::lint::judge::test_support::{ORDERS, fired};

    #[test]
    fn a_type_that_holds_less_narrows() {
        for (from, to) in [
            ("INT8", "INT4"),
            ("BIGINT", "SMALLINT"),
            ("TEXT", "VARCHAR(20)"),
            ("VARCHAR(40)", "VARCHAR(20)"),
            ("NUMERIC(12, 2)", "NUMERIC(10, 2)"),
            ("NUMERIC(10, 4)", "NUMERIC(10, 2)"),
            ("NUMERIC", "NUMERIC(10, 2)"),
            ("NUMERIC(10, 2)", "NUMERIC(10, 4)"),
            ("NUMERIC(10)", "NUMERIC(10, 2)"),
            ("DOUBLE PRECISION", "REAL"),
            ("FLOAT8", "FLOAT4"),
            ("TEXT", "CHAR"),
            ("TIMESTAMPTZ", "DATE"),
        ] {
            assert!(narrows(from, to), "{from} to {to}");
        }
    }

    #[test]
    fn a_type_that_holds_as_much_or_more_does_not() {
        for (from, to) in [
            ("INT4", "INT8"),
            ("VARCHAR(20)", "TEXT"),
            ("VARCHAR(20)", "VARCHAR(40)"),
            ("NUMERIC(10, 2)", "NUMERIC(12, 2)"),
            ("NUMERIC(10, 2)", "NUMERIC(12, 4)"),
            ("NUMERIC(10, 2)", "NUMERIC"),
            ("TEXT", "VARCHAR"),
            ("REAL", "DOUBLE PRECISION"),
            ("DATE", "TIMESTAMPTZ"),
            ("UUID", "TEXT"),
        ] {
            assert!(!narrows(from, to), "{from} to {to}");
        }
    }

    #[test]
    fn a_column_narrowed_in_a_later_file_fires_and_widened_does_not() {
        let narrowed = fired(
            Lint::NarrowedType,
            &[
                ORDERS,
                (
                    "0002_total",
                    "ALTER TABLE orders ALTER COLUMN total TYPE INT4;",
                ),
            ],
        );
        let widened = fired(
            Lint::NarrowedType,
            &[
                ORDERS,
                (
                    "0002_total",
                    "ALTER TABLE orders ALTER COLUMN total TYPE NUMERIC;",
                ),
            ],
        );

        assert_eq!(narrowed.len(), 1);
        assert!(narrowed[0].at.contains("narrows from INT8 to INT4"));
        assert!(widened.is_empty());
    }
}
