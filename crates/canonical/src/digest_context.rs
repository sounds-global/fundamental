/// The purpose a [`ContextDigest`](crate::ContextDigest) is derived for.
///
/// The string should be globally unique and hard-coded, e.g. `"acme 2026-09-19 attribute value id"`.
pub trait DigestContext {
    /// The context string, fixed for the life of every digest derived under it.
    const CONTEXT: &'static str;
}
