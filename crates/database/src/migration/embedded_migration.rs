//! [`EmbeddedMigration`]: one migration compiled into the crate that owns its table.

/// One migration a crate carries in its own binary, applied by the app's migrator.
///
/// `up` is normally `include_str!` of a file beside the crate's sources.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EmbeddedMigration {
    name: &'static str,
    up: &'static str,
}

impl EmbeddedMigration {
    /// A migration named `name` whose forward statements are `up`.
    ///
    /// `name` follows the on-disk naming rule, `[A-Za-z0-9._-]`, checked when the set is applied.
    #[must_use]
    pub const fn new(name: &'static str, up: &'static str) -> Self {
        Self { name, up }
    }

    /// The migration's name within its owner.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        self.name
    }

    /// The forward statements.
    #[must_use]
    pub const fn up(&self) -> &'static str {
        self.up
    }
}
