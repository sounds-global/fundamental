//! `#[derive(Identifier)]`: strongly-typed newtype identifiers, with backend bindings for the backends the consumer depends on.

use proc_macro::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Fields, parse_macro_input};

use crate::derives::{backend_impl_target, core_path};

pub(crate) fn derive(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match expand(&input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

#[expect(
    clippy::too_many_lines,
    clippy::excessive_nesting,
    reason = "readability baseline"
)]
fn expand(input: &DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    let name = &input.ident;

    let mut gen_display = true;
    let mut gen_sql = true;
    let mut gen_cql = true;
    let mut generate: Option<String> = None;
    let mut handle: Option<syn::LitStr> = None;

    for attr in &input.attrs {
        if !attr.path().is_ident("identifier") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("no_display") {
                gen_display = false;
            } else if meta.path.is_ident("no_sql") {
                gen_sql = false;
            } else if meta.path.is_ident("no_cql") {
                gen_cql = false;
            } else if meta.path.is_ident("generate") {
                let value: syn::LitStr = meta.value()?.parse()?;
                let kind = value.value();
                if kind != "uuid_v4" && kind != "uuid_v7" {
                    return Err(meta.error(
                        "`generate` must be \"uuid_v4\" or \"uuid_v7\" (the inner type must be `uuid::Uuid`)",
                    ));
                }
                generate = Some(kind);
            } else if meta.path.is_ident("handle") {
                let value: syn::LitStr = meta.value()?.parse()?;
                if !is_type_id_prefix(&value.value()) || value.value().is_empty() {
                    return Err(syn::Error::new_spanned(
                        &value,
                        "`handle` must be a TypeID prefix: 1–63 characters of `a-z` and `_`, \
                         starting and ending with a letter",
                    ));
                }
                handle = Some(value);
            } else {
                return Err(meta.error(
                    "unknown `#[identifier(...)]` attribute; valid: generate = \"uuid_v4\"|\"uuid_v7\", handle = \"prefix\", no_display, no_sql, no_cql",
                ));
            }
            Ok(())
        })?;
    }

    let inner_ty = match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Unnamed(fields) if fields.unnamed.len() == 1 => &fields.unnamed[0].ty,
            _ => {
                return Err(syn::Error::new_spanned(
                    name,
                    "Identifier can only be derived for a tuple struct with exactly one field, e.g. `struct UserId(uuid::Uuid);`",
                ));
            }
        },
        _ => {
            return Err(syn::Error::new_spanned(
                name,
                "Identifier can only be derived for structs",
            ));
        }
    };

    let core = core_path();

    let identifier_impl = quote! {
        impl #core::model::Identifier for #name {
            type Inner = #inner_ty;
            fn from_inner(inner: Self::Inner) -> Self { Self(inner) }
            fn as_inner(&self) -> &Self::Inner { &self.0 }
            fn into_inner(self) -> Self::Inner { self.0 }
        }

        impl ::core::convert::From<#inner_ty> for #name {
            fn from(inner: #inner_ty) -> Self { Self(inner) }
        }

        impl #core::model::sql::SqlType for #name {
            const SQL: &'static str = <#inner_ty as #core::model::sql::SqlType>::SQL;
        }

        // Binds as its inner value does, so an encrypted model keyed by one seals the same bytes a raw key would.
        impl #core::model::RowBound for #name {
            fn row_part(
                &self,
                precision: #core::model::TimestampPrecision,
            ) -> ::core::option::Option<::std::vec::Vec<u8>> {
                #core::model::RowBound::row_part(&self.0, precision)
            }
        }
    };

    // A matched pair: offline storage writes a typed id column via `Display` and reads it via `FromStr`.
    let display_impl = if gen_display {
        quote! {
            impl ::core::fmt::Display for #name {
                fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                    ::core::fmt::Display::fmt(&self.0, f)
                }
            }

            impl ::core::str::FromStr for #name {
                type Err = <#inner_ty as ::core::str::FromStr>::Err;
                fn from_str(s: &str) -> ::core::result::Result<Self, Self::Err> {
                    ::core::result::Result::Ok(Self(<#inner_ty as ::core::str::FromStr>::from_str(s)?))
                }
            }
        }
    } else {
        quote! {}
    };

    let sqlx_impl = if gen_sql {
        match backend_impl_target("fundamental_sql", "sql") {
            Some((cr, cfg)) => {
                let sqlx = quote! { #cr::sqlx };
                quote! {
                    #cfg
                    const _: () = {
                        impl #sqlx::Type<#sqlx::Postgres> for #name {
                            fn type_info() -> #sqlx::postgres::PgTypeInfo {
                                <#inner_ty as #sqlx::Type<#sqlx::Postgres>>::type_info()
                            }
                            fn compatible(ty: &#sqlx::postgres::PgTypeInfo) -> bool {
                                <#inner_ty as #sqlx::Type<#sqlx::Postgres>>::compatible(ty)
                            }
                        }

                        impl<'q> #sqlx::Encode<'q, #sqlx::Postgres> for #name {
                            fn encode_by_ref(
                                &self,
                                buf: &mut #sqlx::postgres::PgArgumentBuffer,
                            ) -> ::core::result::Result<#sqlx::encode::IsNull, ::std::boxed::Box<dyn ::core::error::Error + ::core::marker::Send + ::core::marker::Sync>> {
                                <#inner_ty as #sqlx::Encode<'q, #sqlx::Postgres>>::encode_by_ref(&self.0, buf)
                            }
                        }

                        impl<'r> #sqlx::Decode<'r, #sqlx::Postgres> for #name {
                            fn decode(
                                value: #sqlx::postgres::PgValueRef<'r>,
                            ) -> ::core::result::Result<Self, ::std::boxed::Box<dyn ::core::error::Error + ::core::marker::Send + ::core::marker::Sync>> {
                                let inner = <#inner_ty as #sqlx::Decode<'r, #sqlx::Postgres>>::decode(value)?;
                                ::core::result::Result::Ok(Self(inner))
                            }
                        }
                    };
                }
            }
            None => quote! {},
        }
    } else {
        quote! {}
    };

    let cql_impl = if gen_cql {
        match backend_impl_target("fundamental_cql", "cql") {
            Some((sc, cfg)) => {
                // The driver re-export keeps the vendor's name: it is the `scylla` crate.
                let driver = quote! { #sc::scylla };
                quote! {
                    #cfg
                    const _: () = {
                        impl #driver::serialize::value::SerializeValue for #name {
                            fn serialize<'b>(
                                &self,
                                typ: &#driver::frame::response::result::ColumnType<'_>,
                                writer: #driver::serialize::writers::CellWriter<'b>,
                            ) -> ::core::result::Result<
                                #driver::serialize::writers::WrittenCellProof<'b>,
                                #driver::serialize::SerializationError,
                            > {
                                <#inner_ty as #driver::serialize::value::SerializeValue>::serialize(&self.0, typ, writer)
                            }
                        }

                        impl #sc::FromCqlValue for #name {
                            fn from_cql_value(
                                value: &#driver::value::CqlValue,
                            ) -> ::core::option::Option<Self> {
                                <#inner_ty as #sc::FromCqlValue>::from_cql_value(value).map(Self)
                            }
                        }
                    };
                }
            }
            None => quote! {},
        }
    } else {
        quote! {}
    };

    let generated_impl = match generate.as_deref() {
        Some(kind) => {
            let ctor = if kind == "uuid_v7" {
                quote! { now_v7 }
            } else {
                quote! { new_v4 }
            };
            quote! {
                impl #core::model::GeneratedId for #name {
                    fn generate() -> Self {
                        Self(<#inner_ty>::#ctor())
                    }
                }
            }
        }
        None => quote! {},
    };

    // The prefix was checked above and again by the `const` constructor, so the panic arm is unreachable.
    let handle_impl = match &handle {
        Some(prefix) => quote! {
            impl #core::model::HasHandle for #name {
                const HANDLE_PREFIX: #core::model::HandlePrefix =
                    match #core::model::HandlePrefix::new(#prefix) {
                        ::core::option::Option::Some(prefix) => prefix,
                        ::core::option::Option::None => ::core::panic!("not a TypeID prefix"),
                    };
            }
        },
        None => quote! {},
    };

    Ok(quote! {
        #identifier_impl
        #handle_impl
        #display_impl
        #sqlx_impl
        #cql_impl
        #generated_impl
    })
}

/// Whether `text` matches TypeID's prefix grammar, `^([a-z]([a-z_]{0,61}[a-z])?)?$`.
fn is_type_id_prefix(text: &str) -> bool {
    let bytes = text.as_bytes();
    match (bytes.first(), bytes.last()) {
        (None, _) | (_, None) => true,
        (Some(first), Some(last)) => {
            bytes.len() <= 63
                && first.is_ascii_lowercase()
                && last.is_ascii_lowercase()
                && bytes
                    .iter()
                    .all(|byte| byte.is_ascii_lowercase() || *byte == b'_')
        }
    }
}

#[cfg(test)]
mod tests {
    use super::is_type_id_prefix;

    #[test]
    fn the_derive_refuses_what_typeid_refuses() {
        for refused in ["Team", "team1", "_team", "team_", "té", "pre.fix"] {
            assert!(!is_type_id_prefix(refused), "{refused}");
        }
        for accepted in ["a", "team", "build_request", "pre__fix"] {
            assert!(is_type_id_prefix(accepted), "{accepted}");
        }
        assert!(!is_type_id_prefix(&"a".repeat(64)));
    }
}
