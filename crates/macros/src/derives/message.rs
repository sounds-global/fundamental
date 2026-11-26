//! `#[derive(Message)]`: a type bound to a NATS subject, `#[subject = "…"]` or `#[subject_fn = "…"]`.

use proc_macro::TokenStream;
use quote::quote;
use syn::{DeriveInput, LitStr, parse_macro_input};

use crate::derives::nats_path;

pub(crate) fn derive(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match expand(&input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

struct MessageAttrs {
    subject: Option<LitStr>,
    subject_fn: Option<LitStr>,
}

fn parse_attrs(input: &DeriveInput) -> syn::Result<MessageAttrs> {
    let mut attrs = MessageAttrs {
        subject: None,
        subject_fn: None,
    };
    for attr in &input.attrs {
        if attr.path().is_ident("subject") {
            attrs.subject = Some(lit_str(&attr.meta.require_name_value()?.value)?);
        } else if attr.path().is_ident("subject_fn") {
            attrs.subject_fn = Some(lit_str(&attr.meta.require_name_value()?.value)?);
        }
    }
    Ok(attrs)
}

/// The string literal of a name-value attribute; shared with `#[derive(MessageSet)]`.
pub(crate) fn lit_str(expr: &syn::Expr) -> syn::Result<LitStr> {
    match expr {
        syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Str(lit),
            ..
        }) => Ok(lit.clone()),
        other => Err(syn::Error::new_spanned(
            other,
            "expected a string literal, e.g. `#[subject = \"events.user.created\"]`",
        )),
    }
}

fn expand(input: &DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    let attrs = parse_attrs(input)?;
    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    let nats = nats_path();

    let body = match (&attrs.subject, &attrs.subject_fn) {
        (Some(_), Some(_)) => {
            return Err(syn::Error::new_spanned(
                name,
                "`#[derive(Message)]` accepts `#[subject = \"…\"]` or `#[subject_fn = \"…\"]`, not both",
            ));
        }
        (None, None) => {
            return Err(syn::Error::new_spanned(
                name,
                "`#[derive(Message)]` requires `#[subject = \"…\"]` or `#[subject_fn = \"…\"]`",
            ));
        }
        (Some(subject), None) => {
            validate_subject(&subject.value(), subject.span())?;
            quote! {
                const SUBJECT: &'static str = #subject;
            }
        }
        (None, Some(subject_fn)) => {
            let method = syn::Ident::new(&subject_fn.value(), subject_fn.span());
            quote! {
                const SUBJECT: &'static str = "__dynamic__";

                fn subject(&self) -> ::std::borrow::Cow<'static, str> {
                    ::std::borrow::Cow::Owned(::std::convert::Into::into(self.#method()))
                }
            }
        }
    };

    let event = match event_id_field(input)? {
        Some(field) => quote! {
            impl #impl_generics #nats::Event for #name #ty_generics #where_clause {
                fn event_id(&self) -> #nats::EventId {
                    self.#field
                }
            }
        },
        None => quote! {},
    };

    Ok(quote! {
        impl #impl_generics #nats::Message for #name #ty_generics #where_clause {
            #body
        }

        #event

        // A concrete `Publish` impl per type: a blanket one would overlap the `MessageSet` derive's.
        impl #impl_generics #nats::Publish for #name #ty_generics #where_clause {
            fn subject(&self) -> ::std::borrow::Cow<'static, str> {
                <Self as #nats::Message>::subject(self)
            }

            fn to_bytes(&self) -> #nats::NatsResult<#nats::Bytes> {
                <Self as #nats::Message>::to_bytes(self)
            }
        }
    })
}

fn event_id_field(input: &DeriveInput) -> syn::Result<Option<syn::Member>> {
    let fields = match &input.data {
        syn::Data::Struct(data) => &data.fields,
        _ => {
            if let Some(attr) = find_event_id_attr(input) {
                return Err(syn::Error::new_spanned(
                    attr,
                    "`#[event_id]` marks a field of a struct",
                ));
            }
            return Ok(None);
        }
    };
    let mut marked = fields
        .iter()
        .enumerate()
        .filter(|(_, field)| field.attrs.iter().any(|a| a.path().is_ident("event_id")));
    let Some((index, field)) = marked.next() else {
        return Ok(None);
    };
    if let Some((_, second)) = marked.next() {
        return Err(syn::Error::new_spanned(
            second,
            "an event has one identity: mark exactly one field `#[event_id]`",
        ));
    }
    Ok(Some(field.ident.clone().map_or_else(
        || syn::Member::Unnamed(syn::Index::from(index)),
        syn::Member::Named,
    )))
}

fn find_event_id_attr(input: &DeriveInput) -> Option<&syn::Attribute> {
    let syn::Data::Enum(data) = &input.data else {
        return None;
    };
    data.variants
        .iter()
        .flat_map(|variant| variant.fields.iter())
        .flat_map(|field| field.attrs.iter())
        .find(|attr| attr.path().is_ident("event_id"))
}

/// Validates a static subject as `fundamental_nats::Subject::new` does.
pub(crate) fn validate_subject(subject: &str, span: proc_macro2::Span) -> syn::Result<()> {
    let err = |msg: String| Err(syn::Error::new(span, msg));
    if subject.is_empty() {
        return err("subject cannot be empty".into());
    }
    if subject.starts_with('.') || subject.ends_with('.') {
        return err(format!(
            "invalid subject \"{subject}\": cannot start or end with a dot"
        ));
    }
    if subject.contains("..") {
        return err(format!(
            "invalid subject \"{subject}\": cannot contain consecutive dots"
        ));
    }
    if subject.contains('*') || subject.contains('>') {
        return err(format!(
            "invalid subject \"{subject}\": wildcards (* or >) are not allowed in a static subject"
        ));
    }
    for (i, c) in subject.char_indices() {
        if !c.is_alphanumeric() && c != '-' && c != '_' && c != '.' {
            return err(format!(
                "invalid subject \"{subject}\": invalid character '{c}' at byte {i}"
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{event_id_field, validate_subject};
    use proc_macro2::Span;
    use syn::{DeriveInput, parse_quote};

    #[test]
    fn one_marked_field_is_the_events_identity() {
        let input: DeriveInput = parse_quote! {
            struct DepositPaid {
                #[event_id]
                id: EventId,
                cents: i64,
            }
        };
        let member = event_id_field(&input).expect("valid").expect("marked");
        assert_eq!(quote::quote!(#member).to_string(), "id");
    }

    #[test]
    fn an_event_has_one_identity() {
        let input: DeriveInput = parse_quote! {
            struct DepositPaid {
                #[event_id]
                id: EventId,
                #[event_id]
                other: EventId,
            }
        };
        let error = event_id_field(&input).expect_err("two identities");
        assert!(error.to_string().contains("exactly one field"), "{error}");
    }

    #[test]
    fn an_event_id_marks_a_struct_field_only() {
        let input: DeriveInput = parse_quote! {
            enum Events {
                Paid(#[event_id] EventId),
            }
        };
        let error = event_id_field(&input).expect_err("on an enum");
        assert!(error.to_string().contains("a field of a struct"), "{error}");
    }

    fn check(s: &str) -> bool {
        validate_subject(s, Span::call_site()).is_ok()
    }

    #[test]
    fn accepts_the_same_subjects_as_runtime() {
        for ok in [
            "events.user.created",
            "simple",
            "a.b.c.d.e",
            "tenant-123.events_v2",
        ] {
            assert!(check(ok), "{ok}");
        }
    }

    #[test]
    fn rejects_the_same_subjects_as_runtime() {
        for bad in [
            "",
            ".leading",
            "trailing.",
            "double..dot",
            "with space",
            "events.*",
            "a.>",
        ] {
            assert!(!check(bad), "{bad}");
        }
    }
}
