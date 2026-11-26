//! Reading a per-app `.dev.vars.<app>` **without** touching the environment.

use std::collections::BTreeMap;
use std::path::Path;

use tracing::{debug, warn};

/// Read `{root}/.dev.vars.{app}` as key/value pairs, or an empty map if it is absent.
///
/// Returned rather than loaded, so the dev supervisor applies it to that app's child process only.
#[must_use]
#[expect(clippy::cognitive_complexity, reason = "readability baseline")]
pub fn dev_vars_for_app(root: &Path, app: &str) -> BTreeMap<String, String> {
    let mut values = BTreeMap::new();
    if app.is_empty() {
        return values;
    }
    let path = root.join(format!(".dev.vars.{app}"));
    let contents = match std::fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return values,
        Err(error) => {
            warn!(%error, path = %path.display(), "could not read the per-app dev vars; skipping");
            return values;
        }
    };
    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            warn!(path = %path.display(), line, "dev-vars line has no '='; skipping");
            continue;
        };
        let key = key.trim();
        if key.is_empty() {
            continue;
        }
        values.insert(key.to_string(), unquote(value.trim()).to_string());
    }
    debug!(path = %path.display(), count = values.len(), "read per-app dev vars");
    values
}

fn unquote(value: &str) -> &str {
    for quote in ['"', '\''] {
        if let Some(inner) = value.strip_prefix(quote)
            && let Some(inner) = inner.strip_suffix(quote)
        {
            return inner;
        }
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write(dir: &Path, app: &str, body: &str) {
        let mut file = std::fs::File::create(dir.join(format!(".dev.vars.{app}"))).expect("create");
        file.write_all(body.as_bytes()).expect("write");
    }

    #[test]
    fn an_absent_file_is_an_empty_map() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert!(dev_vars_for_app(dir.path(), "layer2").is_empty());
    }

    #[test]
    fn comments_and_blank_lines_are_skipped() {
        let dir = tempfile::tempdir().expect("tempdir");
        write(
            dir.path(),
            "layer2",
            "# a comment\n\nA=1\n  # indented\nB=2\n",
        );
        let values = dev_vars_for_app(dir.path(), "layer2");
        assert_eq!(values.get("A").map(String::as_str), Some("1"));
        assert_eq!(values.get("B").map(String::as_str), Some("2"));
        assert_eq!(values.len(), 2);
    }

    #[test]
    fn quotes_are_stripped_and_a_value_may_contain_equals() {
        let dir = tempfile::tempdir().expect("tempdir");
        write(dir.path(), "layer2", "A=\"x\"\nB='y'\nC=k=v\n");
        let values = dev_vars_for_app(dir.path(), "layer2");
        assert_eq!(values.get("A").map(String::as_str), Some("x"));
        assert_eq!(values.get("B").map(String::as_str), Some("y"));
        assert_eq!(values.get("C").map(String::as_str), Some("k=v"));
    }

    #[test]
    fn reading_one_app_does_not_affect_the_environment_or_another_app() {
        let dir = tempfile::tempdir().expect("tempdir");
        write(dir.path(), "e2e_app", "DEV_VARS_APP_SCOPE_TEST=e2e_app\n");
        write(dir.path(), "layer2", "DEV_VARS_APP_SCOPE_TEST=layer2\n");

        let first = dev_vars_for_app(dir.path(), "e2e_app");
        let second = dev_vars_for_app(dir.path(), "layer2");

        assert_eq!(
            first.get("DEV_VARS_APP_SCOPE_TEST").map(String::as_str),
            Some("e2e_app")
        );
        assert_eq!(
            second.get("DEV_VARS_APP_SCOPE_TEST").map(String::as_str),
            Some("layer2"),
            "the first read won for the second app; that is the process-global bug \
             this function exists to avoid"
        );
        assert!(
            std::env::var("DEV_VARS_APP_SCOPE_TEST").is_err(),
            "reading a per-app file must not write to the process environment"
        );
    }
}
