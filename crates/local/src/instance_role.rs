use std::fmt;

use fundamental_core::address::{LocalSuffix, TestApex};

use crate::InstanceLabel;

/// Which checkout of a project an instance is: main, or a lane with its label.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum InstanceRole {
    /// The project's main checkout, serving at its apexes.
    Main,
    /// A lane, serving under `<label>.<apex's lanes twin>`.
    Lane(InstanceLabel),
}

impl InstanceRole {
    /// The suffix this role's names end in under `apex`.
    #[must_use]
    pub fn suffix(&self, apex: &TestApex) -> LocalSuffix {
        match self {
            Self::Main => LocalSuffix::main(apex.clone()),
            Self::Lane(label) => LocalSuffix::lane(label.dns().clone(), apex.clone()),
        }
    }

    /// The lane's label, or `None` for the main checkout.
    #[must_use]
    pub const fn label(&self) -> Option<&InstanceLabel> {
        match self {
            Self::Main => None,
            Self::Lane(label) => Some(label),
        }
    }
}

impl fmt::Display for InstanceRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Main => f.write_str("main"),
            Self::Lane(label) => write!(f, "lane {label}"),
        }
    }
}
