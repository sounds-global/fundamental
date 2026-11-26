//! A bucket nobody reads directly.

use crate::Visibility;

/// A bucket whose objects only this framework can read: it accepts content only to seal it,
/// under a key each object has alone, and hands it back only by opening it.
#[derive(Debug, Clone, Copy)]
pub struct Private;

impl Visibility for Private {
    const NAME: &'static str = "private";
}
