//! [`CommandError`], the error type returned by every command and by the runner.

use fundamental_core::AnyError;
use fundamental_macros::Error;

/// An error from dispatching or running a command.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum CommandError {
    /// A lower-layer Fundamental error, keeping its own code and status.
    Other(AnyError),

    /// A command reported a failure that does not map onto a more specific variant.
    #[error(code = "CMD_001", message = "Command failed: {message}", status = 500)]
    Message {
        /// The failure description supplied by the command.
        message: String,
    },

    /// No command is registered under the requested name.
    #[error(
        code = "CMD_002",
        message = "Unknown command '{name}'. Available commands: {available}",
        status = 404
    )]
    UnknownCommand {
        /// The name that was requested but not found.
        name: String,
        /// The comma-separated names of the registered commands.
        available: String,
    },

    /// Two or more commands are registered under the same name.
    #[error(
        code = "CMD_003",
        message = "Command '{name}' is registered more than once",
        status = 500
    )]
    DuplicateCommand {
        /// The name that was registered more than once.
        name: String,
    },
}

impl CommandError {
    /// Creates a [`CommandError::Message`].
    pub fn msg(message: impl Into<String>) -> Self {
        Self::Message {
            message: message.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fundamental_core::Error as _;

    #[test]
    fn message_carries_stable_code_and_status() {
        let err = CommandError::msg("could not reach the queue");
        assert_eq!(err.error_code(), "CMD_001");
        assert_eq!(err.http_status_code(), 500);
        assert!(!err.is_retryable());
        assert!(err.to_string().contains("could not reach the queue"));
    }

    #[test]
    fn unknown_command_is_not_found_and_lists_the_available() {
        let err = CommandError::UnknownCommand {
            name: "reindex".into(),
            available: "backfill, hello".into(),
        };
        assert_eq!(err.error_code(), "CMD_002");
        assert_eq!(err.http_status_code(), 404);
        let message = err.to_string();
        assert!(message.contains("reindex"), "{message}");
        assert!(message.contains("backfill, hello"), "{message}");
    }

    #[test]
    fn duplicate_command_is_a_server_error() {
        let err = CommandError::DuplicateCommand {
            name: "hello".into(),
        };
        assert_eq!(err.error_code(), "CMD_003");
        assert_eq!(err.http_status_code(), 500);
        assert!(err.to_string().contains("hello"));
    }

    #[test]
    fn other_catch_all_delegates_to_the_wrapped_error() {
        let inner = CommandError::msg("boom");
        let err: CommandError = AnyError::new(inner).into();
        assert_eq!(err.error_code(), "CMD_001");
        assert_eq!(err.http_status_code(), 500);
        assert!(matches!(err, CommandError::Other(_)));
    }
}
