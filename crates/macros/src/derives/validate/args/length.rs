//! Parser for `length(min = N, max = M)` validation arguments.

use syn::{
    Expr, Lit, Meta, Token,
    parse::{Parse, ParseStream},
    punctuated::Punctuated,
};

pub struct LengthArgs {
    pub min: Option<usize>,
    pub max: Option<usize>,
}

impl Parse for LengthArgs {
    #[expect(clippy::excessive_nesting, reason = "readability baseline")]
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut min = None;
        let mut max = None;

        let args = Punctuated::<Meta, Token![,]>::parse_terminated(input)?;
        for arg in args {
            if let Meta::NameValue(nv) = arg {
                let ident = nv.path.get_ident().map(|i| i.to_string());
                let value = extract_usize(&nv.value)?;

                match ident.as_deref() {
                    Some("min") => min = Some(value),
                    Some("max") => max = Some(value),
                    _ => {}
                }
            }
        }

        Ok(LengthArgs { min, max })
    }
}

fn extract_usize(expr: &Expr) -> syn::Result<usize> {
    match expr {
        Expr::Lit(lit) => match &lit.lit {
            Lit::Int(i) => i.base10_parse(),
            _ => Err(syn::Error::new_spanned(expr, "Expected integer literal")),
        },
        _ => Err(syn::Error::new_spanned(expr, "Expected integer literal")),
    }
}
