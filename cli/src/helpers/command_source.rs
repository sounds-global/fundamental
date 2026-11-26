//! [`CommandSource`], where one entry in the command index was declared.

use std::fmt;

/// Where a command came from, and everything needed to run it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CommandSource {
    /// A `[commands.<namespace>]` entry: a shell line run through the system shell.
    Script {
        run: String,
        /// Relative to the project root.
        declared_in: String,
    },
    /// A `#[command]` in an app's `commands` binary, found by reading the sources.
    Rust {
        app: String,
        /// Relative to the project root.
        declared_in: String,
    },
}

impl CommandSource {
    pub(crate) fn app(&self) -> Option<&str> {
        match self {
            Self::Script { .. } => None,
            Self::Rust { app, .. } => Some(app),
        }
    }
}

impl fmt::Display for CommandSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Script { declared_in, .. } | Self::Rust { declared_in, .. } => {
                f.write_str(declared_in)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_script_has_no_app() {
        let source = CommandSource::Script {
            run: "scripts/docs-check.sh".to_string(),
            declared_in: "fun.toml".to_string(),
        };
        assert_eq!(source.app(), None);
        assert_eq!(source.to_string(), "fun.toml");
    }

    #[test]
    fn a_rust_command_names_its_app_and_its_file() {
        let source = CommandSource::Rust {
            app: "layer0".to_string(),
            declared_in: "apps/layer0/src/bin/commands/mint_operator_token.rs".to_string(),
        };
        assert_eq!(source.app(), Some("layer0"));
        assert_eq!(
            source.to_string(),
            "apps/layer0/src/bin/commands/mint_operator_token.rs"
        );
    }
}
