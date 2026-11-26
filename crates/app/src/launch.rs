use std::ffi::OsString;
use std::path::PathBuf;

use fundamental_config::AppSettings;

/// What an app's binary was asked to do by its arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Launch {
    /// Serve the app.
    Serve,
    /// `check-config [manifest]`: check the release at `manifest` and start nothing.
    CheckConfig(PathBuf),
}

impl Launch {
    /// What `args`, the arguments after the binary's name, ask for.
    pub(crate) fn from_args(mut args: impl Iterator<Item = OsString>) -> Self {
        match args.next() {
            Some(word) if word == "check-config" => Self::CheckConfig(
                args.next()
                    .map_or_else(|| PathBuf::from(AppSettings::MANIFEST), PathBuf::from),
            ),
            _ => Self::Serve,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn launch(args: &[&str]) -> Launch {
        Launch::from_args(args.iter().map(OsString::from))
    }

    #[test]
    fn no_argument_serves() {
        assert_eq!(launch(&[]), Launch::Serve);
    }

    #[test]
    fn check_config_checks_the_manifest_named_or_the_default() {
        assert_eq!(
            launch(&["check-config", "next/fun.toml"]),
            Launch::CheckConfig(PathBuf::from("next/fun.toml"))
        );
        assert_eq!(
            launch(&["check-config"]),
            Launch::CheckConfig(PathBuf::from(AppSettings::MANIFEST))
        );
    }
}
