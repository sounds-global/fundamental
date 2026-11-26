//! A `queue`, `stream` or `consumer` name: a literal, or a typed handle whose `NAME` is checked at compile time.

use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote};
use syn::parse::{Parse, ParseStream};
use syn::spanned::Spanned;
use syn::{LitStr, Path};

#[derive(Debug)]
pub(crate) enum NameArg {
    Name(LitStr),
    Handle(Path),
}

impl Parse for NameArg {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        if input.peek(LitStr) {
            Ok(Self::Name(input.parse()?))
        } else {
            Ok(Self::Handle(input.parse()?))
        }
    }
}

impl NameArg {
    /// The name expression, resolving a handle through `entity_trait` to its `NAME`.
    pub(crate) fn name_expr(&self, nats: &TokenStream, entity_trait: &str) -> TokenStream {
        match self {
            Self::Name(literal) => quote!(#literal),
            Self::Handle(path) => {
                let entity_trait = format_ident!("{entity_trait}");
                quote!(<#path as #nats::#entity_trait>::NAME)
            }
        }
    }

    pub(crate) fn span(&self) -> Span {
        match self {
            Self::Name(literal) => literal.span(),
            Self::Handle(path) => path.span(),
        }
    }
}
