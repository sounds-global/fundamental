//! File-content templates for a web app's islands crate, `apps/<app>/islands/`, the wasm bundle.

use crate::helpers::DomainCrate;

/// The islands crate's `Cargo.toml`; the browser build sits behind `hydrate`, apart from `ssr`.
pub(crate) fn islands_cargo_toml(app_crate: &str, domain: &DomainCrate) -> String {
    let domain = domain.dependency(3, &[]);
    format!(
        "[package]\n\
         name = \"{app_crate}_islands\"\n\
         version = \"0.1.0\"\n\
         edition = \"2024\"\n\n\
         [lints]\n\
         workspace = true\n\n\
         [dependencies]\n\
         # The crates whose `#[island]`s the browser hydrates.\n\
         {domain}\n\
         fundamental = {{ workspace = true, optional = true }}\n\
         leptos = {{ version = \"0.8\", default-features = false, features = [\"islands\"], optional = true }}\n\
         wasm-bindgen = {{ version = \"0.2\", optional = true }}\n\n\
         [features]\n\
         # The browser build: `fun build` and `fun dev` compile this crate to wasm with it.\n\
         hydrate = [\"fundamental/web-hydrate\", \"dep:leptos\", \"leptos/hydrate\", \"dep:wasm-bindgen\"]\n"
    )
}

/// The islands crate's `src/lib.rs`: the domain's islands and the `hydrate` entry.
pub(crate) fn islands_lib_rs(app: &str, app_crate: &str) -> String {
    format!(
        "//! `{app}` islands — the browser (wasm) entry.\n\
         //!\n\
         //! Served as `/_fundamental/{app_crate}.js`; an `#[island]` lives in a crate this one uses.\n\n\
         // Links the domain's islands into the bundle.\n\
         use domain as _;\n\n\
         /// Sets up the hydration context; Leptos's island bootstrap calls it.\n\
         #[cfg(feature = \"hydrate\")]\n\
         #[wasm_bindgen::prelude::wasm_bindgen]\n\
         pub fn hydrate() {{\n\
         \x20   fundamental::web::hydrate();\n\
         }}\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_islands_crate_is_named_after_the_app_and_builds_for_the_browser_on_request() {
        let toml = islands_cargo_toml("my_site", &DomainCrate::scaffolded());
        assert!(toml.contains("name = \"my_site_islands\""), "{toml}");
        assert!(
            toml.contains("domain = { path = \"../../../domain\" }"),
            "{toml}"
        );
        assert!(
            toml.contains(
                "hydrate = [\"fundamental/web-hydrate\", \"dep:leptos\", \"leptos/hydrate\""
            ),
            "{toml}"
        );
        assert!(toml.contains("optional = true"), "{toml}");
    }

    #[test]
    fn the_islands_entry_links_the_domain_and_exports_hydrate() {
        let lib = islands_lib_rs("my-site", "my_site");
        syn::parse_file(&lib).unwrap_or_else(|e| panic!("invalid lib.rs: {e}\n{lib}"));
        assert!(lib.contains("use domain as _;"), "{lib}");
        assert!(lib.contains("pub fn hydrate()"), "{lib}");
        assert!(lib.contains("/_fundamental/my_site.js"), "{lib}");
    }
}
