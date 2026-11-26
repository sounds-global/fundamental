//! Per-variant field data collected by the `#[derive(Error)]` expansion.

use super::ErrorAttr;
use crate::derives::template::VariantBindings;

use proc_macro2::TokenStream as TokenStream2;
use syn::{Ident, Type};

pub(super) struct VariantInfo {
    pub ident: Ident,
    pub attrs: ErrorAttr,
    /// Destructures the fields (`{ id, .. }` or `(_0)`).
    pub pattern: TokenStream2,
    /// Binds no field (`{ .. }`).
    pub wildcard_pattern: TokenStream2,
    /// The `#[source]` field's index, if any.
    pub source_field: Option<TokenStream2>,
    /// Binds only the `#[source]` field.
    pub source_pattern: Option<TokenStream2>,
    /// Bound identifiers, parallel to `bound_names`.
    pub bound_idents: Vec<TokenStream2>,
    pub bound_names: Vec<String>,
    pub bound_is_option: Vec<bool>,
    pub bound_types: Vec<Type>,
    /// Whether each bound field is `#[error(skip_param)]`.
    pub bound_skip_param: Vec<bool>,
    /// Bindings of `#[overrides]` fields, in declaration order.
    pub override_fields: Vec<TokenStream2>,
    /// Binds only the `#[overrides]` fields, if any.
    pub override_pattern: Option<TokenStream2>,
}

impl VariantInfo {
    /// The view template interpolation reads.
    pub(super) fn bindings(&self) -> VariantBindings<'_> {
        VariantBindings {
            ident: &self.ident,
            bound_idents: &self.bound_idents,
            bound_names: &self.bound_names,
            bound_is_option: &self.bound_is_option,
            bound_types: &self.bound_types,
            wildcard_pattern: &self.wildcard_pattern,
        }
    }
}
