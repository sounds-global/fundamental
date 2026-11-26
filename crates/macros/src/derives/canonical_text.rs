//! `#[derive(CanonicalText)]`: every codec of a one-spelling text type, each decoding through one parse function.

use proc_macro::TokenStream;
use quote::quote;
use syn::{DeriveInput, ExprPath, parse_macro_input};

use crate::derives::{backend_impl_target, core_path};

pub(crate) fn derive(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match expand(&input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

#[expect(clippy::too_many_lines, reason = "readability baseline")]
fn expand(input: &DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    let name = &input.ident;
    let mut parse: Option<ExprPath> = None;
    let mut admit = false;
    for attr in &input.attrs {
        if !attr.path().is_ident("canonical_text") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("parse") {
                parse = Some(meta.value()?.parse()?);
            } else if meta.path.is_ident("admit") {
                admit = true;
            } else {
                return Err(meta.error(
                    "unknown `#[canonical_text(...)]` attribute; valid: parse = path, admit",
                ));
            }
            Ok(())
        })?;
    }
    let parse = parse.ok_or_else(|| {
        syn::Error::new_spanned(
            name,
            "CanonicalText needs `#[canonical_text(parse = path)]`: the one function every decoder calls",
        )
    })?;

    let core = core_path();
    let serde = quote! { #core::__serde };
    let (impl_generics, type_generics, where_clause) = input.generics.split_for_impl();
    let de_generics = with_lifetime(input, quote!('de));
    let q_generics = with_lifetime(input, quote!('q));
    let r_generics = with_lifetime(input, quote!('r));

    let always = quote! {
        impl #impl_generics ::core::fmt::Display for #name #type_generics #where_clause {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                f.write_str(#core::model::CanonicalText::as_canonical(self))
            }
        }

        impl #impl_generics #serde::Serialize for #name #type_generics #where_clause {
            fn serialize<S: #serde::Serializer>(&self, serializer: S) -> ::core::result::Result<S::Ok, S::Error> {
                serializer.serialize_str(#core::model::CanonicalText::as_canonical(self))
            }
        }

        impl #impl_generics #core::model::RowBound for #name #type_generics #where_clause {
            fn row_part(
                &self,
                precision: #core::model::TimestampPrecision,
            ) -> ::core::option::Option<::std::vec::Vec<u8>> {
                #core::model::RowBound::row_part(#core::model::CanonicalText::as_canonical(self), precision)
            }
        }
    };

    let open_readers = if admit {
        quote! {}
    } else {
        quote! {
            impl #impl_generics ::core::str::FromStr for #name #type_generics #where_clause {
                type Err = ::std::string::String;
                fn from_str(text: &str) -> ::core::result::Result<Self, Self::Err> {
                    if text.len() > <Self as #core::model::CanonicalText>::MAX_LEN {
                        return ::core::result::Result::Err(::std::format!(
                            "longer than {} bytes",
                            <Self as #core::model::CanonicalText>::MAX_LEN
                        ));
                    }
                    #parse(text).map_err(|error| ::std::string::ToString::to_string(&error))
                }
            }

            impl #de_generics #serde::Deserialize<'de> for #name #type_generics #where_clause {
                fn deserialize<D: #serde::Deserializer<'de>>(deserializer: D) -> ::core::result::Result<Self, D::Error> {
                    let text = <::std::borrow::Cow<'de, str> as #serde::Deserialize<'de>>::deserialize(deserializer)?;
                    <Self as ::core::str::FromStr>::from_str(&text).map_err(<D::Error as #serde::de::Error>::custom)
                }
            }
        }
    };

    let sqlx_impl = match backend_impl_target("fundamental_sql", "sql") {
        Some((cr, cfg)) => {
            let sqlx = quote! { #cr::sqlx };
            quote! {
                #cfg
                const _: () = {
                    impl #impl_generics #sqlx::Type<#sqlx::Postgres> for #name #type_generics #where_clause {
                        fn type_info() -> #sqlx::postgres::PgTypeInfo {
                            <&str as #sqlx::Type<#sqlx::Postgres>>::type_info()
                        }
                        fn compatible(ty: &#sqlx::postgres::PgTypeInfo) -> bool {
                            <&str as #sqlx::Type<#sqlx::Postgres>>::compatible(ty)
                        }
                    }

                    impl #q_generics #sqlx::Encode<'q, #sqlx::Postgres> for #name #type_generics #where_clause {
                        fn encode_by_ref(
                            &self,
                            buf: &mut #sqlx::postgres::PgArgumentBuffer,
                        ) -> ::core::result::Result<#sqlx::encode::IsNull, ::std::boxed::Box<dyn ::core::error::Error + ::core::marker::Send + ::core::marker::Sync>> {
                            <&str as #sqlx::Encode<'q, #sqlx::Postgres>>::encode_by_ref(
                                &#core::model::CanonicalText::as_canonical(self),
                                buf,
                            )
                        }
                    }

                    impl #r_generics #sqlx::Decode<'r, #sqlx::Postgres> for #name #type_generics #where_clause {
                        fn decode(
                            value: #sqlx::postgres::PgValueRef<'r>,
                        ) -> ::core::result::Result<Self, ::std::boxed::Box<dyn ::core::error::Error + ::core::marker::Send + ::core::marker::Sync>> {
                            let text = <&str as #sqlx::Decode<'r, #sqlx::Postgres>>::decode(value)?;
                            if text.len() > <Self as #core::model::CanonicalText>::MAX_LEN {
                                return ::core::result::Result::Err(::std::convert::Into::into("a stored identifier is longer than its type allows"));
                            }
                            #parse(text).map_err(|error| ::std::convert::Into::into(::std::string::ToString::to_string(&error)))
                        }
                    }
                };
            }
        }
        None => quote! {},
    };

    let cql_impl = match backend_impl_target("fundamental_cql", "cql") {
        Some((sc, cfg)) => {
            let driver = quote! { #sc::scylla };
            quote! {
                #cfg
                const _: () = {
                    impl #impl_generics #driver::serialize::value::SerializeValue for #name #type_generics #where_clause {
                        fn serialize<'b>(
                            &self,
                            typ: &#driver::frame::response::result::ColumnType<'_>,
                            writer: #driver::serialize::writers::CellWriter<'b>,
                        ) -> ::core::result::Result<
                            #driver::serialize::writers::WrittenCellProof<'b>,
                            #driver::serialize::SerializationError,
                        > {
                            <str as #driver::serialize::value::SerializeValue>::serialize(
                                #core::model::CanonicalText::as_canonical(self),
                                typ,
                                writer,
                            )
                        }
                    }

                    // A refused stored text reads as absent, reported as a failed column.
                    impl #impl_generics #sc::FromCqlValue for #name #type_generics #where_clause {
                        fn from_cql_value(value: &#driver::value::CqlValue) -> ::core::option::Option<Self> {
                            let text = <::std::string::String as #sc::FromCqlValue>::from_cql_value(value)?;
                            if text.len() > <Self as #core::model::CanonicalText>::MAX_LEN {
                                return ::core::option::Option::None;
                            }
                            #parse(&text).ok()
                        }
                    }
                };
            }
        }
        None => quote! {},
    };

    Ok(quote! {
        #always
        #open_readers
        #sqlx_impl
        #cql_impl
    })
}

/// The impl generics with one more lifetime in front.
fn with_lifetime(
    input: &DeriveInput,
    lifetime: proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    let mut generics = input.generics.clone();
    generics.params.insert(0, syn::parse_quote!(#lifetime));
    let (impl_generics, _, _) = generics.split_for_impl();
    quote!(#impl_generics)
}
