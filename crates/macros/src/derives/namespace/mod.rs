//! `#[derive(Namespace)]`: an enum of relations as a typed Zanzibar object type.

mod rel_expr;

use std::collections::BTreeSet;

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::parse::{Parse, ParseStream};
use syn::{Data, DeriveInput, Fields, Ident, LitStr, Path, Token, parse_macro_input};

use crate::derives::authz_path;
use crate::derives::case::to_snake_case;
use rel_expr::RelExpr;

pub(crate) fn derive(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match expand(&input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

#[expect(clippy::too_many_lines, reason = "readability baseline")]
fn expand(input: &DeriveInput) -> syn::Result<TokenStream2> {
    let name = &input.ident;

    if !input.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &input.generics,
            "Namespace cannot be derived for a generic enum; a namespace is a fixed object type",
        ));
    }

    let data = match &input.data {
        Data::Enum(data) => data,
        _ => {
            return Err(syn::Error::new_spanned(
                name,
                "Namespace can only be derived for an enum of relations",
            ));
        }
    };

    let options = parse_namespace_override(input)?;
    let namespace_name = options
        .name
        .clone()
        .unwrap_or_else(|| to_snake_case(&name.to_string()));
    // Emitted only when declared; the trait's identity default covers opaque ids.
    let canonical_id_impl = options.canonical_id.as_ref().map(|path| {
        quote! {
            fn canonical_id(raw: &str) -> ::core::option::Option<::std::borrow::Cow<'_, str>> {
                #path(raw)
            }
        }
    });

    let authz = authz_path();
    let schema = quote!(#authz::schema);

    let mut relation_arms = Vec::new(); // impl Relation match arms
    let mut markers = Vec::new(); // one marker type per defined relation
    let mut marker_impls = Vec::new(); // its `RelationOf`, beside the enum
    let module = format_ident!("{}_relation", to_snake_case(&name.to_string()));
    let mut relation_irs = Vec::new(); // RelationIr construction per variant
    let mut asserted_types: BTreeSet<String> = BTreeSet::new();
    let mut assert_idents: Vec<Ident> = Vec::new();

    for variant in &data.variants {
        if !matches!(variant.fields, Fields::Unit) {
            return Err(syn::Error::new_spanned(
                variant,
                "Namespace relations must be unit variants; a relation carries no data",
            ));
        }
        let relation_name = to_snake_case(&variant.ident.to_string());
        let variant_ident = &variant.ident;

        // Every variant, skipped or not: `relation_name` is total over the enum.
        relation_arms.push(quote!(Self::#variant_ident => #relation_name));

        let locked = parse_locked(variant)?;
        let Some(relation_expr) = parse_relation(variant, data)? else {
            if let Some(attr) = locked {
                return Err(syn::Error::new_spanned(
                    attr,
                    "`#[locked]` declares a relation for locked checks, and a \
                     `#[relation(skip)]` variant defines no relation",
                ));
            }
            continue;
        };
        if let Some(attr) = locked
            && relation_expr.exclusion().is_some()
        {
            return Err(syn::Error::new_spanned(
                attr,
                "a `#[locked]` relation cannot hold an `exclusion(...)`: a locked check \
                 locks the tuples its allow rests on, and an exclusion grants on a tuple's \
                 absence, which cannot be locked",
            ));
        }
        let lock = locked.map(|_| quote!(.with_lock()));
        let (marker, marker_impl) =
            relation_marker(name, &module, variant_ident, &relation_name, &schema);
        markers.push(marker);
        marker_impls.push(marker_impl);

        let rewrite = relation_expr.to_rewrite_tokens(&authz);
        let mut restrictions = Vec::new();
        relation_expr.collect_type_restrictions(&authz, &mut restrictions);
        relation_irs.push(quote! {
            #schema::RelationIr::new(
                #schema::RelationName::new(#relation_name)
                    .expect("derive: valid relation identifier"),
                #rewrite,
                ::std::vec![ #( #restrictions ),* ],
            ) #lock
        });

        let mut referenced = Vec::new();
        relation_expr.collect_namespace_types(&mut referenced);
        for ident in referenced {
            if asserted_types.insert(ident.to_string()) {
                assert_idents.push(ident);
            }
        }
    }

    // `match *self` also covers an empty enum.
    let relation_impl = quote! {
        impl #authz::schema::Relation for #name {
            fn relation_name(&self) -> &'static str {
                match *self {
                    #( #relation_arms, )*
                }
            }
        }
    };

    let namespace_impl = quote! {
        impl #authz::schema::Namespace for #name {
            const NAME: &'static str = #namespace_name;
            type Relation = Self;

            #canonical_id_impl

            #[allow(clippy::expect_used, clippy::unwrap_used)]
            fn ir() -> #schema::NamespaceIr {
                #schema::NamespaceIr::new(
                    #schema::NamespaceName::new(#namespace_name)
                        .expect("derive: valid namespace identifier"),
                    ::std::vec![ #( #relation_irs ),* ],
                )
            }
        }
    };

    // One const per referenced type forces `T: Namespace` at compile time.
    let assertions = assert_idents.iter().map(|ty| {
        quote! {
            const _: fn() = || {
                fn __assert_namespace<T: #authz::schema::Namespace>() {}
                __assert_namespace::<#ty>();
            };
        }
    });

    let marker_module = (!markers.is_empty()).then(|| {
        let vis = &input.vis;
        let doc = format!(
            "The relations of `{name}`, one type each, so a signature can ask for an object \
             at a relation by type: `Authorized<{name}, {module}::…>`."
        );
        quote! {
            #[doc = #doc]
            #vis mod #module {
                #( #markers )*
            }
            #( #marker_impls )*
        }
    });

    Ok(quote! {
        #relation_impl
        #namespace_impl
        #marker_module
        #( #assertions )*
    })
}

/// A relation's marker type and its `RelationOf` impl, emitted beside the enum so a namespace declared in a function body is in scope.
fn relation_marker(
    namespace: &Ident,
    module: &Ident,
    variant: &Ident,
    relation_name: &str,
    schema: &TokenStream2,
) -> (TokenStream2, TokenStream2) {
    let doc = format!("The `{relation_name}` relation of `{namespace}`, as a type.");
    let marker = quote! {
        #[doc = #doc]
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
        pub struct #variant;
    };
    let marker_impl = quote! {
        impl #schema::RelationOf<#namespace> for #module::#variant {
            const RELATION: #namespace = #namespace::#variant;
        }
    };
    (marker, marker_impl)
}

#[derive(Default)]
struct NamespaceOptions {
    /// The wire name, defaulting to the enum in snake_case.
    name: Option<String>,
    canonical_id: Option<Path>,
}

impl Parse for NamespaceOptions {
    #[expect(clippy::excessive_nesting, reason = "readability baseline")]
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut options = Self::default();
        while !input.is_empty() {
            if input.peek(LitStr) {
                options.name = Some(input.parse::<LitStr>()?.value());
            } else {
                let key: Ident = input.parse()?;
                if key != "canonical_id" {
                    return Err(syn::Error::new_spanned(
                        &key,
                        "unsupported `#[namespace]` argument; expected a name literal or `canonical_id = <path>`",
                    ));
                }
                input.parse::<Token![=]>()?;
                options.canonical_id = Some(input.parse()?);
            }
            if input.is_empty() {
                break;
            }
            input.parse::<Token![,]>()?;
        }
        Ok(options)
    }
}

fn parse_namespace_override(input: &DeriveInput) -> syn::Result<NamespaceOptions> {
    let mut found: Option<NamespaceOptions> = None;
    for attr in &input.attrs {
        if !attr.path().is_ident("namespace") {
            continue;
        }
        // Refused, not ignored: dropping a second attribute could silently drop its `canonical_id`.
        if found.is_some() {
            return Err(syn::Error::new_spanned(
                attr,
                "duplicate `#[namespace(...)]`; declare the name and `canonical_id` in one attribute",
            ));
        }
        found = Some(attr.parse_args()?);
    }
    Ok(found.unwrap_or_default())
}

fn parse_locked(variant: &syn::Variant) -> syn::Result<Option<&syn::Attribute>> {
    let Some(attr) = variant.attrs.iter().find(|a| a.path().is_ident("locked")) else {
        return Ok(None);
    };
    attr.meta.require_path_only()?;
    Ok(Some(attr))
}

/// A variant's mandatory `#[relation(...)]`, or `None` for `#[relation(skip)]`.
fn parse_relation(variant: &syn::Variant, data: &syn::DataEnum) -> syn::Result<Option<RelExpr>> {
    let attr = variant
        .attrs
        .iter()
        .find(|a| a.path().is_ident("relation"))
        .ok_or_else(|| {
            syn::Error::new_spanned(
                variant,
                "each relation variant needs a `#[relation(...)]` declaring its rewrite \
                 (e.g. `#[relation(direct(User))]`), or `#[relation(skip)]` if the variant \
                 is a scope with no per-object meaning",
            )
        })?;

    if attr
        .parse_args::<Ident>()
        .is_ok_and(|ident| ident == "skip")
    {
        // `skip` is ambiguous beside a `Skip` variant, so it is refused rather than guessed.
        if data.variants.iter().any(|other| other.ident == "Skip") {
            return Err(syn::Error::new_spanned(
                attr,
                "`#[relation(skip)]` is ambiguous here: this enum has a `Skip` variant, so \
                 `skip` also reads as the computed-userset shorthand for it. Write \
                 `#[relation(computed_userset(skip))]` for the shorthand, or rename `Skip`",
            ));
        }
        return Ok(None);
    }

    attr.parse_args::<RelExpr>().map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;
    use syn::parse_quote;

    fn refusal(input: DeriveInput) -> String {
        expand(&input)
            .err()
            .map(|error| error.to_string())
            .unwrap_or_default()
    }

    #[test]
    fn a_locked_relation_lowers_with_its_lock() {
        let input: DeriveInput = parse_quote! {
            enum Team {
                #[relation(direct(User))]
                #[locked]
                Member,
                #[relation(direct(User))]
                Guest,
            }
        };
        let tokens = expand(&input)
            .expect("a locked relation derives")
            .to_string();
        assert_eq!(tokens.matches("with_lock").count(), 1, "{tokens}");
    }

    #[test]
    fn derive_refuses_a_local_exclusion_on_a_locked_relation() {
        let input: DeriveInput = parse_quote! {
            enum Team {
                #[relation(direct(User))]
                Member,
                #[relation(direct(User))]
                Banned,
                #[relation(union(Member, exclusion(Member, Banned)))]
                #[locked]
                Active,
            }
        };
        assert!(
            refusal(input).contains("cannot hold an `exclusion(...)`"),
            "a locked relation holding an exclusion must not derive"
        );
    }

    #[test]
    fn derive_refuses_a_lock_on_a_skipped_variant() {
        let input: DeriveInput = parse_quote! {
            enum Team {
                #[relation(skip)]
                #[locked]
                Audit,
            }
        };
        assert!(refusal(input).contains("defines no relation"));
    }

    #[test]
    fn derive_refuses_arguments_on_the_lock() {
        let input: DeriveInput = parse_quote! {
            enum Team {
                #[relation(direct(User))]
                #[locked(always)]
                Member,
            }
        };
        assert!(!refusal(input).is_empty());
    }

    #[test]
    fn emits_one_marker_per_relation() {
        let input: DeriveInput = parse_quote! {
            pub enum Team {
                #[relation(direct(User))]
                Admin,
                #[relation(direct(User))]
                Member,
            }
        };
        let tokens = expand(&input).expect("derives").to_string();
        assert!(tokens.contains("pub mod team_relation"), "{tokens}");
        assert!(tokens.contains("pub struct Admin"), "{tokens}");
        assert!(tokens.contains("pub struct Member"), "{tokens}");
        assert_eq!(
            tokens
                .matches("RelationOf < Team > for team_relation ::")
                .count(),
            2,
            "{tokens}"
        );
    }

    #[test]
    fn a_skipped_relation_has_no_marker() {
        let input: DeriveInput = parse_quote! {
            enum Team {
                #[relation(direct(User))]
                Member,
                #[relation(skip)]
                Audit,
            }
        };
        let tokens = expand(&input).expect("derives").to_string();
        assert!(tokens.contains("pub struct Member"), "{tokens}");
        assert!(!tokens.contains("pub struct Audit"), "{tokens}");
    }

    #[test]
    fn a_namespace_without_relations_has_no_marker_module() {
        let input: DeriveInput = parse_quote! {
            pub enum User {}
        };
        let tokens = expand(&input).expect("derives").to_string();
        assert!(!tokens.contains("user_relation"), "{tokens}");
    }
}
