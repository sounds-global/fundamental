//! The [`add_workspace_member`] helper: append a member to the workspace `Cargo.toml`.

use std::fs;
use std::path::Path;

use toml_edit::{Array, DocumentMut, Item, Value};
use tracing::{debug, instrument};

use crate::CliError;

/// Add `member` to `[workspace] members`, keeping the layout; a no-op when present or glob-covered.
///
/// # Errors
/// [`CliError::Io`] if the file cannot be read or written, [`CliError::TomlParse`] if it is not TOML.
#[instrument(skip_all, fields(path = %path.display(), member), err(Debug))]
#[expect(clippy::cognitive_complexity, reason = "readability baseline")]
pub(crate) fn add_workspace_member(path: &Path, member: &str) -> Result<(), CliError> {
    let text = fs::read_to_string(path).map_err(|e| CliError::Io {
        path: path.display().to_string(),
        reason: e.to_string(),
    })?;
    let mut doc = text
        .parse::<DocumentMut>()
        .map_err(|e| CliError::TomlParse {
            path: path.display().to_string(),
            reason: e.to_string(),
        })?;

    let members = doc
        .entry("workspace")
        .or_insert_with(toml_edit::table)
        .as_table_mut()
        .and_then(|t| {
            t.entry("members")
                .or_insert_with(|| Item::Value(Value::Array(Array::new())))
                .as_array_mut()
        });

    let Some(members) = members else {
        return Err(CliError::TomlParse {
            path: path.display().to_string(),
            reason: "[workspace] members is not an array".to_string(),
        });
    };

    if let Some(entry) = members
        .iter()
        .filter_map(Value::as_str)
        .find(|entry| covers(entry, member))
    {
        debug!(member, entry, "workspace member already covered; no change");
        return Ok(());
    }

    members.push(member);
    debug!(member, "added workspace member");

    fs::write(path, doc.to_string()).map_err(|e| CliError::Io {
        path: path.display().to_string(),
        reason: e.to_string(),
    })
}

/// Whether `pattern` names `member`, segment by segment with cargo's `*` and `?`.
fn covers(pattern: &str, member: &str) -> bool {
    let pattern: Vec<&str> = pattern.trim_end_matches('/').split('/').collect();
    let member: Vec<&str> = member.trim_end_matches('/').split('/').collect();
    pattern.len() == member.len()
        && pattern
            .iter()
            .zip(&member)
            .all(|(glob, segment)| segment_matches(glob.as_bytes(), segment.as_bytes()))
}

fn segment_matches(glob: &[u8], text: &[u8]) -> bool {
    match (glob.first(), text.first()) {
        (None, None) => true,
        (Some(b'*'), _) => {
            segment_matches(&glob[1..], text)
                || (!text.is_empty() && segment_matches(glob, &text[1..]))
        }
        (Some(b'?'), Some(_)) => segment_matches(&glob[1..], &text[1..]),
        (Some(wanted), Some(got)) if wanted == got => segment_matches(&glob[1..], &text[1..]),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write_cargo(body: &str) -> (TempDir, std::path::PathBuf) {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("Cargo.toml");
        fs::write(&path, body).unwrap();
        (tmp, path)
    }

    #[test]
    fn appends_a_new_member() {
        let (_t, path) = write_cargo("[workspace]\nresolver = \"2\"\nmembers = [\"apps/main\"]\n");
        add_workspace_member(&path, "apps/billing").unwrap();
        let out = fs::read_to_string(&path).unwrap();
        assert!(out.contains("apps/main"), "{out}");
        assert!(out.contains("apps/billing"), "{out}");
    }

    #[test]
    fn adding_an_existing_member_is_a_noop() {
        let (_t, path) = write_cargo("[workspace]\nmembers = [\"apps/main\"]\n");
        add_workspace_member(&path, "apps/main").unwrap();
        let out = fs::read_to_string(&path).unwrap();
        assert_eq!(out.matches("apps/main").count(), 1, "{out}");
    }

    #[test]
    fn bad_toml_is_a_parse_error() {
        let (_t, path) = write_cargo("= = =");
        let err = add_workspace_member(&path, "apps/x").expect_err("bad toml");
        assert!(matches!(err, CliError::TomlParse { .. }));
    }

    #[test]
    fn a_member_a_glob_already_covers_is_not_appended() {
        let (_tmp, path) =
            write_cargo("[workspace]\nmembers = [\"domain/*\", \"apps/*\", \"crates/*\"]\n");
        add_workspace_member(&path, "apps/billing").unwrap();
        add_workspace_member(&path, "apps/billing/native").unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(!text.contains("\"apps/billing\""), "{text}");
        assert!(
            text.contains("\"apps/billing/native\""),
            "a glob covers one segment, not the tree below it: {text}"
        );
    }

    #[test]
    fn globs_match_within_a_segment_as_cargo_reads_them() {
        assert!(covers("apps/*", "apps/billing"));
        assert!(covers("apps/*/", "apps/billing"));
        assert!(covers("apps/bill?ng", "apps/billing"));
        assert!(covers("apps/b*g", "apps/billing"));
        assert!(!covers("apps/*", "apps/billing/native"));
        assert!(!covers("apps/*", "crates/billing"));
        assert!(!covers("apps/b*x", "apps/billing"));
    }
}
