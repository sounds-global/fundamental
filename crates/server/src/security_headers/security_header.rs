use crate::HeaderCombine;

/// One response header a [`SecurityHeaders`](crate::SecurityHeaders) policy owns, assembled from typed parts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecurityHeader {
    name: &'static str,
    value: String,
    combine: HeaderCombine,
}

impl SecurityHeader {
    pub(crate) fn new(
        name: &'static str,
        value: impl Into<String>,
        combine: HeaderCombine,
    ) -> Self {
        Self {
            name,
            value: value.into(),
            combine,
        }
    }

    /// The header's name, in lower case.
    #[must_use]
    pub fn name(&self) -> &'static str {
        self.name
    }

    /// The header's value.
    #[must_use]
    pub fn value(&self) -> &str {
        &self.value
    }

    /// How this value meets one a handler set for the same name.
    #[must_use]
    pub fn combine(&self) -> HeaderCombine {
        self.combine
    }
}
