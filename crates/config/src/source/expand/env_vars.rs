use super::SecretBaseDir;
use crate::{ConfigError, ConfigResult};
use std::path::{Path, PathBuf};

const FILE_PREFIX: &str = "file:";

const CREDENTIAL_PREFIX: &str = "credential:";

const CREDENTIALS_DIRECTORY: &str = "CREDENTIALS_DIRECTORY";

/// The path a `${file:…}` or `${credential:…}` reference names, and the reference as an error shows it.
fn secret_file(var_name: &str, base: &SecretBaseDir) -> Option<(Option<PathBuf>, String)> {
    if let Some(path) = var_name.strip_prefix(FILE_PREFIX) {
        let path = base.resolve(path);
        let shown = path.display().to_string();
        return Some((Some(path), shown));
    }
    let name = var_name.strip_prefix(CREDENTIAL_PREFIX)?;
    let directory = std::env::var_os(CREDENTIALS_DIRECTORY);
    Some((
        credential_file(name, directory.as_deref().map(Path::new)),
        format!("${CREDENTIALS_DIRECTORY}/{name}"),
    ))
}

/// `None` unless `name` is one path segment, so a reference cannot leave the directory.
fn credential_file(name: &str, directory: Option<&Path>) -> Option<PathBuf> {
    let named = !name.is_empty()
        && name != "."
        && name != ".."
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'));
    directory
        .filter(|_| named)
        .map(|directory| directory.join(name))
}

/// Strips the trailing newline; missing, unreadable and empty are one `None`, so a probe learns nothing.
fn read_secret_file(path: &Path) -> Option<String> {
    let contents = std::fs::read_to_string(path).ok()?;
    let trimmed = contents.trim_end_matches(['\n', '\r']);
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

/// Expands `${VAR}`, `${VAR:-default}`, `${file:path}` and `${credential:name}` references in a string.
///
/// A relative file reference resolves against `base`; an empty variable falls back to its default.
///
/// # Errors
/// [`ConfigError::EnvVarNotFound`] for an unset variable without a default, and
/// [`ConfigError::SecretFileUnreadable`] for an unreadable file without one.
///
/// # Examples
///
/// ```no_run
/// use fundamental_config::source::{SecretBaseDir, expand_env_vars};
///
/// let base = SecretBaseDir::default();
///
/// // Simple variable: reads `HOME` from the environment.
/// let path = expand_env_vars("${HOME}/config", &base)?;
///
/// // With default value: uses the default when `API_URL` is unset or empty.
/// let url = expand_env_vars("${API_URL:-http://localhost:8001}", &base)?;
/// assert_eq!(url, "http://localhost:8001");
/// # Ok::<(), fundamental_config::ConfigError>(())
/// ```
pub fn expand_env_vars(s: &str, base: &SecretBaseDir) -> ConfigResult<String> {
    let mut result = s.to_string();
    let mut start = 0;

    while let Some(var_start) = result[start..].find("${") {
        let var_start = start + var_start;
        let Some(var_end) = result[var_start..].find('}') else {
            break;
        };
        let var_end = var_start + var_end;

        let var_expr = &result[var_start + 2..var_end];

        let (var_name, default_value) = if let Some(sep_pos) = var_expr.find(":-") {
            (&var_expr[..sep_pos], Some(&var_expr[sep_pos + 2..]))
        } else {
            (var_expr, None)
        };

        // The resolved path is kept for the error, naming which root a relative miss was tried against.
        let secret = secret_file(var_name, base);

        let resolved = match secret {
            Some((ref path, _)) => path.as_deref().and_then(read_secret_file),
            None => std::env::var(var_name).ok().filter(|v| !v.is_empty()),
        };

        let value = match (resolved, default_value, secret) {
            (Some(value), _, _) => value,
            (None, Some(default), _) => default.to_string(),
            (None, None, Some((_, shown))) => {
                return Err(ConfigError::SecretFileUnreadable {
                    path: shown,
                    message: "unreadable or empty, and no default was given".to_string(),
                });
            }
            (None, None, None) => {
                return Err(ConfigError::EnvVarNotFound {
                    var: var_name.to_string(),
                });
            }
        };

        result.replace_range(var_start..=var_end, &value);
        start = var_start + value.len();
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_credential_is_a_file_in_the_directory_systemd_passed() {
        let directory = Path::new("/run/credentials/a7d-preview@x.service");
        assert_eq!(
            credential_file("bao-secret-id", Some(directory)),
            Some(directory.join("bao-secret-id"))
        );
        assert_eq!(
            credential_file("mail.dek", Some(directory)),
            Some(directory.join("mail.dek"))
        );
    }

    #[test]
    fn a_credential_never_leaves_its_directory() {
        let directory = Path::new("/run/credentials/a7d-preview@x.service");
        for name in ["", ".", "..", "../x", "a/b", "/etc/shadow", "a b", "naïve"] {
            assert_eq!(credential_file(name, Some(directory)), None, "{name:?}");
        }
    }

    #[test]
    fn no_credential_is_read_where_none_was_passed() {
        assert_eq!(credential_file("bao-secret-id", None), None);
    }

    #[test]
    fn an_unreadable_credential_fails_unless_it_has_a_default() {
        let base = SecretBaseDir::default();
        let refused = expand_env_vars("${credential:never-passed-to-a-test}", &base)
            .expect_err("no such credential");
        assert!(
            matches!(&refused, ConfigError::SecretFileUnreadable { path, .. }
                if path.ends_with("/never-passed-to-a-test")),
            "{refused:?}"
        );
        assert_eq!(
            expand_env_vars("${credential:never-passed-to-a-test:-fallback}", &base)
                .expect("the default"),
            "fallback"
        );
    }
}
