use fundamental_core::escape;

use crate::PageContext;

/// The island bootstrap, a module script mirroring Leptos 0.8.20's `island_script.js`.
const BOOTSTRAP: &str = include_str!("hydration_bootstrap.js");

/// The development live reload's client, loaded from `'self'` so the CSP needs no hash for it.
const LIVE_RELOAD_CLIENT: &str = "<script type=\"module\" src=\"/_fundamental/live.js\"></script>";

/// Builds the document markup that boots a page's islands from the bundle served at `js_url`.
///
/// It belongs in `<head>`: the bootstrap refuses a page with a second hydration element.
/// `data-hydration` on `<html>` reads `pending`, `complete` or `failed`.
///
/// ```
/// # extern crate fundamental_web;
/// use fundamental_web::hydration_script;
///
/// let tags = hydration_script("/_fundamental/app.js");
/// assert!(tags.contains("type=\"module\""));
/// assert!(tags.contains("/_fundamental/app.js"));
/// assert!(tags.contains("/_fundamental/app_bg.wasm"));
/// ```
#[must_use]
pub fn hydration_script(js_url: &str) -> String {
    markup(js_url, &wasm_path(js_url))
}

impl PageContext {
    /// [`hydration_script`] for the bundle at `js_path`, its bundle and wasm linked by version.
    ///
    /// ```
    /// # extern crate fundamental_web;
    /// use fundamental_web::PageContext;
    ///
    /// let page = PageContext::new(Vec::new(), "req");
    /// let tags = page.hydration_script("/_fundamental/app.js");
    /// assert!(tags.contains("data-wasm=\"/_fundamental/app_bg.wasm\""));
    /// ```
    #[must_use]
    pub fn hydration_script(&self, js_path: &str) -> String {
        let js = self.asset_url(js_path).to_string();
        let wasm = self.asset_url(&wasm_path(js_path)).to_string();
        let markup = markup(&js, &wasm);
        if self.live_reload() {
            return markup + LIVE_RELOAD_CLIENT;
        }
        markup
    }
}

fn wasm_path(js_path: &str) -> String {
    format!("{}_bg.wasm", js_path.strip_suffix(".js").unwrap_or(js_path))
}

/// The preloads and the bootstrap for the bundle at `js_url` and its wasm at `wasm_url`.
fn markup(js_url: &str, wasm_url: &str) -> String {
    let js = escape(js_url);
    let wasm = escape(wasm_url);
    format!(
        "<link rel=\"modulepreload\" href=\"{js}\">\
<link rel=\"preload\" href=\"{wasm}\" as=\"fetch\" type=\"application/wasm\" crossorigin=\"\">\
<script type=\"module\" data-fundamental-hydration data-bundle=\"{js}\" data-wasm=\"{wasm}\">{BOOTSTRAP}</script>"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emits_the_island_bootstrap_for_the_bundle_url() {
        let tags = hydration_script("/_fundamental/app.js");
        assert!(tags.contains("type=\"module\""), "tags: {tags}");
        assert!(
            tags.contains("data-bundle=\"/_fundamental/app.js\""),
            "tags: {tags}"
        );
        assert!(
            tags.contains("data-wasm=\"/_fundamental/app_bg.wasm\""),
            "tags: {tags}"
        );
        assert!(
            tags.contains("<link rel=\"modulepreload\" href=\"/_fundamental/app.js\">"),
            "tags: {tags}"
        );
        assert!(
            tags.contains("href=\"/_fundamental/app_bg.wasm\""),
            "tags: {tags}"
        );
        assert!(tags.contains("leptos-island"), "tags: {tags}");
    }

    #[test]
    fn a_page_links_the_bundle_and_its_wasm_by_the_versions_its_router_holds() {
        use std::sync::Arc;

        use fundamental_core::asset::{AssetAddresses, AssetVersion};
        let js = AssetVersion::parse("q83vEjRWeJCrze8SNFZ4kKvN7xI0VniQq83vEjRWeJA").expect("valid");
        let wasm =
            AssetVersion::parse("AAAvEjRWeJCrze8SNFZ4kKvN7xI0VniQq83vEjRWeJA").expect("valid");
        let mut addresses = AssetAddresses::default();
        addresses.insert("/_fundamental/app.js", js.clone());
        addresses.insert("/_fundamental/app_bg.wasm", wasm.clone());
        let page = PageContext::new(Vec::new(), "req").with_asset_addresses(Arc::new(addresses));
        let tags = page.hydration_script("/_fundamental/app.js");
        let js = format!("/_fundamental/app.js?v={js}");
        let wasm = format!("/_fundamental/app_bg.wasm?v={wasm}");
        for linked in [
            format!("<link rel=\"modulepreload\" href=\"{js}\">"),
            format!("<link rel=\"preload\" href=\"{wasm}\""),
            format!("data-bundle=\"{js}\""),
            format!("data-wasm=\"{wasm}\""),
        ] {
            assert!(tags.contains(&linked), "no `{linked}` in: {tags}");
        }
        let unversioned =
            PageContext::new(Vec::new(), "req").hydration_script("/_fundamental/app.js");
        assert_eq!(unversioned, hydration_script("/_fundamental/app.js"));
    }

    #[test]
    fn a_page_on_a_live_reloading_router_loads_the_live_reload_client() {
        let live = PageContext::new(Vec::new(), "req")
            .with_live_reload(true)
            .hydration_script("/_fundamental/app.js");
        assert!(
            live.ends_with("<script type=\"module\" src=\"/_fundamental/live.js\"></script>"),
            "tags: {live}"
        );
        let deployed = PageContext::new(Vec::new(), "req").hydration_script("/_fundamental/app.js");
        assert!(!deployed.contains("live.js"), "tags: {deployed}");
    }

    #[test]
    fn derives_the_bundle_name_from_a_nested_url() {
        let tags = hydration_script("/assets/site.js");
        assert!(
            tags.contains("data-bundle=\"/assets/site.js\""),
            "tags: {tags}"
        );
        assert!(
            tags.contains("data-wasm=\"/assets/site_bg.wasm\""),
            "tags: {tags}"
        );
    }

    #[test]
    fn the_page_declares_its_own_hydration_rather_than_leaving_it_to_be_inferred() {
        let tags = hydration_script("/_fundamental/app.js");
        for declared in [
            "data-hydration",
            "pending",
            "complete",
            "failed",
            "fundamental:hydration",
        ] {
            assert!(tags.contains(declared), "no `{declared}` in: {tags}");
        }
    }

    #[test]
    fn the_bootstrap_never_waits_for_the_browser_to_have_spare_time() {
        // `requestIdleCallback` has no bound on a busy machine, so `complete` would have none either.
        let tags = hydration_script("/_fundamental/app.js");
        // The word appears in the bootstrap's own comment, so the assertion reads code.
        let uses: Vec<&str> = tags
            .lines()
            .filter(|line| line.contains("requestIdleCallback"))
            .filter(|line| !line.trim_start().starts_with("//"))
            .collect();
        assert!(
            uses.is_empty(),
            "the bootstrap waits for idle time: {uses:?}"
        );
    }

    #[test]
    fn a_bundle_url_cannot_break_out_of_the_markup_it_is_written_into() {
        // A `</script>` in script text could not be undone downstream, so URLs are escaped attributes.
        let tags = hydration_script("/app.js\"></script><script>alert(1)</script>");
        assert!(!tags.contains("<script>alert(1)"), "tags: {tags}");
        assert!(tags.contains("&quot;&gt;&lt;/script&gt;"), "tags: {tags}");
        assert_eq!(tags.matches("<script").count(), 1, "tags: {tags}");
    }
}
