/// Where a static mount's bytes come from.
///
/// [`Memory`](StaticSource::Memory), read once, is the only one a deployed service should use.
/// [`Disk`](StaticSource::Disk) reads per request, under a canonicalised root.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StaticSource {
    /// Read the directory once, at construction. The default.
    #[default]
    Memory,
    /// Read each file when it is requested, so changes on disk are visible.
    Disk,
}

impl StaticSource {
    /// `Disk` for a development environment, `Memory` for everything else.
    #[must_use]
    pub fn for_development(is_development: bool) -> Self {
        if is_development {
            Self::Disk
        } else {
            Self::Memory
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_is_the_default() {
        assert_eq!(StaticSource::default(), StaticSource::Memory);
    }

    #[test]
    fn only_development_selects_disk() {
        assert_eq!(StaticSource::for_development(true), StaticSource::Disk);
        assert_eq!(StaticSource::for_development(false), StaticSource::Memory);
    }
}
