use fundamental_core::escape;

/// An icon a site serves at `path` and links from every page's head, declared once.
///
/// ```
/// # extern crate fundamental_web;
/// use fundamental_web::SiteIcon;
///
/// const ICONS: [SiteIcon; 2] = [
///     SiteIcon::new("/favicon.ico", "image/x-icon", b"\0\0\x01\0").sizes("48x48"),
///     SiteIcon::new("/icon.svg", "image/svg+xml", b"<svg/>").typed(),
/// ];
/// assert_eq!(
///     ICONS[1].link(),
///     r#"<link rel="icon" href="/icon.svg" type="image/svg+xml">"#
/// );
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SiteIcon {
    path: &'static str,
    content_type: &'static str,
    bytes: &'static [u8],
    rel: &'static str,
    sizes: Option<&'static str>,
    typed: bool,
}

impl SiteIcon {
    /// The icon `bytes`, of media type `content_type`, served at `path` and linked as `rel="icon"`.
    #[must_use]
    pub const fn new(path: &'static str, content_type: &'static str, bytes: &'static [u8]) -> Self {
        Self {
            path,
            content_type,
            bytes,
            rel: "icon",
            sizes: None,
            typed: false,
        }
    }

    /// Linked as `rel` instead, such as `"apple-touch-icon"`.
    #[must_use]
    pub const fn rel(mut self, rel: &'static str) -> Self {
        self.rel = rel;
        self
    }

    /// The link says the icon's pixel sizes, such as `"48x48"`.
    #[must_use]
    pub const fn sizes(mut self, sizes: &'static str) -> Self {
        self.sizes = Some(sizes);
        self
    }

    /// The link says the icon's media type, so a browser that cannot draw it skips it.
    #[must_use]
    pub const fn typed(mut self) -> Self {
        self.typed = true;
        self
    }

    /// The address it is served at, from the site's root.
    #[must_use]
    pub const fn path(&self) -> &'static str {
        self.path
    }

    /// Its media type.
    #[must_use]
    pub const fn content_type(&self) -> &'static str {
        self.content_type
    }

    /// The file.
    #[must_use]
    pub const fn bytes(&self) -> &'static [u8] {
        self.bytes
    }

    /// The `<link>` a page's head writes for it.
    #[must_use]
    pub fn link(&self) -> String {
        let mut link = format!(
            "<link rel=\"{}\" href=\"{}\"",
            escape(self.rel),
            escape(self.path)
        );
        if let Some(sizes) = self.sizes {
            link.push_str(&format!(" sizes=\"{}\"", escape(sizes)));
        }
        if self.typed {
            link.push_str(&format!(" type=\"{}\"", escape(self.content_type)));
        }
        link.push('>');
        link
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_icon_is_linked_with_what_it_declares_and_nothing_else() {
        let favicon = SiteIcon::new("/favicon.ico", "image/x-icon", b"").sizes("48x48");
        let touch =
            SiteIcon::new("/apple-touch-icon.png", "image/png", b"").rel("apple-touch-icon");
        assert_eq!(
            favicon.link(),
            r#"<link rel="icon" href="/favicon.ico" sizes="48x48">"#
        );
        assert_eq!(
            touch.link(),
            r#"<link rel="apple-touch-icon" href="/apple-touch-icon.png">"#
        );
    }
}
