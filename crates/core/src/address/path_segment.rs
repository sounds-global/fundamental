//! [`PathSegment`] — a value that may stand as one segment of a link.

#[cfg(not(feature = "std"))]
use alloc::string::String;

/// A value that may stand as one segment of a link: a public form of a record — a handle, a
/// slug, a scoped number — or [`SegmentText`](crate::address::SegmentText), which escapes
/// whatever it carries.
///
/// Implemented for no raw id, so a link built from a UUID or an unescaped string does not
/// compile.
///
/// ```compile_fail
/// use fundamental_core::address::PathSegment;
///
/// fn link(team: impl PathSegment) -> String {
///     format!("/{}", team.to_segment())
/// }
/// // A raw id is not a public form: this does not compile.
/// link(uuid::Uuid::nil());
/// ```
///
/// ```
/// use fundamental_core::address::{PathSegment, SegmentText};
///
/// fn link(tab: impl PathSegment) -> String {
///     format!("/projecten/{}", tab.to_segment())
/// }
/// assert_eq!(link(SegmentText::new("a/b").expect("not empty")), "/projecten/a%2Fb");
/// ```
pub trait PathSegment {
    /// The segment as a link carries it: never empty, and never holding a `/`, `?` or `#`.
    fn to_segment(&self) -> String;
}

impl<K: crate::model::HasHandle> PathSegment for crate::model::Handle<K> {
    fn to_segment(&self) -> String {
        self.to_text()
    }
}
