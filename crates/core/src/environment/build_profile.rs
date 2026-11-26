//! [`BuildProfile`]: whether the running binary was built for a developer or to ship.

use core::fmt;

/// Whether the running binary was built for a developer or to ship.
///
/// # Examples
///
/// ```
/// use fundamental_core::BuildProfile;
///
/// // A test suite runs as a debug build.
/// assert_eq!(BuildProfile::CURRENT, BuildProfile::Debug);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BuildProfile {
    /// Built with `debug_assertions`: `cargo run`, `cargo test`, `fun dev`.
    Debug,
    /// Built without `debug_assertions`: every image and installer that ships.
    Release,
}

impl BuildProfile {
    /// The profile `fundamental_core` was compiled in.
    pub const CURRENT: Self = if cfg!(debug_assertions) {
        Self::Debug
    } else {
        Self::Release
    };

    /// The profile's name, as `cargo` spells it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Debug => "debug",
            Self::Release => "release",
        }
    }
}

impl fmt::Display for BuildProfile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_current_profile_follows_debug_assertions() {
        assert_eq!(
            BuildProfile::CURRENT == BuildProfile::Debug,
            cfg!(debug_assertions)
        );
    }
}
