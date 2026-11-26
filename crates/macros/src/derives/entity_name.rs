//! The shared expansion of `Queue`, `Stream` and `Consumer`: a unit struct's `const NAME`.

use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote};
use syn::{Data, DeriveInput, Fields, LitStr};

use crate::derives::case::to_snake_case;
use crate::derives::nats_path;

/// `impl nats::{entity} for T { const NAME = … }`; `attr`'s `name = "…"` overrides the `snake_case` default.
pub(crate) fn expand(input: &DeriveInput, entity: &str, attr: &str) -> syn::Result<TokenStream> {
    let name = &input.ident;

    if !input.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &input.generics,
            format!("{entity} cannot be derived for a generic type; it is a single fixed marker"),
        ));
    }

    match &input.data {
        Data::Struct(data) => {
            if !matches!(data.fields, Fields::Unit) {
                return Err(syn::Error::new_spanned(
                    name,
                    format!(
                        "{entity} can only be derived for a unit struct (e.g. `struct Orders;`); \
                         the marker carries no data"
                    ),
                ));
            }
        }
        Data::Enum(_) => {
            return Err(syn::Error::new_spanned(
                name,
                format!("{entity} can only be derived for a unit struct, not an enum"),
            ));
        }
        Data::Union(_) => {
            return Err(syn::Error::new_spanned(
                name,
                format!("{entity} can only be derived for a unit struct, not a union"),
            ));
        }
    }

    let name_override = parse_name(input, attr)?;
    let entity_name = match &name_override {
        Some(literal) => literal.value(),
        None => to_snake_case(&name.to_string()),
    };
    let name_span = name_override
        .as_ref()
        .map_or_else(|| name.span(), LitStr::span);
    validate_entity_name(&entity_name, name_span, entity)?;

    let nats = nats_path();
    let entity = format_ident!("{entity}");

    Ok(quote! {
        impl #nats::#entity for #name {
            const NAME: &'static str = #entity_name;
        }
    })
}

/// An optional `#[<attr>(name = "…")]`, kept as a literal for its span.
fn parse_name(input: &DeriveInput, attr: &str) -> syn::Result<Option<LitStr>> {
    let mut name = None;
    for attribute in &input.attrs {
        if !attribute.path().is_ident(attr) {
            continue;
        }
        attribute.parse_nested_meta(|meta| {
            if meta.path.is_ident("name") {
                name = Some(meta.value()?.parse()?);
                Ok(())
            } else {
                Err(meta.error(format!(
                    "unknown `#[{attr}(...)]` key; the only key is `name`"
                )))
            }
        })?;
    }
    Ok(name)
}

/// Refuses an entity name that is not one flat token of letters, digits, `-` and `_`.
pub(crate) fn validate_entity_name(name: &str, span: Span, entity: &str) -> syn::Result<()> {
    if name.is_empty() {
        return Err(syn::Error::new(
            span,
            format!("a {entity} name cannot be empty"),
        ));
    }
    for (i, c) in name.char_indices() {
        if !(c.is_alphanumeric() || c == '-' || c == '_') {
            let hint = if c == '.' {
                " — a name is a single token, not a dotted subject; use '_' instead of '.'"
            } else {
                " — a name may contain only letters, digits, '-' and '_'"
            };
            return Err(syn::Error::new(
                span,
                format!("invalid {entity} name \"{name}\": '{c}' at byte {i} is not allowed{hint}"),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::validate_entity_name;
    use proc_macro2::Span;

    fn check(name: &str) -> bool {
        validate_entity_name(name, Span::call_site(), "Queue").is_ok()
    }

    #[test]
    fn accepts_flat_tokens() {
        for ok in [
            "orders",
            "activity_workers",
            "ORDERS",
            "analytics-v2",
            "layer5_erase",
        ] {
            assert!(check(ok), "{ok}");
        }
    }

    #[test]
    fn rejects_dotted_and_wildcard_names() {
        for bad in [
            "layer5.erase",
            "",
            "a.b.c",
            "with space",
            "star*",
            "gt>",
            "a/b",
        ] {
            assert!(!check(bad), "{bad}");
        }
    }
}
