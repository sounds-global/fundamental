//! Parser for `one_of("a", "b", ...)` validation arguments.

use syn::{
    Lit, Token,
    parse::{Parse, ParseStream},
    punctuated::Punctuated,
};

pub struct OneOfArgs {
    pub values: Vec<String>,
}

impl Parse for OneOfArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let args = Punctuated::<Lit, Token![,]>::parse_terminated(input)?;
        let values = args
            .into_iter()
            .filter_map(|lit| {
                if let Lit::Str(s) = lit {
                    Some(s.value())
                } else {
                    None
                }
            })
            .collect();

        Ok(OneOfArgs { values })
    }
}
