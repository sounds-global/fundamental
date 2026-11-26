use std::path::Path;

use fundamental_canonical::ContextDigest;
use fundamental_core::asset::AssetVersion;
use tracing::warn;

use crate::static_asset_content::StaticAssetContent;

/// A single static file held in memory; its [`version`](Self::version), the digest of its bytes, is its strong `ETag`.
#[derive(Debug, Clone)]
pub struct StaticAsset {
    bytes: Vec<u8>,
    content_type: &'static str,
    cache_control: Option<&'static str>,
    version: Option<AssetVersion>,
}

impl StaticAsset {
    /// Creates an asset whose type is inferred from `path`'s extension.
    #[must_use]
    pub fn new(path: &Path, bytes: Vec<u8>) -> Self {
        Self::typed(bytes, content_type_for(path))
    }

    /// Creates an asset whose content type is declared rather than inferred.
    #[must_use]
    pub fn typed(bytes: Vec<u8>, content_type: &'static str) -> Self {
        Self {
            version: version_of(&bytes),
            bytes,
            content_type,
            cache_control: None,
        }
    }

    /// The same asset holding `bytes` instead, its version derived again.
    #[must_use]
    pub(crate) fn rewritten(&self, bytes: Vec<u8>) -> Self {
        Self {
            version: version_of(&bytes),
            bytes,
            content_type: self.content_type,
            cache_control: self.cache_control,
        }
    }

    /// Serves the asset with this `Cache-Control` value.
    #[must_use]
    pub fn with_cache_control(mut self, cache_control: &'static str) -> Self {
        self.cache_control = Some(cache_control);
        self
    }

    /// The `Cache-Control` value the asset is served with, if one was declared.
    #[must_use]
    pub fn cache_control(&self) -> Option<&'static str> {
        self.cache_control
    }

    /// The asset's bytes.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// The content type a backend stamps on the asset.
    #[must_use]
    pub fn content_type(&self) -> &'static str {
        self.content_type
    }

    /// The version of the asset's bytes: its strong `ETag`, and the `?v=` a page links it by.
    #[must_use]
    pub fn version(&self) -> Option<&AssetVersion> {
        self.version.as_ref()
    }
}

/// The version of `bytes`; `None` is unreachable, and the asset is then served unversioned.
fn version_of(bytes: &[u8]) -> Option<AssetVersion> {
    let encoded = ContextDigest::<StaticAssetContent>::derive(&[bytes]).to_base64url();
    let version = AssetVersion::parse(&encoded);
    if version.is_none() {
        warn!(
            length = encoded.len(),
            "a static asset's digest did not encode as a version; serving it unversioned"
        );
    }
    version
}

/// Maps a file's extension to a content type; anything unrecognized is served as bytes.
fn content_type_for(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("html" | "htm") => "text/html; charset=utf-8",
        Some("js" | "mjs") => "text/javascript; charset=utf-8",
        Some("wasm") => "application/wasm",
        Some("css") => "text/css; charset=utf-8",
        Some("json") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("ico") => "image/x-icon",
        Some("woff2") => "font/woff2",
        Some("woff") => "font/woff",
        Some("txt") => "text/plain; charset=utf-8",
        Some("xml") => "application/xml; charset=utf-8",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn infers_content_type_from_extension() {
        assert_eq!(
            StaticAsset::new(Path::new("app.js"), Vec::new()).content_type(),
            "text/javascript; charset=utf-8"
        );
        assert_eq!(
            StaticAsset::new(Path::new("app_bg.wasm"), Vec::new()).content_type(),
            "application/wasm"
        );
        assert_eq!(
            StaticAsset::new(Path::new("style.css"), Vec::new()).content_type(),
            "text/css; charset=utf-8"
        );
    }

    #[test]
    fn unknown_extension_is_octet_stream() {
        assert_eq!(
            StaticAsset::new(Path::new("data.bin"), Vec::new()).content_type(),
            "application/octet-stream"
        );
        assert_eq!(
            StaticAsset::new(Path::new("noext"), Vec::new()).content_type(),
            "application/octet-stream"
        );
    }

    #[test]
    fn extension_match_is_case_insensitive() {
        assert_eq!(
            StaticAsset::new(Path::new("ICON.PNG"), Vec::new()).content_type(),
            "image/png"
        );
    }

    #[test]
    fn xml_is_application_xml_in_utf_8() {
        assert_eq!(
            StaticAsset::new(Path::new("sitemap.xml"), Vec::new()).content_type(),
            "application/xml; charset=utf-8"
        );
    }

    #[test]
    fn the_version_is_the_bytes_and_changes_with_them() {
        let one = StaticAsset::new(Path::new("app.css"), b"a{}".to_vec());
        let same = StaticAsset::typed(b"a{}".to_vec(), "text/plain");
        let other = one.rewritten(b"a{color:red}".to_vec());
        assert!(one.version().is_some());
        assert_eq!(one.version(), same.version());
        assert_ne!(one.version(), other.version());
        assert_eq!(other.content_type(), "text/css; charset=utf-8");
        assert_eq!(
            other.version(),
            StaticAsset::new(Path::new("x"), b"a{color:red}".to_vec()).version()
        );
    }

    #[test]
    fn a_typed_asset_keeps_its_declared_type_and_cache_policy() {
        let asset = StaticAsset::typed(b"Contact: x".to_vec(), "text/plain; charset=utf-8")
            .with_cache_control("public, max-age=3600");
        assert_eq!(asset.content_type(), "text/plain; charset=utf-8");
        assert_eq!(asset.cache_control(), Some("public, max-age=3600"));
        assert_eq!(
            StaticAsset::new(Path::new("a.txt"), Vec::new()).cache_control(),
            None
        );
    }
}
