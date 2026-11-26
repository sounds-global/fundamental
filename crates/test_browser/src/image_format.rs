/// How [`Page::screenshot`](crate::Page::screenshot) encodes what it captures.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ImageFormat {
    /// Lossless: what a pixel-for-pixel comparison needs.
    Png,
    /// WebP at `quality` (0 to 100): what a page serves.
    Webp {
        /// The encoder's quality, 0 to 100; above 100 is taken as 100.
        quality: u8,
    },
}

impl ImageFormat {
    pub(crate) fn devtools(self) -> (&'static str, Option<u8>) {
        match self {
            Self::Png => ("png", None),
            Self::Webp { quality } => ("webp", Some(quality.min(100))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_format_names_its_devtools_encoding_and_quality() {
        assert_eq!(ImageFormat::Png.devtools(), ("png", None));
        assert_eq!(
            ImageFormat::Webp { quality: 90 }.devtools(),
            ("webp", Some(90))
        );
        assert_eq!(
            ImageFormat::Webp { quality: 250 }.devtools(),
            ("webp", Some(100))
        );
    }
}
