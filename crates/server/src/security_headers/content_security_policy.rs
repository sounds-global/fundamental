use fundamental_core::address::WebOrigin;

use crate::{CspHash, CspSource, FrameAncestors, FrameSource};

/// A strict Content Security Policy (CSP Level 3), widened only through typed sources.
///
/// `script-src` accepts nothing but `'self'`, `'wasm-unsafe-eval'` and hashes, and `style-src`
/// never takes `'unsafe-inline'`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ContentSecurityPolicy {
    script_hashes: Vec<CspHash>,
    style_hashes: Vec<CspHash>,
    images: Vec<CspSource>,
    fonts: Vec<CspSource>,
    connections: Vec<WebOrigin>,
    form_targets: Vec<WebOrigin>,
    frames: Vec<FrameSource>,
}

impl ContentSecurityPolicy {
    /// This origin only, for everything; no frames, no plugins, no `<base>`.
    #[must_use]
    pub fn strict() -> Self {
        Self::default()
    }

    /// Admits the inline script whose SHA-256 digest this is.
    #[must_use]
    pub fn with_script_hash(mut self, hash: CspHash) -> Self {
        self.script_hashes.push(hash);
        self
    }

    /// Admits the inline `<style>` element whose SHA-256 digest this is.
    #[must_use]
    pub fn with_style_hash(mut self, hash: CspHash) -> Self {
        self.style_hashes.push(hash);
        self
    }

    /// Allows images from `source` as well as this origin.
    #[must_use]
    pub fn with_image_source(mut self, source: CspSource) -> Self {
        self.images.push(source);
        self
    }

    /// Allows fonts from `source` as well as this origin.
    #[must_use]
    pub fn with_font_source(mut self, source: CspSource) -> Self {
        self.fonts.push(source);
        self
    }

    /// Allows `fetch`, XHR, WebSocket and EventSource connections to `origin`.
    #[must_use]
    pub fn with_connect_source(mut self, origin: WebOrigin) -> Self {
        self.connections.push(origin);
        self
    }

    /// Allows a form to submit to `origin`.
    #[must_use]
    pub fn with_form_action(mut self, origin: WebOrigin) -> Self {
        self.form_targets.push(origin);
        self
    }

    /// Allows `source` to be shown in an `<iframe>`.
    #[must_use]
    pub fn with_frame_source(mut self, source: FrameSource) -> Self {
        self.frames.push(source);
        self
    }

    /// The header value, with `ancestors` as its `frame-ancestors` directive.
    #[must_use]
    pub fn value(&self, ancestors: &FrameAncestors) -> String {
        let mut script = String::from("script-src 'self' 'wasm-unsafe-eval'");
        for hash in &self.script_hashes {
            script.push(' ');
            script.push_str(&hash.source());
        }
        let mut style = String::from("style-src 'self'");
        for hash in &self.style_hashes {
            style.push(' ');
            style.push_str(&hash.source());
        }
        let frames = match self.frames.is_empty() {
            true => "frame-src 'none'".to_owned(),
            false => directive(
                "frame-src",
                self.frames.iter().map(FrameSource::expression),
                false,
            ),
        };
        [
            "default-src 'none'".to_owned(),
            script,
            style,
            directive(
                "img-src",
                self.images
                    .iter()
                    .map(|source| source.expression().to_owned()),
                true,
            ),
            directive(
                "font-src",
                self.fonts
                    .iter()
                    .map(|source| source.expression().to_owned()),
                true,
            ),
            directive(
                "connect-src",
                self.connections
                    .iter()
                    .map(|origin| origin.as_str().to_owned()),
                true,
            ),
            "manifest-src 'self'".to_owned(),
            "media-src 'self'".to_owned(),
            frames,
            directive(
                "form-action",
                self.form_targets
                    .iter()
                    .map(|origin| origin.as_str().to_owned()),
                true,
            ),
            ancestors.directive(),
            "base-uri 'none'".to_owned(),
            "object-src 'none'".to_owned(),
        ]
        .join("; ")
    }
}

fn directive(name: &str, sources: impl Iterator<Item = String>, with_self: bool) -> String {
    let mut directive = String::from(name);
    if with_self {
        directive.push_str(" 'self'");
    }
    for source in sources {
        directive.push(' ');
        directive.push_str(&source);
    }
    directive
}

#[cfg(test)]
mod tests {
    use super::*;

    fn origin(value: &str) -> WebOrigin {
        WebOrigin::parse(value).expect("origin")
    }

    #[test]
    fn strict_names_no_inline_or_third_party_script() {
        let value = ContentSecurityPolicy::strict().value(&FrameAncestors::None);
        assert_eq!(
            value,
            "default-src 'none'; script-src 'self' 'wasm-unsafe-eval'; style-src 'self'; \
             img-src 'self'; font-src 'self'; connect-src 'self'; manifest-src 'self'; \
             media-src 'self'; frame-src 'none'; form-action 'self'; frame-ancestors 'none'; \
             base-uri 'none'; object-src 'none'"
        );
        for widening in [
            "'unsafe-inline'",
            " 'unsafe-eval'",
            " * ",
            "https:",
            "http:",
        ] {
            assert!(!value.contains(widening), "{widening} in {value}");
        }
    }

    #[test]
    fn typed_sources_widen_only_their_own_directive() {
        let hash =
            CspHash::sha256_base64("47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU=").expect("hash");
        let value = ContentSecurityPolicy::strict()
            .with_script_hash(hash)
            .with_image_source(CspSource::Data)
            .with_font_source(CspSource::Origin(origin("https://fonts.example.nl")))
            .with_connect_source(origin("https://api.example.nl"))
            .with_form_action(origin("https://pay.example.nl"))
            .with_frame_source(FrameSource::SubdomainsOf(origin(
                "https://preview.example.nl",
            )))
            .value(&FrameAncestors::Origins(vec![origin(
                "https://mijn.example.nl",
            )]));
        assert!(value.contains(
            "script-src 'self' 'wasm-unsafe-eval' 'sha256-47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU='"
        ));
        assert!(value.contains("img-src 'self' data:"));
        assert!(value.contains("font-src 'self' https://fonts.example.nl"));
        assert!(value.contains("connect-src 'self' https://api.example.nl"));
        assert!(value.contains("form-action 'self' https://pay.example.nl"));
        assert!(value.contains("frame-src https://*.preview.example.nl"));
        assert!(value.contains("frame-ancestors https://mijn.example.nl"));
        assert!(!value.contains("script-src 'self' 'wasm-unsafe-eval' https"));
    }

    #[test]
    fn a_style_hash_admits_one_inline_style_and_nothing_else() {
        let hash =
            CspHash::sha256_base64("47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU=").expect("hash");
        let value = ContentSecurityPolicy::strict()
            .with_style_hash(hash)
            .value(&FrameAncestors::None);
        assert!(value.contains(
            "; style-src 'self' 'sha256-47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU='; img-src"
        ));
        assert!(value.contains("script-src 'self' 'wasm-unsafe-eval'; style-src"));
        assert!(!value.contains("'unsafe-inline'"));
    }
}
