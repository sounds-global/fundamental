//! `embed_canonical!`: canonical bytes a build refuses to link unless they are what they claim;
//! `#[derive(CanonicalJson)]`: a struct whose one RFC 8785 encoding is written without an error path.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod canonical_json;
mod embed_request;
mod expand;

use proc_macro::TokenStream;
use syn::{DeriveInput, parse_macro_input};

/// Embed a canonical document, checked at compile time.
///
/// ```
/// const DOCUMENT: fundamental_canonical::EmbeddedCanonical = fundamental_canonical_macros::embed_canonical!(
///     path = "tests/fixtures/canonical.json",
///     sha256 = "sha256-WziFmoOwqtjRfGmjlkpPDNWibcXL4QD5B54FddBo1Rc=",
/// );
/// assert!(DOCUMENT.verify().is_ok());
/// ```
///
/// The same call over a file one byte different does not build:
///
/// ```compile_fail
/// const DOCUMENT: fundamental_canonical::EmbeddedCanonical = fundamental_canonical_macros::embed_canonical!(
///     path = "tests/fixtures/perturbed.json",
///     sha256 = "sha256-WziFmoOwqtjRfGmjlkpPDNWibcXL4QD5B54FddBo1Rc=",
/// );
/// ```
///
/// The path is relative to the invoking crate's manifest directory.
#[proc_macro]
pub fn embed_canonical(input: TokenStream) -> TokenStream {
    let request = parse_macro_input!(input as embed_request::EmbedRequest);
    match expand::expand(&request) {
        Ok(tokens) => tokens.into(),
        Err(message) => syn::Error::new(request.span, message)
            .to_compile_error()
            .into(),
    }
}

/// Derive `CanonicalJson`: the struct's one RFC 8785 encoding, with no error path.
///
/// ```
/// use fundamental_canonical::CanonicalJson;
///
/// #[derive(fundamental_canonical_macros::CanonicalJson)]
/// struct Event<'a> {
///     t: &'a str,
///     #[canonical(rename = "by")]
///     submitter: &'a str,
///     obj: Option<&'a str>,
/// }
///
/// let event = Event { t: "grant", submitter: "https://a.example\",obj\":\"x", obj: None };
/// assert_eq!(
///     event.to_canonical_json(),
///     br#"{"by":"https://a.example\",obj\":\"x","t":"grant"}"#,
/// );
/// ```
///
/// A float has no canonical encoding in this profile, so a struct holding one does not build:
///
/// ```compile_fail
/// #[derive(fundamental_canonical_macros::CanonicalJson)]
/// struct Score { value: f32 }
/// ```
///
/// Its twin, carrying the float's exact bits, does:
///
/// ```
/// #[derive(fundamental_canonical_macros::CanonicalJson)]
/// struct Score { value_bits: u32 }
/// ```
///
/// Nor does a struct naming one member twice:
///
/// ```compile_fail
/// #[derive(fundamental_canonical_macros::CanonicalJson)]
/// struct Twice {
///     a: u8,
///     #[canonical(rename = "a")]
///     b: u8,
/// }
/// ```
#[proc_macro_derive(CanonicalJson, attributes(canonical))]
pub fn derive_canonical_json(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    canonical_json::expand(&input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}
