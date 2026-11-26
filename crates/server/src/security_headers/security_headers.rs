use crate::{
    ContentSecurityPolicy, CspHash, CspSource, FrameAncestors, HeaderCombine, Hsts, Indexing,
    OpenerPolicy, ReferrerPolicy, SecurityHeader,
};

/// The security headers a server stamps on every response, applied by the backend's outermost layer.
///
/// HSTS is the one header off by default.
///
/// ```
/// # extern crate fundamental_server;
/// use fundamental_server::{FrameAncestors, Hsts, Indexing, SecurityHeaders};
///
/// let preview = SecurityHeaders::api()
///     .with_hsts(Hsts::preload())
///     .with_indexing(Indexing::Private);
/// let names: Vec<_> = preview.headers().iter().map(|h| h.name()).collect();
/// assert!(names.contains(&"strict-transport-security"));
/// assert!(names.contains(&"x-robots-tag"));
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecurityHeaders {
    hsts: Option<Hsts>,
    csp: Option<ContentSecurityPolicy>,
    frame_ancestors: FrameAncestors,
    indexing: Indexing,
    referrer: ReferrerPolicy,
    opener: Option<OpenerPolicy>,
    deny_powerful_features: bool,
    sandboxed: bool,
}

impl SecurityHeaders {
    /// What an API server sends: never framed, never sniffed, the referrer trimmed.
    #[must_use]
    pub fn api() -> Self {
        Self {
            hsts: None,
            csp: None,
            frame_ancestors: FrameAncestors::None,
            indexing: Indexing::Public,
            referrer: ReferrerPolicy::StrictOriginWhenCrossOrigin,
            opener: None,
            deny_powerful_features: false,
            sandboxed: false,
        }
    }

    /// What a server of untrusted uploaded content sends: `default-src 'none'; sandbox`, no referrer, never indexed.
    #[must_use]
    pub fn untrusted_content() -> Self {
        Self {
            indexing: Indexing::Private,
            referrer: ReferrerPolicy::NoReferrer,
            deny_powerful_features: true,
            sandboxed: true,
            ..Self::api()
        }
    }

    /// What a web server sends: everything [`api`](Self::api) does, plus a strict [`ContentSecurityPolicy`],
    /// `Cross-Origin-Opener-Policy` and a `Permissions-Policy`.
    #[must_use]
    pub fn web() -> Self {
        Self {
            csp: Some(ContentSecurityPolicy::strict()),
            opener: Some(OpenerPolicy::SameOrigin),
            deny_powerful_features: true,
            ..Self::api()
        }
    }

    /// Sends `Strict-Transport-Security`.
    #[must_use]
    pub fn with_hsts(mut self, hsts: Hsts) -> Self {
        self.hsts = Some(hsts);
        self
    }

    /// Admits the inline script with this digest, adding a strict policy if none is set.
    #[must_use]
    pub fn with_script_hash(mut self, hash: CspHash) -> Self {
        let csp = self.csp.take().unwrap_or_default();
        self.csp = Some(csp.with_script_hash(hash));
        self
    }

    /// Admits the inline `<style>` with this digest, adding a strict policy if none is set.
    #[must_use]
    pub fn with_style_hash(mut self, hash: CspHash) -> Self {
        let csp = self.csp.take().unwrap_or_default();
        self.csp = Some(csp.with_style_hash(hash));
        self
    }

    /// Allows images from `source` as well as this origin, adding a strict policy if none is set.
    #[must_use]
    pub fn with_image_source(mut self, source: CspSource) -> Self {
        let csp = self.csp.take().unwrap_or_default();
        self.csp = Some(csp.with_image_source(source));
        self
    }

    /// Allows fonts from `source` as well as this origin, adding a strict policy if none is set.
    #[must_use]
    pub fn with_font_source(mut self, source: CspSource) -> Self {
        let csp = self.csp.take().unwrap_or_default();
        self.csp = Some(csp.with_font_source(source));
        self
    }

    /// Replaces the Content Security Policy.
    #[must_use]
    pub fn with_csp(mut self, csp: ContentSecurityPolicy) -> Self {
        self.csp = Some(csp);
        self
    }

    /// Sets who may frame this surface.
    #[must_use]
    pub fn with_frame_ancestors(mut self, ancestors: FrameAncestors) -> Self {
        self.frame_ancestors = ancestors;
        self
    }

    /// Sets whether this surface may be indexed.
    #[must_use]
    pub fn with_indexing(mut self, indexing: Indexing) -> Self {
        self.indexing = indexing;
        self
    }

    /// Sets the `Referrer-Policy`.
    #[must_use]
    pub fn with_referrer_policy(mut self, referrer: ReferrerPolicy) -> Self {
        self.referrer = referrer;
        self
    }

    /// Sets the `Cross-Origin-Opener-Policy`.
    #[must_use]
    pub fn with_opener_policy(mut self, opener: OpenerPolicy) -> Self {
        self.opener = Some(opener);
        self
    }

    /// Whether this surface may be indexed.
    #[must_use]
    pub fn indexing(&self) -> Indexing {
        self.indexing
    }

    /// The Content Security Policy, if one is sent.
    #[must_use]
    pub fn csp(&self) -> Option<&ContentSecurityPolicy> {
        self.csp.as_ref()
    }

    /// Every header this policy owns, with its value and how it meets a handler's own.
    #[must_use]
    #[expect(clippy::too_many_lines, reason = "readability baseline")]
    pub fn headers(&self) -> Vec<SecurityHeader> {
        let mut headers = Vec::with_capacity(8);
        if let Some(hsts) = &self.hsts {
            headers.push(SecurityHeader::new(
                "strict-transport-security",
                hsts.value(),
                HeaderCombine::Replace,
            ));
        }
        let csp = match (&self.csp, self.sandboxed) {
            (_, true) => format!(
                "default-src 'none'; sandbox; {}",
                self.frame_ancestors.directive()
            ),
            (Some(csp), false) => csp.value(&self.frame_ancestors),
            (None, false) => self.frame_ancestors.directive(),
        };
        headers.push(SecurityHeader::new(
            "content-security-policy",
            csp,
            HeaderCombine::Alongside,
        ));
        if let Some(value) = self.frame_ancestors.x_frame_options() {
            headers.push(SecurityHeader::new(
                "x-frame-options",
                value,
                HeaderCombine::Replace,
            ));
        }
        headers.push(SecurityHeader::new(
            "x-content-type-options",
            "nosniff",
            HeaderCombine::Replace,
        ));
        headers.push(SecurityHeader::new(
            "referrer-policy",
            self.referrer.token(),
            HeaderCombine::Replace,
        ));
        if let Some(opener) = self.opener {
            headers.push(SecurityHeader::new(
                "cross-origin-opener-policy",
                opener.token(),
                HeaderCombine::Replace,
            ));
        }
        if self.deny_powerful_features {
            headers.push(SecurityHeader::new(
                "permissions-policy",
                "accelerometer=(), camera=(), geolocation=(), gyroscope=(), magnetometer=(), \
                 microphone=(), payment=(), usb=()",
                HeaderCombine::Replace,
            ));
        }
        if self.indexing == Indexing::Private {
            headers.push(SecurityHeader::new(
                "x-robots-tag",
                "noindex, nofollow",
                HeaderCombine::Alongside,
            ));
        }
        headers
    }
}

impl Default for SecurityHeaders {
    fn default() -> Self {
        Self::api()
    }
}

#[cfg(test)]
mod tests {
    use fundamental_core::address::WebOrigin;

    use super::*;

    fn value<'a>(headers: &'a [SecurityHeader], name: &str) -> Option<&'a str> {
        headers
            .iter()
            .find(|header| header.name() == name)
            .map(SecurityHeader::value)
    }

    #[test]
    fn an_api_server_is_unframeable_unsniffable_and_says_nothing_it_need_not() {
        let headers = SecurityHeaders::api().headers();
        assert_eq!(
            value(&headers, "content-security-policy"),
            Some("frame-ancestors 'none'")
        );
        assert_eq!(value(&headers, "x-frame-options"), Some("DENY"));
        assert_eq!(value(&headers, "x-content-type-options"), Some("nosniff"));
        assert_eq!(
            value(&headers, "referrer-policy"),
            Some("strict-origin-when-cross-origin")
        );
        assert_eq!(value(&headers, "strict-transport-security"), None);
        assert_eq!(value(&headers, "x-robots-tag"), None);
    }

    #[test]
    fn a_web_server_without_a_hash_still_admits_no_inline_script() {
        let headers = SecurityHeaders::web().headers();
        let csp = value(&headers, "content-security-policy").expect("a csp");
        assert!(
            csp.contains("script-src 'self' 'wasm-unsafe-eval';"),
            "{csp}"
        );
    }

    #[test]
    fn a_web_server_adds_a_strict_csp_with_its_hydration_hash() {
        let hash =
            CspHash::sha256_base64("47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU=").expect("hash");
        let headers = SecurityHeaders::web().with_script_hash(hash).headers();
        let csp = value(&headers, "content-security-policy").expect("a csp");
        assert!(
            csp.starts_with("default-src 'none'; script-src 'self' 'wasm-unsafe-eval' 'sha256-")
        );
        assert!(csp.contains("frame-ancestors 'none'"));
        assert_eq!(
            value(&headers, "cross-origin-opener-policy"),
            Some("same-origin")
        );
        assert!(value(&headers, "permissions-policy").is_some_and(|v| v.contains("camera=()")));
    }

    #[test]
    fn data_images_widen_only_the_image_directive() {
        let headers = SecurityHeaders::web()
            .with_image_source(CspSource::Data)
            .headers();
        let csp = value(&headers, "content-security-policy").expect("a csp");
        assert!(csp.contains("img-src 'self' data:;"), "{csp}");
        assert!(csp.contains("default-src 'none';"), "{csp}");
        assert!(
            csp.contains("script-src 'self' 'wasm-unsafe-eval';"),
            "{csp}"
        );
        assert!(csp.contains("font-src 'self';"), "{csp}");
        assert!(csp.contains("style-src 'self';"), "{csp}");
    }

    #[test]
    fn a_style_hash_and_data_fonts_widen_only_their_directives() {
        let hash =
            CspHash::sha256_base64("47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU=").expect("hash");
        let headers = SecurityHeaders::web()
            .with_style_hash(hash)
            .with_font_source(CspSource::Data);
        let csp = headers
            .headers()
            .into_iter()
            .find(|header| header.name() == "content-security-policy")
            .expect("a CSP")
            .value()
            .to_owned();
        assert!(
            csp.contains("style-src 'self' 'sha256-47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU='")
        );
        assert!(csp.contains("font-src 'self' data:"));
        assert!(csp.contains("default-src 'none'"));
        assert!(!csp.contains("'unsafe-inline'"));
    }

    #[test]
    fn a_private_surface_is_noindex_and_an_allow_list_drops_x_frame_options() {
        let mijn = WebOrigin::parse("https://mijn.example.nl").expect("origin");
        let headers = SecurityHeaders::api()
            .with_indexing(Indexing::Private)
            .with_frame_ancestors(FrameAncestors::Origins(vec![mijn]))
            .headers();
        assert_eq!(value(&headers, "x-robots-tag"), Some("noindex, nofollow"));
        assert_eq!(value(&headers, "x-frame-options"), None);
        assert_eq!(
            value(&headers, "content-security-policy"),
            Some("frame-ancestors https://mijn.example.nl")
        );
    }

    #[test]
    fn only_csp_and_robots_are_kept_beside_a_handlers_own() {
        let headers = SecurityHeaders::api()
            .with_hsts(Hsts::preload())
            .with_indexing(Indexing::Private)
            .headers();
        for header in &headers {
            let alongside = matches!(header.name(), "content-security-policy" | "x-robots-tag");
            assert_eq!(
                header.combine() == HeaderCombine::Alongside,
                alongside,
                "{}",
                header.name()
            );
        }
    }

    #[test]
    fn untrusted_content_is_sandboxed_unindexed_and_sends_no_referrer() {
        let headers = SecurityHeaders::untrusted_content().headers();
        let csp = value(&headers, "content-security-policy").expect("a policy");
        assert!(csp.starts_with("default-src 'none'; sandbox;"), "{csp}");
        assert!(!csp.contains("script-src"), "{csp}");
        assert_eq!(value(&headers, "x-content-type-options"), Some("nosniff"));
        assert_eq!(value(&headers, "referrer-policy"), Some("no-referrer"));
        assert_eq!(value(&headers, "x-robots-tag"), Some("noindex, nofollow"));
    }
}
