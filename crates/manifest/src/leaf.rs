//! A value in one manifest file, with where it is written.

use crate::{KeyPath, Origin};

/// One value a manifest file defines: a scalar, an array (an array of tables included), or an
/// empty table. Tables with contents are not leaves; their contents are.
#[derive(Debug, Clone)]
pub(crate) struct Leaf {
    pub(crate) key: KeyPath,
    pub(crate) value: toml::Value,
    pub(crate) origin: Origin,
}
