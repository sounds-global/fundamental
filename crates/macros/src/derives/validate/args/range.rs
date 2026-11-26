//! Parser for `range(min = ..., max = ...)` validation arguments.
//!
//! Bounds stay token strings so the generated code reproduces any expression.

use quote::ToTokens;
use syn::{
    Meta, Token,
    parse::{Parse, ParseStream},
    punctuated::Punctuated,
};

pub struct RangeArgs {
    pub min: Option<String>,
    pub max: Option<String>,
}

impl Parse for RangeArgs {
    #[expect(clippy::excessive_nesting, reason = "readability baseline")]
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut min = None;
        let mut max = None;

        let args = Punctuated::<Meta, Token![,]>::parse_terminated(input)?;
        for arg in args {
            if let Meta::NameValue(nv) = arg {
                let ident = nv.path.get_ident().map(|i| i.to_string());
                let value = nv.value.to_token_stream().to_string();

                match ident.as_deref() {
                    Some("min") => min = Some(value),
                    Some("max") => max = Some(value),
                    _ => {}
                }
            }
        }

        Ok(RangeArgs { min, max })
    }
}
