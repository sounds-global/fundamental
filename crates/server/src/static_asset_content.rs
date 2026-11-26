use fundamental_canonical::DigestContext;

/// The digest context a static asset is versioned under, so its version equals no other digest.
pub(crate) struct StaticAssetContent;

impl DigestContext for StaticAssetContent {
    const CONTEXT: &'static str = "fundamental.server.v1 static asset content";
}
