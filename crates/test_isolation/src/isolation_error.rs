//! [`IsolationError`], why the isolation check could not judge a project.

use std::path::PathBuf;

use fundamental_macros::Error;

/// Why the isolation check could not judge a test source or the run-alone list.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum IsolationError {
    /// A test source is not Rust the check can parse.
    #[error(
        code = "ISO_101",
        message = "'{}' is not Rust the isolation check can read: {message}", file.display(),
        status = 422
    )]
    Unparsable {
        /// The file.
        file: PathBuf,
        /// Why.
        message: String,
    },

    /// A test source or a fixture it includes could not be read.
    #[error(
        code = "ISO_102",
        message = "failed to read '{}': {source}", path.display(),
        status = 500
    )]
    Io {
        /// The path whose read failed.
        path: PathBuf,
        /// The underlying error.
        #[source]
        source: std::io::Error,
    },

    /// The run-alone list holds a term the check cannot read, so it cannot know what runs alone.
    #[error(
        code = "ISO_103",
        message = "the run-alone list cannot be read: {message}",
        status = 422
    )]
    RunAloneUnreadable {
        /// What was wrong.
        message: String,
    },
}

#[cfg(test)]
mod tests {
    use fundamental_core::Error as _;

    use super::IsolationError;

    #[test]
    fn an_unreadable_run_alone_list_names_its_code() {
        let refused = IsolationError::RunAloneUnreadable {
            message: "a term".to_owned(),
        };
        assert_eq!(refused.error_code(), "ISO_103");
        assert_eq!(refused.http_status_code(), 422);
    }
}
