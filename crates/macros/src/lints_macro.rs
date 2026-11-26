//! `lints!()` — a project's migration lint levels, compiled in from its `fun.toml`.

use std::path::{Path, PathBuf};

use proc_macro2::{Span, TokenStream};
use quote::quote;

use crate::derives::db_path;

const FUN_TOML: &str = "fun.toml";

pub(crate) fn lints(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let expanded = if input.is_empty() {
        std::env::var("CARGO_MANIFEST_DIR")
            .map_err(|_| syn::Error::new(Span::call_site(), "cargo sets CARGO_MANIFEST_DIR"))
            .and_then(|dir| expand(Path::new(&dir), &db_path()))
    } else {
        Err(syn::Error::new(
            Span::call_site(),
            "`lints!()` takes nothing: it reads the nearest fun.toml at or above the crate",
        ))
    };
    expanded
        .unwrap_or_else(|error| error.to_compile_error())
        .into()
}

/// `Lints::from_toml` of the nearest `fun.toml`, included so an edit rebuilds; the defaults when none.
fn expand(crate_dir: &Path, db: &TokenStream) -> syn::Result<TokenStream> {
    let Some(settings) = nearest(crate_dir) else {
        return Ok(quote! {
            ::core::result::Result::<
                #db::migration::Lints,
                #db::migration::LintsError,
            >::Ok(#db::migration::Lints::default())
        });
    };
    let settings = settings.to_str().ok_or_else(|| {
        syn::Error::new(
            Span::call_site(),
            format!(
                "the path of {} is not UTF-8, so it cannot be included",
                settings.display()
            ),
        )
    })?;
    Ok(quote! {
        #db::migration::Lints::from_toml(::core::include_str!(#settings))
    })
}

/// The `fun.toml` at or above `dir`, as the settings loader finds it.
fn nearest(dir: &Path) -> Option<PathBuf> {
    dir.ancestors()
        .map(|ancestor| ancestor.join(FUN_TOML))
        .find(|candidate| candidate.symlink_metadata().is_ok())
}

#[cfg(test)]
mod tests {
    use quote::quote;

    use super::{expand, nearest};

    #[test]
    fn the_nearest_fun_toml_at_or_above_the_crate_is_included() {
        let project = tempfile::tempdir().expect("a directory");
        let crate_dir = project.path().join("crates/orders");
        std::fs::create_dir_all(&crate_dir).expect("made");
        std::fs::write(project.path().join("fun.toml"), "").expect("written");

        assert_eq!(nearest(&crate_dir), Some(project.path().join("fun.toml")));
        let expanded = expand(&crate_dir, &quote!(::fundamental_db))
            .expect("expanded")
            .to_string();
        assert!(expanded.contains("Lints :: from_toml"), "{expanded}");
        assert!(expanded.contains("include_str"), "{expanded}");
    }

    #[test]
    fn a_crate_with_no_fun_toml_above_it_gets_the_defaults() {
        let alone = tempfile::tempdir().expect("a directory");
        let crate_dir = alone.path().join("orders");
        std::fs::create_dir_all(&crate_dir).expect("made");

        // A temporary directory may sit under a developer's own `fun.toml`.
        if nearest(&crate_dir).is_none() {
            let expanded = expand(&crate_dir, &quote!(::fundamental_db))
                .expect("expanded")
                .to_string();
            assert!(expanded.contains("Lints :: default"), "{expanded}");
            assert!(!expanded.contains("include_str"), "{expanded}");
        }
    }
}
