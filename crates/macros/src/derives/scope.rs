//! `#[derive(Scope)]`: an enum as a typed authorization scope vocabulary.

use std::collections::BTreeMap;

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{Data, DeriveInput, Expr, Fields, Ident, LitStr, parse_macro_input};

use crate::derives::case::to_snake_case;
use crate::derives::core_path;

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
            "Scope cannot be derived for a generic enum; a scope vocabulary is a fixed set",
        ));
    }

    let Data::Enum(data) = &input.data else {
        return Err(syn::Error::new_spanned(
            name,
            "Scope can only be derived for an enum, where each variant names one scope",
        ));
    };

    let prefix = parse_prefix(input)?.unwrap_or_else(|| default_prefix(&name.to_string()));

    // Two passes: a group may appear before the leaves it names.
    let mut leaves: BTreeMap<String, String> = BTreeMap::new();
    let mut groups: Vec<(Ident, Vec<Ident>)> = Vec::new();
    // Skipped variants still require their scope, or the guard admits everyone; they never reach `ALL_SCOPES`.
    let mut skipped: Vec<String> = Vec::new();

    for variant in &data.variants {
        if !matches!(variant.fields, Fields::Unit) {
            return Err(syn::Error::new_spanned(
                variant,
                "a scope must be a unit variant; a scope carries no data",
            ));
        }
        let scope = format!("{prefix}:{}", to_snake_case(&variant.ident.to_string()));
        match parse_variant(variant)? {
            VariantKind::Group(members) => groups.push((variant.ident.clone(), members)),
            VariantKind::Skipped => {
                skipped.push(variant.ident.to_string());
                leaves.insert(variant.ident.to_string(), scope);
            }
            VariantKind::Leaf => {
                leaves.insert(variant.ident.to_string(), scope);
            }
        }
    }

    let mut arms: Vec<TokenStream2> = Vec::new();
    for variant in &data.variants {
        let ident = &variant.ident;
        if let Some(scope) = leaves.get(&ident.to_string()) {
            arms.push(quote!(Self::#ident => &[#scope]));
            continue;
        }
        let members = groups
            .iter()
            .find(|(group, _)| group == ident)
            .map(|(_, members)| members)
            .ok_or_else(|| syn::Error::new_spanned(ident, "internal: unclassified variant"))?;

        let mut scopes: Vec<&String> = Vec::new();
        for member in members {
            // Groups of groups would need cycle detection.
            let scope = leaves.get(&member.to_string()).ok_or_else(|| {
                syn::Error::new_spanned(
                    member,
                    format!(
                        "`{member}` is not a plain scope variant of this enum; \
                         a group may only name variants that are themselves single scopes"
                    ),
                )
            })?;
            // A group with an ungrantable member could never be satisfied.
            if skipped.contains(&member.to_string()) {
                return Err(syn::Error::new_spanned(
                    member,
                    format!(
                        "`{member}` is `#[scope(skip)]`, so it is not grantable; \
                         a group naming it could never be satisfied by any bearer"
                    ),
                ));
            }
            scopes.push(scope);
        }
        if scopes.is_empty() {
            return Err(syn::Error::new_spanned(
                ident,
                "a scope group must name at least one variant",
            ));
        }
        arms.push(quote!(Self::#ident => &[#(#scopes),*]));
    }

    // Only grantable leaves: groups add no wire string, and a skipped variant here would become grantable.
    let all: Vec<&String> = leaves
        .iter()
        .filter(|(variant, _)| !skipped.contains(variant))
        .map(|(_, scope)| scope)
        .collect();

    let core = core_path();
    let all_doc = format!(
        "Every **grantable** wire scope `{name}` defines, in lexical order.\n\n\
         The vocabulary itself, for validating input against the real set — a CLI \
         parsing `--scopes`, or a test asserting a route's guard — rather than a \
         second copy that can drift from it. A `#[scope(skip)]` variant is absent: \
         it names a relation, not an act, and nothing may mint it."
    );

    Ok(quote! {
        impl #name {
            #[doc = #all_doc]
            pub const ALL_SCOPES: &'static [&'static str] = &[#(#all),*];
        }

        impl #core::ScopeSet for #name {
            fn required_scopes(&self) -> &'static [&'static str] {
                match self {
                    #(#arms),*
                }
            }
        }
    })
}

/// The enum name in `snake_case`, less a trailing `Scope`/`Scopes`, which must never reach the wire.
fn default_prefix(name: &str) -> String {
    let trimmed = name
        .strip_suffix("Scopes")
        .or_else(|| name.strip_suffix("Scope"))
        .filter(|stem| !stem.is_empty())
        .unwrap_or(name);
    to_snake_case(trimmed)
}

fn parse_prefix(input: &DeriveInput) -> syn::Result<Option<String>> {
    let mut prefix = None;
    for attr in &input.attrs {
        if !attr.path().is_ident("scope") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("prefix") {
                let value: LitStr = meta.value()?.parse()?;
                prefix = Some(value.value());
                Ok(())
            } else {
                Err(meta
                    .error("unsupported `#[scope]` argument on an enum; expected `prefix = \"…\"`"))
            }
        })?;
    }
    Ok(prefix)
}

enum VariantKind {
    Leaf,
    Group(Vec<Ident>),
    /// Not grantable (`#[scope(skip)]`).
    Skipped,
}

#[expect(clippy::excessive_nesting, reason = "readability baseline")]
fn parse_variant(variant: &syn::Variant) -> syn::Result<VariantKind> {
    let mut kind = VariantKind::Leaf;
    for attr in &variant.attrs {
        if !attr.path().is_ident("scope") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("skip") {
                if matches!(kind, VariantKind::Group(_)) {
                    return Err(meta.error(
                        "`skip` and `all` are contradictory: a variant that grants nothing \
                         cannot also name a conjunction of grants",
                    ));
                }
                kind = VariantKind::Skipped;
                Ok(())
            } else if meta.path.is_ident("all") {
                if matches!(kind, VariantKind::Skipped) {
                    return Err(meta.error(
                        "`all` and `skip` are contradictory: a variant that grants nothing \
                         cannot also name a conjunction of grants",
                    ));
                }
                let array: syn::ExprArray = meta.value()?.parse()?;
                let mut idents = Vec::new();
                for element in &array.elems {
                    let Expr::Path(path) = element else {
                        return Err(syn::Error::new_spanned(
                            element,
                            "a scope group names variants of this enum, e.g. `all = [Revoke, Suspend]`",
                        ));
                    };
                    let ident = path.path.get_ident().ok_or_else(|| {
                        syn::Error::new_spanned(
                            element,
                            "a scope group names bare variants, not a qualified path",
                        )
                    })?;
                    idents.push(ident.clone());
                }
                kind = VariantKind::Group(idents);
                Ok(())
            } else {
                Err(meta.error("unsupported `#[scope]` argument; expected `all = [A, B]` or `skip`"))
            }
        })?;
    }
    Ok(kind)
}

#[cfg(test)]
mod tests {
    use super::*;
    use syn::parse_quote;

    fn expand_ok(input: DeriveInput) -> String {
        expand(&input).expect("expands").to_string()
    }

    #[test]
    fn a_variant_becomes_a_prefixed_snake_case_scope() {
        let input: DeriveInput = parse_quote! {
            enum Credential { Revoke, SourceChange }
        };
        let out = expand_ok(input);
        assert!(out.contains(r#"Self :: Revoke => & ["credential:revoke"]"#));
        assert!(out.contains(r#"Self :: SourceChange => & ["credential:source_change"]"#));
    }

    #[test]
    fn a_scope_or_scopes_suffix_is_stripped_from_the_prefix() {
        assert_eq!(default_prefix("Subject"), "subject");
        assert_eq!(default_prefix("SubjectScope"), "subject");
        assert_eq!(default_prefix("SubjectScopes"), "subject");
    }

    #[test]
    fn all_three_spellings_produce_the_same_scope() {
        for name in ["Subject", "SubjectScope", "SubjectScopes"] {
            let ident = Ident::new(name, proc_macro2::Span::call_site());
            let input: DeriveInput = parse_quote! {
                enum #ident { Erase }
            };
            assert!(
                expand_ok(input).contains(r#"& ["subject:erase"]"#),
                "{name} must yield subject:erase"
            );
        }
    }

    #[test]
    fn a_bare_scope_type_name_is_not_stripped_to_nothing() {
        assert_eq!(default_prefix("Scope"), "scope");
        assert_eq!(default_prefix("Scopes"), "scopes");
    }

    #[test]
    fn an_explicit_prefix_wins_over_the_type_name() {
        let input: DeriveInput = parse_quote! {
            #[scope(prefix = "federation")]
            enum Whatever { Accredit }
        };
        assert!(expand_ok(input).contains(r#"& ["federation:accredit"]"#));
    }

    #[test]
    fn a_skipped_variant_is_absent_from_the_grantable_vocabulary() {
        let input: DeriveInput = parse_quote! {
            enum Credential {
                #[scope(skip)]
                Issuer,
                Revoke,
            }
        };
        let out = expand_ok(input);
        assert!(
            out.contains(r#"ALL_SCOPES : & 'static [& 'static str] = & ["credential:revoke"]"#),
            "{out}"
        );
    }

    /// An empty requirement would admit every authenticated principal; an ungrantable one refuses everyone.
    #[test]
    fn a_skipped_variant_still_requires_something_so_a_guard_on_it_fails_closed() {
        let input: DeriveInput = parse_quote! {
            enum Credential {
                #[scope(skip)]
                Issuer,
                Revoke,
            }
        };
        let out = expand_ok(input);
        assert!(
            out.contains(r#"Self :: Issuer => & ["credential:issuer"]"#),
            "a skipped variant must not require the empty set: {out}"
        );
    }

    #[test]
    fn rejects_a_group_naming_a_skipped_variant() {
        let input: DeriveInput = parse_quote! {
            enum Credential {
                #[scope(skip)]
                Issuer,
                Revoke,
                #[scope(all = [Revoke, Issuer])]
                Everything,
            }
        };
        let err = expand(&input).expect_err("ungrantable member");
        assert!(err.to_string().contains("not grantable"), "{err}");
    }

    #[test]
    fn a_group_expands_to_every_scope_it_names() {
        let input: DeriveInput = parse_quote! {
            enum Credential {
                Revoke,
                Suspend,
                #[scope(all = [Revoke, Suspend])]
                Lifecycle,
            }
        };
        let out = expand_ok(input);
        assert!(
            out.contains(r#"Self :: Lifecycle => & ["credential:revoke" , "credential:suspend"]"#),
            "{out}"
        );
    }

    #[test]
    fn a_group_may_precede_its_members() {
        let input: DeriveInput = parse_quote! {
            enum Credential {
                #[scope(all = [Revoke])]
                Lifecycle,
                Revoke,
            }
        };
        assert!(expand_ok(input).contains(r#"Self :: Lifecycle => & ["credential:revoke"]"#));
    }

    #[test]
    fn all_scopes_lists_the_leaves_only() {
        let input: DeriveInput = parse_quote! {
            enum Credential {
                Revoke,
                Suspend,
                #[scope(all = [Revoke, Suspend])]
                Lifecycle,
            }
        };
        let out = expand_ok(input);
        assert!(out.contains(r#"ALL_SCOPES : & 'static [& 'static str] = & ["credential:revoke" , "credential:suspend"]"#), "{out}");
    }

    #[test]
    fn rejects_a_group_naming_an_unknown_variant() {
        let input: DeriveInput = parse_quote! {
            enum Credential {
                Revoke,
                #[scope(all = [Reinstate])]
                Lifecycle,
            }
        };
        let err = expand(&input).expect_err("unknown member");
        assert!(err.to_string().contains("not a plain scope variant"));
    }

    #[test]
    fn rejects_a_group_naming_another_group() {
        let input: DeriveInput = parse_quote! {
            enum Credential {
                Revoke,
                #[scope(all = [Revoke])]
                Lifecycle,
                #[scope(all = [Lifecycle])]
                Everything,
            }
        };
        let err = expand(&input).expect_err("nested group");
        assert!(err.to_string().contains("not a plain scope variant"));
    }

    #[test]
    fn rejects_a_non_unit_variant() {
        let input: DeriveInput = parse_quote! {
            enum Credential { Revoke(String) }
        };
        let err = expand(&input).expect_err("carries data");
        assert!(err.to_string().contains("unit variant"));
    }

    #[test]
    fn rejects_a_struct() {
        let input: DeriveInput = parse_quote! {
            struct Credential { revoke: bool }
        };
        let err = expand(&input).expect_err("not an enum");
        assert!(err.to_string().contains("only be derived for an enum"));
    }
}
