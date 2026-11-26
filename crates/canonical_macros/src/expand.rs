use crate::embed_request::EmbedRequest;
use fundamental_canonical::{Limits, Profile, Sha256Digest, validate_canonical};
use proc_macro_crate::{FoundCrate, crate_name};
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use std::path::{Component, Path, PathBuf};

/// Check the file and produce the `EmbeddedCanonical` expression, or say why the build fails.
pub(crate) fn expand(request: &EmbedRequest) -> Result<TokenStream, String> {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").map_err(|_| {
        "embed_canonical! needs CARGO_MANIFEST_DIR, which cargo always sets".to_owned()
    })?;
    let file = checked(Path::new(&manifest), request)?;
    let digest = Sha256Digest::of(
        &std::fs::read(&file).map_err(|error| format!("{}: {error}", file.display()))?,
    );
    let absolute = file.display().to_string();
    let root = canonical_root();
    let bytes = digest.as_bytes().iter();
    Ok(quote! {
        {
            const EMBEDDED: &[u8] = ::core::include_bytes!(#absolute);
            #root::EmbeddedCanonical::__checked_at_build(
                EMBEDDED,
                #root::Sha256Digest::from_bytes([#(#bytes),*]),
            )
        }
    })
}

/// Check the path is inside the crate, the bytes canonical, and the digest the one written.
pub(crate) fn checked(manifest: &Path, request: &EmbedRequest) -> Result<PathBuf, String> {
    let relative = Path::new(&request.path);
    if !relative
        .components()
        .all(|component| matches!(component, Component::Normal(_)))
    {
        return Err(format!(
            "`{}` must be a path inside this crate, relative to its manifest, with no `..`",
            request.path
        ));
    }
    let expected = Sha256Digest::parse_sri(&request.sha256)
        .map_err(|_| format!("`{}` is not one canonical `sha256-…` SRI", request.sha256))?;
    let file = manifest.join(relative);
    let bytes = std::fs::read(&file).map_err(|error| format!("{}: {error}", file.display()))?;
    validate_canonical(&bytes, &Limits::DOCUMENT, Profile::Registry)
        .map_err(|error| format!("{} is not a canonical document: {error}", request.path))?;
    let actual = Sha256Digest::of(&bytes);
    if actual != expected {
        return Err(format!(
            "{} has digest {} but the call says {}: the bytes changed, or the digest was not updated with them",
            request.path,
            actual.to_sri(),
            expected.to_sri()
        ));
    }
    Ok(file)
}

pub(crate) fn canonical_root() -> TokenStream {
    match crate_name("fundamental_canonical") {
        Ok(FoundCrate::Itself) => quote!(crate),
        Ok(FoundCrate::Name(name)) => {
            let ident = format_ident!("{name}");
            quote!(::#ident)
        }
        Err(_) => match crate_name("fundamental") {
            Ok(FoundCrate::Name(name)) => {
                let ident = format_ident!("{name}");
                quote!(::#ident::canonical)
            }
            _ => quote!(::fundamental::canonical),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::checked;
    use crate::embed_request::EmbedRequest;
    use fundamental_canonical::Sha256Digest;
    use proc_macro2::Span;
    use std::path::Path;

    fn request(path: &str, sha256: String) -> EmbedRequest {
        EmbedRequest {
            path: path.to_owned(),
            sha256,
            span: Span::call_site(),
        }
    }

    fn manifest() -> &'static Path {
        Path::new(env!("CARGO_MANIFEST_DIR"))
    }

    fn sri_of(fixture: &str) -> String {
        let bytes = std::fs::read(manifest().join(fixture)).expect("the fixture exists");
        Sha256Digest::of(&bytes).to_sri()
    }

    #[test]
    fn canonical_bytes_with_their_digest_are_embedded() {
        let fixture = "tests/fixtures/canonical.json";
        assert!(checked(manifest(), &request(fixture, sri_of(fixture))).is_ok());
    }

    #[test]
    fn embed_canonical_refuses_a_perturbed_byte() {
        // The original's digest over changed bytes: an edit that forgot the digest.
        let refused = checked(
            manifest(),
            &request(
                "tests/fixtures/perturbed.json",
                sri_of("tests/fixtures/canonical.json"),
            ),
        )
        .expect_err("a changed byte");
        assert!(refused.contains("but the call says"), "{refused}");
    }

    #[test]
    fn embed_canonical_refuses_non_canonical_bytes() {
        let fixture = "tests/fixtures/pretty.json";
        let refused =
            checked(manifest(), &request(fixture, sri_of(fixture))).expect_err("whitespace");
        assert!(refused.contains("not a canonical document"), "{refused}");
    }

    #[test]
    fn embed_canonical_refuses_a_path_outside_the_crate() {
        for path in ["../elsewhere.json", "/etc/passwd"] {
            let refused = checked(
                manifest(),
                &request(path, sri_of("tests/fixtures/canonical.json")),
            )
            .expect_err("outside the crate");
            assert!(refused.contains("inside this crate"), "{refused}");
        }
    }

    #[test]
    fn embed_canonical_refuses_a_malformed_digest() {
        let refused = checked(
            manifest(),
            &request("tests/fixtures/canonical.json", "sha256-nope".to_owned()),
        )
        .expect_err("not an SRI");
        assert!(refused.contains("SRI"), "{refused}");
    }
}
