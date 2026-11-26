use std::any::TypeId;

/// The parameters a route's policies declared public, the only ones a loader may read.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct PublicParams {
    pub(crate) raw: Vec<&'static str>,
    pub(crate) segments: Vec<(&'static str, TypeId)>,
}

impl PublicParams {
    /// Whether `name` is declared public in either form.
    pub(crate) fn declares(&self, name: &str) -> bool {
        self.raw.contains(&name) || self.segments.iter().any(|(declared, _)| *declared == name)
    }
}
