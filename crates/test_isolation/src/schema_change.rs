//! Whether an SQL script changes the schema.

/// The keywords a statement that changes the schema, a role or a grant begins with.
const SCHEMA_CHANGING: [&str; 7] = [
    "CREATE", "ALTER", "DROP", "GRANT", "REVOKE", "TRUNCATE", "COMMENT",
];

/// The first keyword of each statement of `script` that changes the schema, a role or a grant.
pub(crate) fn schema_changes(script: &str) -> Vec<String> {
    let statements: String = script
        .lines()
        .map(|line| line.split("--").next().unwrap_or_default())
        .filter(|line| !line.trim_start().starts_with('\\'))
        .collect::<Vec<_>>()
        .join("\n");
    statements
        .split(';')
        .filter_map(|statement| statement.split_whitespace().next())
        .map(str::to_ascii_uppercase)
        .filter(|keyword| SCHEMA_CHANGING.contains(&keyword.as_str()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::schema_changes;

    #[test]
    fn each_schema_change_is_named_by_its_first_keyword() {
        let script = "-- DROP nothing here\n\\pset format unaligned\nSELECT 1;\ncreate role x;\n  Grant select on t to x;\nDELETE FROM t WHERE id = :'id';\n";
        assert_eq!(schema_changes(script), ["CREATE", "GRANT"]);
    }

    #[test]
    fn rows_written_and_read_change_nothing() {
        assert!(schema_changes("UPDATE t SET a = 1 WHERE b = 2; SELECT 1;").is_empty());
    }
}
