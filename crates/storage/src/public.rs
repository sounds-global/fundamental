//! A bucket anyone with a link may read.

use crate::Visibility;

/// A bucket of artefacts meant to be fetched by anyone with a link — signed release builds and
/// their manifests — stored as they are and never sealed.
#[derive(Debug, Clone, Copy)]
pub struct Public;

impl Visibility for Public {
    const NAME: &'static str = "public";
}
