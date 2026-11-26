//! Message-template interpolation for `#[derive(Error)]`.

use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{Ident, LitStr, Type};

/// The slice of a variant that template interpolation needs.
#[derive(Clone, Copy)]
pub(crate) struct VariantBindings<'a> {
    pub ident: &'a Ident,
    /// Bound identifiers, parallel to `bound_names`.
    pub bound_idents: &'a [TokenStream2],
    pub bound_names: &'a [String],
    pub bound_is_option: &'a [bool],
    /// Each bound field's type, so numeric fields pass as typed args.
    pub bound_types: &'a [Type],
    /// A pattern binding no fields (`{ .. }` / `(..)`).
    pub wildcard_pattern: &'a TokenStream2,
}

/// Whether a type is syntactically `Option<...>`, under any of its paths.
pub(crate) fn is_option(ty: &Type) -> bool {
    let syn::Type::Path(type_path) = ty else {
        return false;
    };
    if type_path.qself.is_some() {
        return false;
    }
    let segments = &type_path.path.segments;
    let Some(last) = segments.last() else {
        return false;
    };
    if last.ident != "Option" || !matches!(last.arguments, syn::PathArguments::AngleBracketed(_)) {
        return false;
    }
    let prefix: Vec<String> = segments
        .iter()
        .take(segments.len() - 1)
        .map(|s| s.ident.to_string())
        .collect();
    matches!(
        prefix.iter().map(String::as_str).collect::<Vec<_>>()[..],
        [] | ["option"] | ["std", "option"] | ["core", "option"]
    )
}

/// Whether a bound field reaches the translator as a typed number, for CLDR plurals.
enum NumericKind {
    Int,
    Float,
}

/// The numeric kind of the field at `idx`; `None` for `Option` fields, which render as text.
fn numeric_kind(v: VariantBindings, idx: usize) -> Option<NumericKind> {
    if v.bound_is_option[idx] {
        return None;
    }
    let syn::Type::Path(tp) = &v.bound_types[idx] else {
        return None;
    };
    match tp.path.segments.last()?.ident.to_string().as_str() {
        "i8" | "i16" | "i32" | "i64" | "u8" | "u16" | "u32" | "u64" | "usize" | "isize" => {
            Some(NumericKind::Int)
        }
        "f32" | "f64" => Some(NumericKind::Float),
        _ => None,
    }
}

/// Template to match-arm code; the template goes verbatim to `write!`, with only `{0}` rewritten to `{_0}`.
pub(crate) struct Interpolate;

/// A template rendered against a variant.
pub(crate) struct Rendered {
    /// Rewritten format string (`{0}` to `{_0}`).
    pub fmt: String,
    /// Trailing format-argument expressions, verbatim.
    pub args: Vec<TokenStream2>,
    /// `OptDisplay` shadows for referenced `Option` fields.
    pub shadows: Vec<TokenStream2>,
    /// Positions in `bound_names` the match pattern must bind.
    pub bound_positions: Vec<usize>,
    /// Whether the template has no placeholders and no args.
    pub is_static: bool,
    /// What a static template's fallback writes.
    pub static_text: String,
}

impl Interpolate {
    /// The rewritten template and its referenced names, read as the messages build reads them.
    pub(crate) fn scan_template(template: &str) -> (String, Vec<String>) {
        fundamental_i18n_keys::scan_template(template)
    }

    /// The index in `bound_names` of `_0`, `0` or `arg0` forms of a name.
    pub(crate) fn resolve_placeholder(ph: &str, bound_names: &[String]) -> Option<usize> {
        bound_names.iter().position(|n| n == ph).or_else(|| {
            let num = ph.strip_prefix("arg").unwrap_or(ph);
            num.parse::<usize>().ok().and_then(|num| {
                let alt = format!("_{num}");
                bound_names.iter().position(|n| n == &alt)
            })
        })
    }

    /// Whether `tokens` mention `name`, in nested groups too.
    fn tokens_mention(tokens: &TokenStream2, name: &str) -> bool {
        tokens.clone().into_iter().any(|tt| match tt {
            proc_macro2::TokenTree::Ident(i) => i == name,
            proc_macro2::TokenTree::Group(g) => Self::tokens_mention(&g.stream(), name),
            _ => false,
        })
    }

    /// Renders a template against a variant; `by_ref` is true for the by-value `ValidationError` conversion.
    #[expect(clippy::excessive_nesting, reason = "readability baseline")]
    pub(crate) fn render(
        v: VariantBindings,
        template: &str,
        args: &[TokenStream2],
        by_ref: bool,
        core: &TokenStream2,
    ) -> syn::Result<Rendered> {
        let (fmt, referenced) = Self::scan_template(template);

        let mut template_positions: Vec<usize> = Vec::new();
        for name in &referenced {
            match Self::resolve_placeholder(name, v.bound_names) {
                Some(idx) => {
                    if !template_positions.contains(&idx) {
                        template_positions.push(idx);
                    }
                }
                None => {
                    return Err(syn::Error::new_spanned(
                        v.ident,
                        format!(
                            "unknown placeholder '{{{}}}' in message for variant '{}'",
                            name, v.ident
                        ),
                    ));
                }
            }
        }

        // Fields used only by a trailing argument are bound but not shadowed.
        let mut bound_positions = template_positions.clone();
        for arg in args {
            for (idx, name) in v.bound_names.iter().enumerate() {
                if Self::tokens_mention(arg, name) && !bound_positions.contains(&idx) {
                    bound_positions.push(idx);
                }
            }
        }
        bound_positions.sort_unstable();

        let shadows: Vec<TokenStream2> = template_positions
            .iter()
            .filter(|&&idx| v.bound_is_option[idx])
            .map(|&idx| {
                let ident = &v.bound_idents[idx];
                let name = &v.bound_names[idx];
                let target = if by_ref {
                    quote! { &#ident }
                } else {
                    quote! { #ident }
                };
                quote! { let #ident = #core::error::OptDisplay::new(#target, #name); }
            })
            .collect();

        let is_static = template_positions.is_empty() && args.is_empty();

        Ok(Rendered {
            fmt,
            args: args.to_vec(),
            shadows,
            bound_positions,
            is_static,
            static_text: template.to_owned(),
        })
    }

    /// A match pattern binding exactly `positions`.
    #[expect(clippy::excessive_nesting, reason = "readability baseline")]
    pub(crate) fn binding_pattern(v: VariantBindings, positions: &[usize]) -> TokenStream2 {
        let is_named = v.bound_names.iter().any(|n| !n.starts_with('_'));
        if is_named {
            let idents: Vec<&TokenStream2> =
                positions.iter().map(|&i| &v.bound_idents[i]).collect();
            quote! { { #(#idents),* , .. } }
        } else {
            let slots: Vec<TokenStream2> = (0..v.bound_names.len())
                .map(|i| {
                    if positions.contains(&i) {
                        v.bound_idents[i].clone()
                    } else {
                        quote! { _ }
                    }
                })
                .collect();
            quote! { ( #(#slots),* ) }
        }
    }

    /// A `display_message` or (with `as_option`) `developer_message` arm; static messages borrow.
    pub(crate) fn make_message_arm(
        v: VariantBindings,
        template: &str,
        args: &[TokenStream2],
        as_option: bool,
        core: &TokenStream2,
    ) -> syn::Result<TokenStream2> {
        let ident = &v.ident;
        let r = Self::render(v, template, args, false, core)?;

        if r.is_static {
            let wildcard = &v.wildcard_pattern;
            let template = &r.static_text;
            let body = if as_option {
                quote! { Some(::core::convert::Into::into(#template)) }
            } else {
                quote! { ::core::convert::Into::into(#template) }
            };
            return Ok(quote! { Self::#ident #wildcard => #body });
        }

        let pattern = Self::binding_pattern(v, &r.bound_positions);
        let fmt_lit = LitStr::new(&r.fmt, proc_macro2::Span::call_site());
        let shadows = &r.shadows;
        let args = &r.args;
        let formatted = quote! { ::core::convert::Into::into(format!(#fmt_lit #(, #args)*)) };
        let body = if as_option {
            quote! { Some(#formatted) }
        } else {
            formatted
        };
        Ok(quote! {
            Self::#ident #pattern => { #(#shadows)* #body }
        })
    }

    /// A `display_message` arm through `i18n::translate`, falling back to the exact `format!` rendering.
    pub(crate) fn make_translated_message_arm(
        v: VariantBindings,
        key: &str,
        template: &str,
        args: &[TokenStream2],
        core: &TokenStream2,
    ) -> syn::Result<TokenStream2> {
        let ident = &v.ident;
        let r = Self::render(v, template, args, false, core)?;
        let key_lit = LitStr::new(key, proc_macro2::Span::call_site());

        if r.is_static {
            let wildcard = &v.wildcard_pattern;
            let template = &r.static_text;
            return Ok(quote! {
                Self::#ident #wildcard => #core::i18n::translate(
                    #key_lit,
                    &[],
                    || ::core::convert::Into::into(#template),
                )
            });
        }

        let pattern = Self::binding_pattern(v, &r.bound_positions);
        let fmt_lit = LitStr::new(&r.fmt, proc_macro2::Span::call_site());
        let shadows = &r.shadows;

        // Every named placeholder becomes a translator argument, rendered with its own spec.
        let named_args: Vec<TokenStream2> = fundamental_i18n_keys::named_placeholders(template)
            .into_iter()
            .filter_map(|(name, spec)| {
                let idx = Self::resolve_placeholder(&name, v.bound_names)?;
                let binding = &v.bound_idents[idx];
                let name_lit = fundamental_i18n_keys::argument_name(&v.bound_names[idx]);
                let value = match spec {
                    // Typed numbers for CLDR plurals; otherwise lazy `Display`.
                    None => match numeric_kind(v, idx) {
                        Some(NumericKind::Int) => {
                            quote! { #core::i18n::ArgValue::Int((*#binding) as i64) }
                        }
                        Some(NumericKind::Float) => {
                            quote! { #core::i18n::ArgValue::Float((*#binding) as f64) }
                        }
                        None => quote! { #core::i18n::ArgValue::display(&#binding) },
                    },
                    // Rendered with the exact spec, as the fallback does.
                    Some(spec) => {
                        let fmt =
                            LitStr::new(&format!("{{:{spec}}}"), proc_macro2::Span::call_site());
                        quote! { #core::i18n::ArgValue::owned(format!(#fmt, #binding)) }
                    }
                };
                Some(quote! { (#name_lit, #value) })
            })
            .collect();

        let fallback = Self::fallback(&r, &fmt_lit);
        Ok(quote! {
            Self::#ident #pattern => {
                #(#shadows)*
                #core::i18n::translate(
                    #key_lit,
                    &[ #(#named_args),* ],
                    || #fallback,
                )
            }
        })
    }

    /// What a message writes with no catalog entry.
    fn fallback(r: &Rendered, fmt_lit: &LitStr) -> TokenStream2 {
        if r.is_static {
            let template = &r.static_text;
            quote! { ::core::convert::Into::into(#template) }
        } else {
            let fmt_args = &r.args;
            quote! { ::core::convert::Into::into(format!(#fmt_lit #(, #fmt_args)*)) }
        }
    }

    /// A `message_numbers` arm: each plain `{name}` over an integer field, as the translator gets it.
    pub(crate) fn make_message_numbers_arm(v: VariantBindings, template: &str) -> TokenStream2 {
        let ident = &v.ident;
        let mut positions: Vec<usize> = Vec::new();
        for (name, spec) in fundamental_i18n_keys::named_placeholders(template) {
            if spec.is_some() {
                continue;
            }
            let Some(idx) = Self::resolve_placeholder(&name, v.bound_names) else {
                continue;
            };
            if matches!(numeric_kind(v, idx), Some(NumericKind::Int)) && !positions.contains(&idx) {
                positions.push(idx);
            }
        }
        if positions.is_empty() {
            let wildcard = &v.wildcard_pattern;
            return quote! { Self::#ident #wildcard => ::std::vec::Vec::new() };
        }
        let pattern = Self::binding_pattern(v, &positions);
        let pairs: Vec<TokenStream2> = positions
            .iter()
            .map(|&idx| {
                let binding = &v.bound_idents[idx];
                let name = fundamental_i18n_keys::argument_name(&v.bound_names[idx]);
                quote! { (#name, (*#binding) as i64) }
            })
            .collect();
        quote! { Self::#ident #pattern => ::std::vec![ #(#pairs),* ] }
    }

    pub(crate) fn make_write_arm(
        v: VariantBindings,
        template: &str,
        args: &[TokenStream2],
        core: &TokenStream2,
    ) -> syn::Result<TokenStream2> {
        let ident = &v.ident;
        let r = Self::render(v, template, args, false, core)?;

        if r.is_static {
            let wildcard = &v.wildcard_pattern;
            let template = &r.static_text;
            return Ok(quote! { Self::#ident #wildcard => write!(f, "{}", #template) });
        }

        let pattern = Self::binding_pattern(v, &r.bound_positions);
        let fmt_lit = LitStr::new(&r.fmt, proc_macro2::Span::call_site());
        let shadows = &r.shadows;
        let args = &r.args;
        Ok(quote! {
            Self::#ident #pattern => { #(#shadows)* write!(f, #fmt_lit #(, #args)*) }
        })
    }
}
