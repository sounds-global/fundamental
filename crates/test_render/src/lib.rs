//! A component's HTML in a test, rendered as a served page renders it.

use std::sync::Arc;

use hydration_context::SsrSharedContext;
use leptos::prelude::*;

/// Renders `view` to HTML under the islands' shared context, as a served page does.
pub fn render_html<V: IntoView>(view: impl FnOnce() -> V) -> String {
    Owner::new_root(Some(Arc::new(SsrSharedContext::new_islands()))).with(|| view().to_html())
}

#[doc(hidden)]
pub use insta;

/// `html` without its `class` attributes, so a structure test survives a change of look.
#[must_use]
pub fn without_classes(html: &str) -> String {
    const CLASS: &str = r#" class=""#;
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(start) = rest.find(CLASS) {
        out.push_str(&rest[..start]);
        let after = &rest[start + CLASS.len()..];
        rest = &after[after.find('"').map_or(after.len(), |end| end + 1)..];
    }
    out.push_str(rest);
    out
}

/// The HTML a snapshot holds: one element per line, island hashes masked, `<!>` markers dropped.
pub fn normalize_html(html: &str) -> String {
    const ATTR: &str = "data-component=\"";
    let html = html.replace("<!>", "");
    let mut out = String::with_capacity(html.len());
    let mut rest = html.as_str();
    while let Some(start) = rest.find(ATTR) {
        let value_start = start + ATTR.len();
        out.push_str(&rest[..value_start]);
        rest = &rest[value_start..];
        let end = rest.find('"').unwrap_or(rest.len());
        let value = &rest[..end];
        match value.rsplit_once('_') {
            Some((name, hash)) if !hash.is_empty() && hash.bytes().all(|b| b.is_ascii_digit()) => {
                out.push_str(name);
                out.push_str("_[hash]");
            }
            _ => out.push_str(value),
        }
        rest = &rest[end..];
    }
    out.push_str(rest);
    out.replace("><", ">\n<")
}

/// Snapshots a component's HTML after `normalize_html`.
#[macro_export]
macro_rules! assert_html_snapshot {
    ($html:expr $(,)?) => {
        $crate::insta::assert_snapshot!($crate::normalize_html(&$html))
    };
}
