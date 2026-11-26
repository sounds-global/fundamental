//! Warns about unknown environment override keys, suggesting close matches.

use tracing::warn;

fn levenshtein(a: &str, b: &str) -> usize {
    let a_len = a.len();
    let b_len = b.len();
    if a_len == 0 {
        return b_len;
    }
    if b_len == 0 {
        return a_len;
    }
    let mut prev: Vec<usize> = (0..=b_len).collect();
    let mut curr = vec![0; b_len + 1];
    for (i, ca) in a.chars().enumerate() {
        curr[0] = i + 1;
        for (j, cb) in b.chars().enumerate() {
            let cost = if ca == cb { 0 } else { 1 };
            curr[j + 1] = (prev[j + 1] + 1).min(curr[j] + 1).min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[b_len]
}

/// Warns when environment override keys don't match any base config key.
#[expect(
    clippy::cognitive_complexity,
    clippy::excessive_nesting,
    reason = "readability baseline"
)]
pub(crate) fn validate_override_keys(base: &toml::Value, overrides: &toml::Value, path: &str) {
    let (Some(base_table), Some(override_table)) = (base.as_table(), overrides.as_table()) else {
        return;
    };

    for key in override_table.keys() {
        let full_path = if path.is_empty() {
            key.clone()
        } else {
            format!("{path}.{key}")
        };

        if !base_table.contains_key(key) {
            let suggestion = base_table
                .keys()
                .min_by_key(|k| levenshtein(k, key))
                .filter(|k| levenshtein(k, key) <= 2);

            if let Some(similar) = suggestion {
                let suggested_path = if path.is_empty() {
                    similar.clone()
                } else {
                    format!("{path}.{similar}")
                };
                warn!(
                    key = %full_path,
                    suggestion = %suggested_path,
                    "unknown config key in environment override; did you mean the suggestion?"
                );
            } else {
                warn!(
                    key = %full_path,
                    "unknown config key in environment override (no matching base key found)"
                );
            }
        } else if let (Some(base_val), Some(override_val)) =
            (base_table.get(key), override_table.get(key))
        {
            // Recurse into nested tables.
            if base_val.is_table() && override_val.is_table() {
                validate_override_keys(base_val, override_val, &full_path);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_levenshtein() {
        assert_eq!(levenshtein("", ""), 0);
        assert_eq!(levenshtein("abc", "abc"), 0);
        assert_eq!(levenshtein("abc", "ab"), 1);
        assert_eq!(levenshtein("database", "databse"), 1);
        assert_eq!(levenshtein("database", "databace"), 1);
        assert_eq!(levenshtein("server", "servr"), 1);
        assert_eq!(levenshtein("abc", "xyz"), 3);
    }

    #[test]
    fn test_validate_override_keys_exact_match() {
        let base = toml::from_str::<toml::Value>(
            r#"
            [apps.main]
            web = { host = "127.0.0.1", port = 3000 }
            [apps.main.database]
            [apps.main.database.sql]
            url = "postgresql://localhost"
            "#,
        )
        .unwrap();

        let overrides = toml::from_str::<toml::Value>(
            r#"
            [apps.main]
            web = { port = 8080 }
            "#,
        )
        .unwrap();

        validate_override_keys(&base, &overrides, "");
    }

    #[test]
    fn test_validate_override_keys_typo() {
        let base = toml::from_str::<toml::Value>(
            r#"
            [apps.main]
            web = { host = "127.0.0.1" }
            [apps.main.database]
            [apps.main.database.sql]
            url = "postgresql://localhost"
            "#,
        )
        .unwrap();

        let overrides = toml::from_str::<toml::Value>(
            r#"
            [apps.main.databse]
            [apps.main.databse.sql]
            url = "postgresql://production"
            "#,
        )
        .unwrap();

        validate_override_keys(&base, &overrides, "");
    }

    #[test]
    fn test_validate_override_keys_nested_typo() {
        let base = toml::from_str::<toml::Value>(
            r#"
            [apps.main.database]
            [apps.main.database.sql]
            url = "postgresql://localhost"
            max_connections = 10
            "#,
        )
        .unwrap();

        let overrides = toml::from_str::<toml::Value>(
            r#"
            [apps.main.database]
            [apps.main.database.sql]
            max_conections = 50
            "#,
        )
        .unwrap();

        validate_override_keys(&base, &overrides, "");
    }
}
