//! `#[derive(PublicSegment)]`: a path segment from a fieldless enum, so it can never carry a record id.

use std::collections::BTreeMap;

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{Data, DeriveInput, Fields, LitStr, parse_macro_input};

use crate::derives::case::to_snake_case;
use crate::derives::web_path;

pub(crate) fn derive(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    expand(&input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

fn expand(input: &DeriveInput) -> syn::Result<TokenStream2> {
    let name = &input.ident;
    let refusal = "PublicSegment is derived for a fieldless enum: a public segment is one of a \
                   fixed set of words, so it can never carry a record's id";
    let Data::Enum(data) = &input.data else {
        return Err(syn::Error::new_spanned(input, refusal));
    };
    if !input.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(&input.generics, refusal));
    }
    if data.variants.is_empty() {
        return Err(syn::Error::new_spanned(
            input,
            "PublicSegment needs at least one variant: a segment with no words matches nothing",
        ));
    }

    let mut words: BTreeMap<String, syn::Ident> = BTreeMap::new();
    let mut parse_arms = Vec::new();
    let mut segment_arms = Vec::new();
    for variant in &data.variants {
        if !matches!(variant.fields, Fields::Unit) {
            return Err(syn::Error::new_spanned(variant, refusal));
        }
        let ident = &variant.ident;
        let word = segment_word(variant)?;
        if let Some(earlier) = words.insert(word.clone(), ident.clone()) {
            return Err(syn::Error::new_spanned(
                variant,
                format!("`{earlier}` and `{ident}` are both the segment `{word}`"),
            ));
        }
        parse_arms.push(quote!(#word => ::core::option::Option::Some(Self::#ident)));
        segment_arms.push(quote!(Self::#ident => #word));
    }

    let web = web_path();
    Ok(quote! {
        impl #web::PublicSegment for #name {
            fn parse(segment: &str) -> ::core::option::Option<Self> {
                match segment {
                    #( #parse_arms, )*
                    _ => ::core::option::Option::None,
                }
            }

            fn segment(&self) -> &'static str {
                match *self {
                    #( #segment_arms, )*
                }
            }

            #[doc(hidden)]
            fn __derived_for_a_fieldless_enum() {}
        }
    })
}

fn segment_word(variant: &syn::Variant) -> syn::Result<String> {
    let Some(attr) = variant.attrs.iter().find(|a| a.path().is_ident("segment")) else {
        return Ok(to_snake_case(&variant.ident.to_string()).replace('_', "-"));
    };
    let word: LitStr = attr.parse_args()?;
    let value = word.value();
    let valid = !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if !valid {
        return Err(syn::Error::new_spanned(
            word,
            "a segment is lower-case letters, digits and `-`, so it reads the same in every \
             address and needs no escaping",
        ));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::expand;
    use syn::{DeriveInput, parse_quote};

    fn refusal(input: DeriveInput) -> String {
        expand(&input)
            .err()
            .map(|error| error.to_string())
            .unwrap_or_default()
    }

    #[test]
    fn a_fieldless_enum_derives_its_words() {
        let input: DeriveInput = parse_quote! {
            enum Step {
                Company,
                PaymentDetails,
                #[segment("klaar")]
                Done,
            }
        };
        let tokens = expand(&input).expect("derives").to_string();
        for word in ["\"company\"", "\"payment-details\"", "\"klaar\""] {
            assert!(tokens.contains(word), "{word}: {tokens}");
        }
    }

    #[test]
    fn a_shape_that_could_carry_an_id_is_refused() {
        let newtype: DeriveInput = parse_quote! { struct ProjectId(String); };
        assert!(refusal(newtype).contains("fieldless enum"));
        let tuple_variant: DeriveInput = parse_quote! { enum Step { One(u64) } };
        assert!(refusal(tuple_variant).contains("fieldless enum"));
    }

    #[test]
    fn two_variants_with_one_word_are_refused() {
        let input: DeriveInput = parse_quote! {
            enum Step {
                Done,
                #[segment("done")]
                Finished,
            }
        };
        assert!(refusal(input).contains("both the segment `done`"));
    }

    #[test]
    fn a_segment_outside_the_safe_alphabet_is_refused() {
        let input: DeriveInput = parse_quote! {
            enum Step {
                #[segment("a/b")]
                Slash,
            }
        };
        assert!(refusal(input).contains("lower-case letters"));
    }
}
