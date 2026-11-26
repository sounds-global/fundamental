//! `#[derive(Schema)]`: a type's structural shape as a `SchemaNode`, honouring serde's renames and skips.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{Data, DeriveInput, Fields, LitStr, Type, parse_macro_input, parse_quote};

use crate::derives::case::to_snake_case;
use crate::derives::schema_path;

pub fn derive(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match build(&input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

fn build(input: &DeriveInput) -> syn::Result<TokenStream2> {
    let schema = schema_path();
    let name = &input.ident;
    let container_rename_all = rename_all_of(&input.attrs)?;

    let body = match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(_) => {
                object_body(&data.fields, container_rename_all.as_deref(), &schema)?
            }
            Fields::Unnamed(fields) if fields.unnamed.len() == 1 => {
                let inner = &fields.unnamed[0].ty;
                quote!(<#inner as #schema::Schema>::schema())
            }
            Fields::Unnamed(_) | Fields::Unit => {
                return Err(syn::Error::new_spanned(
                    name,
                    "Schema supports a struct with named fields or a single-field newtype; \
                     describe a multi-field tuple or unit struct as a named-field struct instead",
                ));
            }
        },
        Data::Enum(data) => enum_body(name, data, container_rename_all.as_deref())?,
        Data::Union(_) => {
            return Err(syn::Error::new_spanned(
                name,
                "Schema cannot be derived for a union",
            ));
        }
    };

    let mut generics = input.generics.clone();
    for type_param in generics.type_params_mut() {
        type_param.bounds.push(parse_quote!(#schema::Schema));
    }
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    Ok(quote! {
        impl #impl_generics #schema::Schema for #name #ty_generics #where_clause {
            fn schema() -> #schema::SchemaNode {
                #body
            }
        }
    })
}

fn object_body(
    fields: &Fields,
    rename_all: Option<&str>,
    schema: &TokenStream2,
) -> syn::Result<TokenStream2> {
    let mut properties = Vec::new();
    let mut required = Vec::new();

    for field in fields {
        if field_is_skipped(&field.attrs)? {
            continue;
        }
        let ident = field.ident.as_ref().expect("named fields have identifiers");
        let wire_name = match field_rename(&field.attrs)? {
            Some(name) => name,
            None => apply_rename_all(&ident.to_string(), rename_all),
        };
        let ty = &field.ty;
        properties.push(quote!((#wire_name.to_string(), <#ty as #schema::Schema>::schema())));
        if !is_option(ty) {
            required.push(quote!(#wire_name.to_string()));
        }
    }

    Ok(quote! {
        #schema::SchemaNode::Object {
            properties: ::std::vec![#(#properties),*],
            required: ::std::vec![#(#required),*],
        }
    })
}

fn enum_body(
    name: &syn::Ident,
    data: &syn::DataEnum,
    rename_all: Option<&str>,
) -> syn::Result<TokenStream2> {
    let schema = schema_path();
    let mut values = Vec::new();
    for variant in &data.variants {
        // A variant serde never writes is no value of the wire form.
        if field_is_skipped(&variant.attrs)? {
            continue;
        }
        if !matches!(variant.fields, Fields::Unit) {
            return Err(syn::Error::new_spanned(
                name,
                "Schema supports a fieldless enum only; a data-carrying enum has no single \
                 string shape — model it as a struct or split the variants",
            ));
        }
        let wire = match variant_rename(&variant.attrs)? {
            Some(name) => name,
            None => apply_rename_all(&variant.ident.to_string(), rename_all),
        };
        values.push(quote!(#wire.to_string()));
    }
    Ok(quote! {
        #schema::SchemaNode::Enum { values: ::std::vec![#(#values),*] }
    })
}

fn is_option(ty: &Type) -> bool {
    let Type::Path(path) = ty else { return false };
    path.path
        .segments
        .last()
        .is_some_and(|segment| segment.ident == "Option")
}

fn rename_all_of(attrs: &[syn::Attribute]) -> syn::Result<Option<String>> {
    let mut found = None;
    for attr in attrs {
        if !attr.path().is_ident("serde") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("rename_all") {
                let value: LitStr = meta.value()?.parse()?;
                found = Some(value.value());
            } else {
                let _ = meta.value().and_then(|v| v.parse::<syn::Expr>());
            }
            Ok(())
        })?;
    }
    Ok(found)
}

fn field_rename(attrs: &[syn::Attribute]) -> syn::Result<Option<String>> {
    serde_string_key(attrs, "rename")
}

fn variant_rename(attrs: &[syn::Attribute]) -> syn::Result<Option<String>> {
    serde_string_key(attrs, "rename")
}

fn field_is_skipped(attrs: &[syn::Attribute]) -> syn::Result<bool> {
    let mut skipped = false;
    for attr in attrs {
        if !attr.path().is_ident("serde") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("skip") || meta.path.is_ident("skip_serializing") {
                skipped = true;
            } else {
                let _ = meta.value().and_then(|v| v.parse::<syn::Expr>());
            }
            Ok(())
        })?;
    }
    Ok(skipped)
}

fn serde_string_key(attrs: &[syn::Attribute], key: &str) -> syn::Result<Option<String>> {
    let mut found = None;
    for attr in attrs {
        if !attr.path().is_ident("serde") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident(key) {
                let value: LitStr = meta.value()?.parse()?;
                found = Some(value.value());
            } else {
                let _ = meta.value().and_then(|v| v.parse::<syn::Expr>());
            }
            Ok(())
        })?;
    }
    Ok(found)
}

/// Applies a serde `rename_all` rule; an unknown rule leaves the name unchanged.
fn apply_rename_all(name: &str, rule: Option<&str>) -> String {
    match rule {
        Some("snake_case") => to_snake_case(name),
        Some("lowercase") => name.to_ascii_lowercase(),
        Some("UPPERCASE") => name.to_ascii_uppercase(),
        Some("SCREAMING_SNAKE_CASE") => to_snake_case(name).to_ascii_uppercase(),
        Some("kebab-case") => to_snake_case(name).replace('_', "-"),
        Some("SCREAMING-KEBAB-CASE") => to_snake_case(name).to_ascii_uppercase().replace('_', "-"),
        Some("camelCase") => to_camel_case(name, false),
        Some("PascalCase") => to_camel_case(name, true),
        _ => name.to_string(),
    }
}

fn to_camel_case(name: &str, upper_first: bool) -> String {
    let mut out = String::with_capacity(name.len());
    let mut upper_next = upper_first;
    for ch in to_snake_case(name).chars() {
        if ch == '_' {
            upper_next = true;
        } else if upper_next {
            out.push(ch.to_ascii_uppercase());
            upper_next = false;
        } else {
            out.push(ch);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rename_all_camel() {
        assert_eq!(apply_rename_all("user_id", Some("camelCase")), "userId");
        assert_eq!(apply_rename_all("UserId", Some("camelCase")), "userId");
    }

    #[test]
    fn rename_all_screaming() {
        assert_eq!(
            apply_rename_all("user_id", Some("SCREAMING_SNAKE_CASE")),
            "USER_ID"
        );
    }

    #[test]
    fn rename_all_kebab() {
        assert_eq!(apply_rename_all("UserId", Some("kebab-case")), "user-id");
    }

    #[test]
    fn option_is_detected() {
        let ty: Type = parse_quote!(Option<String>);
        assert!(is_option(&ty));
        let ty: Type = parse_quote!(String);
        assert!(!is_option(&ty));
    }
}
