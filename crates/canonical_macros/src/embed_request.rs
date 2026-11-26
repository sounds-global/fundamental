use proc_macro2::Span;
use syn::parse::{Parse, ParseStream};
use syn::{Ident, LitStr, Token};

/// `path = "…", sha256 = "…"`, in that order, with an optional trailing comma.
pub(crate) struct EmbedRequest {
    pub(crate) path: String,
    pub(crate) sha256: String,
    pub(crate) span: Span,
}

impl Parse for EmbedRequest {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let span = input.span();
        let path = named(input, "path")?;
        input.parse::<Token![,]>()?;
        let sha256 = named(input, "sha256")?;
        if input.peek(Token![,]) {
            input.parse::<Token![,]>()?;
        }
        Ok(Self { path, sha256, span })
    }
}

fn named(input: ParseStream<'_>, expected: &str) -> syn::Result<String> {
    let name: Ident = input.parse()?;
    if name != expected {
        return Err(syn::Error::new(
            name.span(),
            format!("expected `{expected} = \"…\"`"),
        ));
    }
    input.parse::<Token![=]>()?;
    Ok(input.parse::<LitStr>()?.value())
}
