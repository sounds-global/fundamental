//! Field-level analysis of an Error variant.

use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::{Attribute, Fields, Ident, Type};

/// A variant's fields, analysed.
pub(super) struct FieldAnalysis {
    /// Destructures every field by name.
    pub pattern: TokenStream2,
    /// Binds no field, for static-return arms.
    pub wildcard_pattern: TokenStream2,
    pub source_field: Option<TokenStream2>,
    /// Binds only the `#[source]` field, so `source()` binds nothing unused.
    pub source_pattern: Option<TokenStream2>,
    pub from_fields: Vec<(Type, bool, Option<Ident>)>, // (type, is_tuple, field_name)
    /// Bound identifiers, parallel to `bound_names`.
    pub bound_idents: Vec<TokenStream2>,
    pub bound_names: Vec<String>,
    /// Whether each bound field is `Option<_>`, for `OptDisplay` shadowing.
    pub bound_is_option: Vec<bool>,
    /// Each bound field's type, so numeric fields pass as typed `ArgValue`s.
    pub bound_types: Vec<Type>,
    /// Whether each bound field is `#[error(skip_param)]`.
    pub bound_skip_param: Vec<bool>,
    /// Bindings of `#[overrides]` fields, in declaration order.
    pub override_fields: Vec<TokenStream2>,
    /// Binds only the `#[overrides]` fields.
    pub override_pattern: Option<TokenStream2>,
}

impl FieldAnalysis {
    pub(super) fn has_source_attr(attrs: &[Attribute]) -> bool {
        attrs.iter().any(|attr| attr.path().is_ident("source"))
    }

    pub(super) fn has_from_attr(attrs: &[Attribute]) -> bool {
        attrs.iter().any(|attr| attr.path().is_ident("from"))
    }

    /// Whether a field is `#[error(skip_param)]`; the variant-level `#[error("…")]` fails to parse and is ignored.
    #[expect(clippy::excessive_nesting, reason = "readability baseline")]
    pub(super) fn has_skip_param_attr(attrs: &[Attribute]) -> bool {
        attrs.iter().any(|attr| {
            if !attr.path().is_ident("error") {
                return false;
            }
            let mut found = false;
            let _ = attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("skip_param") {
                    found = true;
                }
                Ok(())
            });
            found
        })
    }

    /// Whether a type is `AnyError`, under any qualified path.
    fn is_any_error(ty: &Type) -> bool {
        let syn::Type::Path(type_path) = ty else {
            return false;
        };
        if type_path.qself.is_some() {
            return false;
        }
        type_path
            .path
            .segments
            .last()
            .is_some_and(|last| last.ident == "AnyError" && last.arguments.is_none())
    }

    /// Whether a field has the bare `#[overrides]`; `#[overrides(...)]` is refused with a pointer to `derive(ErrorOverride)`.
    pub(super) fn has_overrides_attr(attrs: &[Attribute]) -> syn::Result<bool> {
        for attr in attrs {
            if !attr.path().is_ident("overrides") {
                continue;
            }
            if !matches!(attr.meta, syn::Meta::Path(_)) {
                return Err(syn::Error::new_spanned(
                    attr,
                    "`#[overrides(...)]` with values is not valid on a field; \
                     use a bare `#[overrides]` to delegate to the field's value, \
                     or declare static values with `#[error(status = ...)]` on the variant",
                ));
            }
            return Ok(true);
        }
        Ok(false)
    }

    /// Analyses a variant's fields; with `catch_all_eligible`, a sole `AnyError` field implies `#[from]` and `#[overrides]`.
    #[expect(
        clippy::too_many_lines,
        clippy::cognitive_complexity,
        clippy::excessive_nesting,
        reason = "readability baseline"
    )]
    pub(super) fn analyze(fields: &Fields, catch_all_eligible: bool) -> syn::Result<FieldAnalysis> {
        let implicit_catch_all = catch_all_eligible
            && fields.len() == 1
            && fields
                .iter()
                .next()
                .is_some_and(|f| Self::is_any_error(&f.ty));

        Ok(match fields {
            Fields::Unit => FieldAnalysis {
                pattern: quote! {},
                wildcard_pattern: quote! {},
                source_field: None,
                source_pattern: None,
                from_fields: Vec::new(),
                bound_idents: Vec::new(),
                bound_names: Vec::new(),
                bound_is_option: Vec::new(),
                bound_types: Vec::new(),
                bound_skip_param: Vec::new(),
                override_fields: Vec::new(),
                override_pattern: None,
            },
            Fields::Unnamed(unnamed) => {
                let mut source_idx = None;
                // `#[from]` implies `#[source]`; an explicit `#[source]` wins.
                let mut implied_source = None;
                let mut source_pos = None;
                let mut implied_source_pos = None;
                let mut patterns = Vec::new();
                let mut from_fields = Vec::new();
                let mut bound_idents = Vec::new();
                let mut bound_names = Vec::new();
                let mut bound_is_option = Vec::new();
                let mut bound_types = Vec::new();
                let mut bound_skip_param = Vec::new();
                let mut override_fields = Vec::new();
                let mut override_slots = Vec::new();

                for (idx, field) in unnamed.unnamed.iter().enumerate() {
                    let var_name = format_ident!("_{}", idx);
                    patterns.push(quote! { #var_name });

                    bound_idents.push(quote! { #var_name });
                    bound_names.push(var_name.to_string());
                    bound_is_option.push(crate::derives::template::is_option(&field.ty));
                    bound_types.push(field.ty.clone());
                    bound_skip_param.push(Self::has_skip_param_attr(&field.attrs));

                    if Self::has_source_attr(&field.attrs) {
                        source_idx = Some(quote! { #var_name });
                        source_pos = Some(idx);
                    }

                    if Self::has_from_attr(&field.attrs) || implicit_catch_all {
                        from_fields.push((field.ty.clone(), true, None));
                        implied_source.get_or_insert(quote! { #var_name });
                        implied_source_pos.get_or_insert(idx);
                    }

                    if Self::has_overrides_attr(&field.attrs)? || implicit_catch_all {
                        override_fields.push(quote! { #var_name });
                        override_slots.push(quote! { #var_name });
                    } else {
                        override_slots.push(quote! { _ });
                    }
                }

                let source_idx = source_idx.or(implied_source);
                let pattern = quote! { ( #(#patterns),* ) };
                let wildcard_pattern = pattern.clone();
                let source_pattern = source_pos.or(implied_source_pos).map(|src| {
                    let slots = (0..unnamed.unnamed.len()).map(|idx| {
                        if idx == src {
                            let var_name = format_ident!("_{}", idx);
                            quote! { #var_name }
                        } else {
                            quote! { _ }
                        }
                    });
                    quote! { ( #(#slots),* ) }
                });
                let override_pattern = if override_fields.is_empty() {
                    None
                } else {
                    Some(quote! { ( #(#override_slots),* ) })
                };
                FieldAnalysis {
                    pattern,
                    wildcard_pattern,
                    source_field: source_idx,
                    source_pattern,
                    from_fields,
                    bound_idents,
                    bound_names,
                    bound_is_option,
                    bound_types,
                    bound_skip_param,
                    override_fields,
                    override_pattern,
                }
            }
            Fields::Named(named) => {
                let mut source_field = None;
                // `#[from]` implies `#[source]`; an explicit `#[source]` wins.
                let mut implied_source = None;
                let mut field_names = Vec::new();
                let mut from_fields = Vec::new();
                let mut bound_idents = Vec::new();
                let mut bound_names = Vec::new();
                let mut bound_is_option = Vec::new();
                let mut bound_types = Vec::new();
                let mut bound_skip_param = Vec::new();
                let mut override_fields = Vec::new();

                for field in &named.named {
                    let name = field.ident.as_ref().unwrap();
                    field_names.push(name.clone());

                    bound_idents.push(quote! { #name });
                    bound_names.push(name.to_string());
                    bound_is_option.push(crate::derives::template::is_option(&field.ty));
                    bound_types.push(field.ty.clone());
                    bound_skip_param.push(Self::has_skip_param_attr(&field.attrs));

                    if Self::has_source_attr(&field.attrs) {
                        source_field = Some(quote! { #name });
                    }

                    if Self::has_from_attr(&field.attrs) || implicit_catch_all {
                        from_fields.push((field.ty.clone(), false, Some(name.clone())));
                        implied_source.get_or_insert(quote! { #name });
                    }

                    if Self::has_overrides_attr(&field.attrs)? || implicit_catch_all {
                        override_fields.push(quote! { #name });
                    }
                }

                let source_field = source_field.or(implied_source);
                let pattern = quote! { { #(#field_names),*, .. } };
                let wildcard_pattern = quote! { { .. } };
                let source_pattern = source_field.as_ref().map(|name| quote! { { #name, .. } });
                let override_pattern = if override_fields.is_empty() {
                    None
                } else {
                    Some(quote! { { #(#override_fields),*, .. } })
                };
                FieldAnalysis {
                    pattern,
                    wildcard_pattern,
                    source_field,
                    source_pattern,
                    from_fields,
                    bound_idents,
                    bound_names,
                    bound_is_option,
                    bound_types,
                    bound_skip_param,
                    override_fields,
                    override_pattern,
                }
            }
        })
    }
}
