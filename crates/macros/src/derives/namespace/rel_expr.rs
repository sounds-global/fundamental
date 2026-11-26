//! The `#[relation(...)]` rewrite-expression DSL: its parsed form and the code it lowers to.

use proc_macro2::TokenStream;
use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::{Ident, Path, Token, parenthesized, token};

use crate::derives::case::to_snake_case;

/// One direct-subject shape in `direct(...)`: a namespace type or a `Namespace::Relation` userset.
pub(super) struct TypeRestriction {
    pub namespace: Ident,
    pub relation: Option<Ident>,
}

impl Parse for TypeRestriction {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        // A path so it lexes on edition 2021+, where `Group#member` would be a reserved prefix.
        let path: Path = input.parse()?;
        let segments: Vec<Ident> = path.segments.iter().map(|s| s.ident.clone()).collect();
        match segments.as_slice() {
            [namespace] => Ok(Self {
                namespace: namespace.clone(),
                relation: None,
            }),
            [namespace, relation] => Ok(Self {
                namespace: namespace.clone(),
                relation: Some(relation.clone()),
            }),
            _ => Err(syn::Error::new_spanned(
                &path,
                "a direct-subject restriction is `Namespace` (a leaf) or \
                 `Namespace::Relation` (a userset)",
            )),
        }
    }
}

pub(super) enum RelExpr {
    /// A bare relation name or `computed_userset(rel)`.
    Computed(Ident),
    /// `direct(...)`: tuples stored on this relation, restricted to the listed shapes.
    Direct(Vec<TypeRestriction>),
    /// `tuple_to_userset(tupleset, Target::computed)`.
    TupleToUserset {
        tupleset: Ident,
        target_namespace: Ident,
        computed: Ident,
    },
    /// `union(...)`.
    Union(Vec<RelExpr>),
    /// `intersection(...)`.
    Intersection(Vec<RelExpr>),
    /// `exclusion(include, exclude)`.
    Exclusion(Box<RelExpr>, Box<RelExpr>),
}

impl Parse for RelExpr {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let head: Ident = input.parse()?;
        if !input.peek(token::Paren) {
            return Ok(RelExpr::Computed(head));
        }
        let content;
        parenthesized!(content in input);
        match head.to_string().as_str() {
            "direct" => {
                let restrictions = content.parse_terminated(TypeRestriction::parse, Token![,])?;
                Ok(RelExpr::Direct(restrictions.into_iter().collect()))
            }
            "computed_userset" => {
                let relation: Ident = content.parse()?;
                Ok(RelExpr::Computed(relation))
            }
            "union" => Ok(RelExpr::Union(parse_children(&content)?)),
            "intersection" => Ok(RelExpr::Intersection(parse_children(&content)?)),
            "exclusion" => {
                let include: RelExpr = content.parse()?;
                content.parse::<Token![,]>()?;
                let exclude: RelExpr = content.parse()?;
                Ok(RelExpr::Exclusion(Box::new(include), Box::new(exclude)))
            }
            "tuple_to_userset" => {
                let tupleset: Ident = content.parse()?;
                content.parse::<Token![,]>()?;
                let target: Path = content.parse()?;
                let (target_namespace, computed) = split_target(&target)?;
                Ok(RelExpr::TupleToUserset {
                    tupleset,
                    target_namespace,
                    computed,
                })
            }
            other => Err(syn::Error::new_spanned(
                &head,
                format!(
                    "unknown relation operator `{other}`; expected one of direct, union, \
                     intersection, exclusion, computed_userset, tuple_to_userset"
                ),
            )),
        }
    }
}

fn parse_children(content: ParseStream) -> syn::Result<Vec<RelExpr>> {
    let children = content.parse_terminated(RelExpr::parse, Token![,])?;
    Ok(children.into_iter().collect())
}

fn split_target(path: &Path) -> syn::Result<(Ident, Ident)> {
    let segments: Vec<&Ident> = path.segments.iter().map(|s| &s.ident).collect();
    match segments.as_slice() {
        [namespace, relation] => Ok(((*namespace).clone(), (*relation).clone())),
        _ => Err(syn::Error::new_spanned(
            path,
            "tuple_to_userset expects `Tupleset, Target::relation` (a two-segment path)",
        )),
    }
}

impl RelExpr {
    pub(super) fn exclusion(&self) -> Option<&Self> {
        match self {
            RelExpr::Exclusion(..) => Some(self),
            RelExpr::Union(children) | RelExpr::Intersection(children) => {
                children.iter().find_map(Self::exclusion)
            }
            RelExpr::Computed(_) | RelExpr::Direct(_) | RelExpr::TupleToUserset { .. } => None,
        }
    }

    pub(super) fn to_rewrite_tokens(&self, authz: &TokenStream) -> TokenStream {
        let schema = quote!(#authz::schema);
        match self {
            RelExpr::Computed(relation) => {
                let name = relation_name_tokens(authz, relation);
                quote!(#schema::RewriteIr::ComputedUserset { relation: #name })
            }
            RelExpr::Direct(_) => quote!(#schema::RewriteIr::This),
            RelExpr::TupleToUserset {
                tupleset, computed, ..
            } => {
                let tupleset = relation_name_tokens(authz, tupleset);
                let computed = relation_name_tokens(authz, computed);
                quote!(#schema::RewriteIr::TupleToUserset { tupleset: #tupleset, computed: #computed })
            }
            RelExpr::Union(children) => {
                let children = children.iter().map(|c| c.to_rewrite_tokens(authz));
                quote!(#schema::RewriteIr::Union(::std::vec![ #( #children ),* ]))
            }
            RelExpr::Intersection(children) => {
                let children = children.iter().map(|c| c.to_rewrite_tokens(authz));
                quote!(#schema::RewriteIr::Intersection(::std::vec![ #( #children ),* ]))
            }
            RelExpr::Exclusion(include, exclude) => {
                let include = include.to_rewrite_tokens(authz);
                let exclude = exclude.to_rewrite_tokens(authz);
                quote!(#schema::RewriteIr::Exclusion {
                    include: ::std::boxed::Box::new(#include),
                    exclude: ::std::boxed::Box::new(#exclude),
                })
            }
        }
    }

    pub(super) fn collect_type_restrictions(
        &self,
        authz: &TokenStream,
        out: &mut Vec<TokenStream>,
    ) {
        match self {
            RelExpr::Direct(restrictions) => {
                for restriction in restrictions {
                    out.push(related_type_tokens(authz, restriction));
                }
            }
            RelExpr::Computed(_) => {}
            RelExpr::TupleToUserset { .. } => {}
            RelExpr::Union(children) | RelExpr::Intersection(children) => {
                for child in children {
                    child.collect_type_restrictions(authz, out);
                }
            }
            RelExpr::Exclusion(include, exclude) => {
                include.collect_type_restrictions(authz, out);
                exclude.collect_type_restrictions(authz, out);
            }
        }
    }

    /// The namespace types this expression references, for the `T: Namespace` assertions.
    pub(super) fn collect_namespace_types(&self, out: &mut Vec<Ident>) {
        match self {
            RelExpr::Direct(restrictions) => {
                for restriction in restrictions {
                    out.push(restriction.namespace.clone());
                }
            }
            RelExpr::Computed(_) => {}
            RelExpr::TupleToUserset {
                target_namespace, ..
            } => out.push(target_namespace.clone()),
            RelExpr::Union(children) | RelExpr::Intersection(children) => {
                for child in children {
                    child.collect_namespace_types(out);
                }
            }
            RelExpr::Exclusion(include, exclude) => {
                include.collect_namespace_types(out);
                exclude.collect_namespace_types(out);
            }
        }
    }
}

fn relation_name_tokens(authz: &TokenStream, ident: &Ident) -> TokenStream {
    let name = to_snake_case(&ident.to_string());
    quote!(#authz::schema::RelationName::new(#name).expect("derive: valid relation identifier"))
}

fn related_type_tokens(authz: &TokenStream, restriction: &TypeRestriction) -> TokenStream {
    let schema = quote!(#authz::schema);
    let namespace = to_snake_case(&restriction.namespace.to_string());
    let namespace_tokens = quote!(#schema::NamespaceName::new(#namespace).expect("derive: valid namespace identifier"));
    match &restriction.relation {
        None => quote!(#schema::RelatedType::leaf(#namespace_tokens)),
        Some(relation) => {
            let relation_name = to_snake_case(&relation.to_string());
            let relation_tokens = quote!(
                #schema::RelationName::new(#relation_name).expect("derive: valid relation identifier")
            );
            quote!(#schema::RelatedType::userset(#namespace_tokens, #relation_tokens))
        }
    }
}
