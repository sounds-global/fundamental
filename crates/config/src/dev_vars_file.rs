//! Writing development secrets into `.dev.vars` ([`DevVarsFile`]).

use std::path::{Path, PathBuf};

use crate::{ConfigError, ConfigResult};

const DEV_VARS_FILE: &str = ".dev.vars";

/// Upserts secrets into the git-ignored, owner-only `.dev.vars` file the loader reads in development.
pub struct DevVarsFile {
    path: PathBuf,
}

impl DevVarsFile {
    /// Targets `.dev.vars` in the current directory.
    #[must_use]
    pub fn current() -> Self {
        Self {
            path: PathBuf::from(DEV_VARS_FILE),
        }
    }

    /// Targets a specific dotenv file.
    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// The file this writes to.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Sets `key` to `value`, replacing any existing entry and preserving the rest of the file.
    ///
    /// # Errors
    /// [`ConfigError::SecretFileUnreadable`] if the file cannot be read or written.
    pub fn set(&self, key: &str, value: &str) -> ConfigResult<()> {
        let existing = match std::fs::read_to_string(&self.path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(error) => return Err(self.unreadable(error)),
        };

        let lines: Vec<&str> = existing.lines().collect();
        let entry = format!("{key}=\"{}\"", quote(value));
        let block: Vec<&str> = entry.lines().collect();

        // A quoted value may span lines, so the whole entry is removed, not just its first line.
        let mut out: Vec<&str> = Vec::with_capacity(lines.len() + block.len());
        match entry_span(&lines, key) {
            Some((start, end)) => {
                out.extend_from_slice(&lines[..start]);
                out.extend_from_slice(&block);
                out.extend_from_slice(&lines[end..]);
            }
            None => {
                out.extend_from_slice(&lines);
                out.extend_from_slice(&block);
            }
        }

        let mut contents = out.join("\n");
        contents.push('\n');
        self.write_owner_only(&contents)
    }

    /// Sets `key` only if it has no entry yet, reporting whether it wrote.
    ///
    /// # Errors
    /// [`ConfigError::SecretFileUnreadable`] if the file cannot be read or written.
    pub fn set_if_absent(&self, key: &str, value: &str) -> ConfigResult<bool> {
        if self.contains(key)? {
            return Ok(false);
        }
        self.set(key, value)?;
        Ok(true)
    }

    /// Whether `key` already has an entry.
    ///
    /// # Errors
    /// [`ConfigError::SecretFileUnreadable`] if the file exists but cannot be read.
    pub fn contains(&self, key: &str) -> ConfigResult<bool> {
        match std::fs::read_to_string(&self.path) {
            Ok(contents) => {
                let lines: Vec<&str> = contents.lines().collect();
                Ok(entry_span(&lines, key).is_some())
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(self.unreadable(error)),
        }
    }

    fn write_owner_only(&self, contents: &str) -> ConfigResult<()> {
        crate::SecretFile::at(&self.path).write(contents)
    }

    fn unreadable(&self, error: std::io::Error) -> ConfigError {
        ConfigError::SecretFileUnreadable {
            path: self.path.display().to_string(),
            message: error.to_string(),
        }
    }
}

/// Escapes only what would end the double-quoted span; newlines stay literal.
fn quote(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

/// The `[start, end)` line span of `key`'s entry, following a quoted value across lines.
fn entry_span(lines: &[&str], key: &str) -> Option<(usize, usize)> {
    let prefix = format!("{key}=");
    let start = lines
        .iter()
        .position(|line| line.trim_start().starts_with(&prefix))?;

    let after_eq = &lines[start][lines[start].find('=')? + 1..];
    let Some(quote) = after_eq.chars().next().filter(|c| *c == '"' || *c == '\'') else {
        return Some((start, start + 1)); // unquoted → single line
    };

    let mut segment = &after_eq[quote.len_utf8()..];
    let mut index = start;
    loop {
        if unescaped(segment, quote).is_some() {
            return Some((start, index + 1));
        }
        index += 1;
        match lines.get(index) {
            Some(next) => segment = next,
            None => return Some((start, lines.len())), // unterminated → to EOF
        }
    }
}

fn unescaped(s: &str, quote: char) -> Option<usize> {
    let mut escaped = false;
    for (offset, ch) in s.char_indices() {
        if escaped {
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == quote {
            return Some(offset);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("fundamental_dev_vars_{name}"))
    }

    #[test]
    fn setting_a_key_twice_leaves_one_entry() {
        let path = temp_path("upsert");
        std::fs::remove_file(&path).ok();
        let file = DevVarsFile::at(&path);

        file.set("KEY", "first").expect("first write");
        file.set("KEY", "second").expect("second write");

        let contents = std::fs::read_to_string(&path).expect("read back");
        assert_eq!(contents.matches("KEY=").count(), 1, "{contents}");
        assert!(contents.contains(r#"KEY="second""#), "{contents}");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn a_value_containing_quotes_is_escaped() {
        let path = temp_path("escape");
        std::fs::remove_file(&path).ok();

        DevVarsFile::at(&path)
            .set("TRICKY", r#"say "hi" \ bye"#)
            .expect("write");

        let contents = std::fs::read_to_string(&path).expect("read back");
        assert!(contents.contains(r#"\"hi\""#), "{contents}");
        assert!(contents.contains(r"\\"), "{contents}");
        assert_eq!(contents.trim_end().matches('\n').count(), 0, "{contents}");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn set_if_absent_does_not_replace_an_existing_key() {
        let path = temp_path("if_absent");
        std::fs::remove_file(&path).ok();
        let file = DevVarsFile::at(&path);

        assert!(file.set_if_absent("KEY", "original").expect("first"));
        assert!(!file.set_if_absent("KEY", "rotated").expect("second"));

        let contents = std::fs::read_to_string(&path).expect("read back");
        assert!(contents.contains(r#"KEY="original""#), "{contents}");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn replacing_a_multiline_value_removes_the_whole_entry() {
        let path = temp_path("multiline_replace");
        std::fs::remove_file(&path).ok();
        let file = DevVarsFile::at(&path);
        let pem = "-----BEGIN PRIVATE KEY-----\nAAAA\nBBBB\n-----END PRIVATE KEY-----";

        file.set("BEFORE", "keep-me").expect("write");
        file.set("PEM", pem).expect("write pem");
        file.set("AFTER", "keep-me-too").expect("write");
        file.set("PEM", "replaced").expect("replace pem");

        let contents = std::fs::read_to_string(&path).expect("read back");
        assert!(!contents.contains("AAAA"), "orphaned PEM body: {contents}");
        assert!(!contents.contains("BBBB"), "orphaned PEM body: {contents}");
        assert!(
            !contents.contains("-----END"),
            "orphaned PEM tail: {contents}"
        );
        assert!(contents.contains(r#"PEM="replaced""#), "{contents}");
        assert!(contents.contains(r#"BEFORE="keep-me""#), "{contents}");
        assert!(contents.contains(r#"AFTER="keep-me-too""#), "{contents}");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn setting_a_key_preserves_the_others() {
        let path = temp_path("preserve");
        std::fs::remove_file(&path).ok();
        let file = DevVarsFile::at(&path);

        file.set("FIRST", "1").expect("write");
        file.set("SECOND", "2").expect("write");
        file.set("FIRST", "rotated").expect("rewrite");

        let contents = std::fs::read_to_string(&path).expect("read back");
        assert!(contents.contains(r#"SECOND="2""#), "{contents}");
        assert!(contents.contains(r#"FIRST="rotated""#), "{contents}");
        std::fs::remove_file(&path).ok();
    }

    #[cfg(unix)]
    #[test]
    fn the_file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;

        let path = temp_path("perms");
        std::fs::remove_file(&path).ok();
        DevVarsFile::at(&path)
            .set("SECRET", "value")
            .expect("write");

        let mode = std::fs::metadata(&path).expect("stat").permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "mode was {:o}", mode & 0o777);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn contains_reports_a_missing_file_as_absent() {
        let path = temp_path("absent");
        std::fs::remove_file(&path).ok();
        assert!(!DevVarsFile::at(&path).contains("ANY").expect("check"));
    }

    #[test]
    fn a_multiline_value_is_quoted_so_it_round_trips() {
        let path = temp_path("pem");
        std::fs::remove_file(&path).ok();
        let pem = "-----BEGIN PRIVATE KEY-----\nAAAA\n-----END PRIVATE KEY-----";

        DevVarsFile::at(&path).set("PEM", pem).expect("write");

        let contents = std::fs::read_to_string(&path).expect("read back");
        assert!(
            contents.contains("-----BEGIN PRIVATE KEY-----"),
            "{contents}"
        );
        assert!(contents.starts_with("PEM=\""), "{contents}");
        std::fs::remove_file(&path).ok();
    }
}
