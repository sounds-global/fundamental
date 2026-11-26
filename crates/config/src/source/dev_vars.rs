//! Development secrets loaded from a git-ignored `.dev.vars` file.

use tracing::{debug, warn};

const DEV_VARS_FILE: &str = ".dev.vars";

/// In development, load `.dev.vars` into the process environment, never overriding a set variable.
///
/// Process-wide, so per-app identity belongs in `DevApp::env` instead.
#[expect(clippy::cognitive_complexity, reason = "readability baseline")]
pub(crate) fn load_dev_vars() {
    match dotenvy::from_filename(DEV_VARS_FILE) {
        Ok(path) => debug!(path = %path.display(), "loaded development secrets from .dev.vars"),
        Err(error) if error.not_found() => {}
        Err(error) => warn!(%error, "failed to load .dev.vars; skipping"),
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    /// Precedence rests on `dotenvy` letting the first load of a key win.
    #[test]
    fn the_first_load_of_a_key_wins() {
        let mut specific = tempfile::NamedTempFile::new().expect("temp file");
        writeln!(specific, "DEV_VARS_TEST_PRECEDENCE=per_app").expect("write");
        let mut shared = tempfile::NamedTempFile::new().expect("temp file");
        writeln!(shared, "DEV_VARS_TEST_PRECEDENCE=shared").expect("write");

        dotenvy::from_path(specific.path()).expect("load the per-app file");
        dotenvy::from_path(shared.path()).expect("load the shared file");

        assert_eq!(
            std::env::var("DEV_VARS_TEST_PRECEDENCE").expect("set"),
            "per_app",
            "the shared file overwrote the app's own value; every app would run as \
             the same identity"
        );
    }

    #[test]
    fn loads_a_multiline_quoted_value() {
        let mut file = tempfile::NamedTempFile::new().expect("temp file");
        writeln!(
            file,
            "DEV_VARS_TEST_MULTILINE=\"-----BEGIN-----\\nmiddle\\n-----END-----\\n\""
        )
        .expect("write");

        dotenvy::from_path(file.path()).expect("load");
        assert_eq!(
            std::env::var("DEV_VARS_TEST_MULTILINE").expect("set"),
            "-----BEGIN-----\nmiddle\n-----END-----\n"
        );
    }
}
