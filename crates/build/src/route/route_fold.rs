//! [`RouteFold`], every page registered by its own small function and folded over the router.

use std::fmt::Write as _;

/// How a tree's pages are registered: one private `__route_{index}` function per page, folded
/// over the router in `register_pages`, so the generated code holds the lints at any page count.
pub(super) struct RouteFold<'a> {
    /// What follows `__route_{index}` up to the page's own bounds: generics, the router
    /// parameter, the return type and the start of a `where` clause, if any.
    pub(super) signature: &'a str,
    /// What names one function in the list after `__route_{index}`: `::<S>`, or nothing.
    pub(super) turbofish: &'a str,
}

impl RouteFold<'_> {
    /// Emits `__route_{index}`, whose `bounds` close the signature and whose `body` registers the page.
    pub(super) fn emit_route(&self, out: &mut String, index: usize, bounds: &str, body: &str) {
        let _ = write!(
            out,
            "\nfn __route_{index}{signature}{bounds}{{\n    {body}\n}}\n",
            signature = self.signature,
        );
    }

    /// Emits the `register_pages` statements folding the `count` route functions over `router`.
    pub(super) fn emit_fold(&self, out: &mut String, count: usize) {
        let list: Vec<String> = (0..count)
            .map(|index| format!("__route_{index}{}", self.turbofish))
            .collect();
        let _ = write!(
            out,
            "    let __routes: [fn(_) -> _; {count}] = [{list}];\n    \
             let __router = __routes\n        \
             .into_iter()\n        \
             .try_fold(router, |__router, __route| __route(__router))?;\n",
            list = list.join(", "),
        );
    }
}
