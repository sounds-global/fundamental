//! [`Dependency`], one crate a workspace member depends on.

/// One crate a workspace member depends on, as its `Cargo.toml` declares it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Dependency {
    /// The name the depending crate's source uses: the dependency's key, with `-` as `_`.
    pub(crate) ident: String,
    /// The package it resolves to, through `package =` here or in `[workspace.dependencies]`.
    pub(crate) package: String,
    /// The line of the key in the depending crate's `Cargo.toml`.
    pub(crate) line: usize,
    /// Whether the dependency is only for the crate's tests and benches.
    pub(crate) dev: bool,
}
