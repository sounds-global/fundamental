//! Token-stream expansion for `#[derive(Error)]`.

use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{Data, DeriveInput};

use super::{
    ErrorAttr, FieldAnalysis, FromFieldInfo, Interpolate, VariantInfo,
    generate_unauthenticated_impl,
};

#[expect(
    clippy::too_many_lines,
    clippy::cognitive_complexity,
    clippy::excessive_nesting,
    reason = "readability baseline"
)]
pub(super) fn error_impl(input: &DeriveInput) -> syn::Result<TokenStream2> {
    let core = crate::derives::core_path();
    let name = &input.ident;
    let generics = &input.generics;

    // The Error trait requires `'static`; generic lifetimes cannot satisfy it.
    if let Some(lt) = generics.lifetimes().next() {
        return Err(syn::Error::new_spanned(
            lt,
            "Error cannot be derived for enums with lifetime parameters: \
             the Error trait requires `'static`",
        ));
    }

    let data_enum = match &input.data {
        Data::Enum(data) => data,
        Data::Struct(_) => {
            return Err(syn::Error::new_spanned(
                input,
                "Error can only be derived for enums",
            ));
        }
        Data::Union(_) => {
            return Err(syn::Error::new_spanned(
                input,
                "Error cannot be derived for unions",
            ));
        }
    };

    // Each `#[error_into(Target)]` gets a `From<Self> for Target` that erases through `AnyError`.
    let mut error_into_targets: Vec<syn::Path> = Vec::new();
    for attr in &input.attrs {
        if !attr.path().is_ident("error_into") {
            continue;
        }
        let targets = attr.parse_args_with(
            syn::punctuated::Punctuated::<syn::Path, syn::Token![,]>::parse_terminated,
        )?;
        if targets.is_empty() {
            return Err(syn::Error::new_spanned(
                attr,
                "`#[error_into(...)]` expects at least one target error type",
            ));
        }
        error_into_targets.extend(targets);
    }

    // Generic enums stay bound-free; each impl gets the bounds a parameter's usage demands.
    let type_params: Vec<syn::Ident> = generics.type_params().map(|p| p.ident.clone()).collect();
    let mut debug_params: std::collections::HashSet<usize> = std::collections::HashSet::new();
    let mut display_params: std::collections::HashSet<usize> = std::collections::HashSet::new();
    let mut error_params: std::collections::HashSet<usize> = std::collections::HashSet::new();
    let mut override_params: std::collections::HashSet<usize> = std::collections::HashSet::new();

    fn mentions_ident(tokens: TokenStream2, ident: &syn::Ident) -> bool {
        tokens.into_iter().any(|tt| match tt {
            proc_macro2::TokenTree::Ident(i) => &i == ident,
            proc_macro2::TokenTree::Group(g) => mentions_ident(g.stream(), ident),
            _ => false,
        })
    }

    /// The index of the param `ty` is exactly, if any.
    fn as_param(ty: &syn::Type, params: &[syn::Ident]) -> Option<usize> {
        let syn::Type::Path(tp) = ty else { return None };
        if tp.qself.is_some() || tp.path.segments.len() != 1 {
            return None;
        }
        let seg = tp.path.segments.first()?;
        if !seg.arguments.is_none() {
            return None;
        }
        params.iter().position(|p| p == &seg.ident)
    }

    let mut variants_info = Vec::new();
    let mut from_impls_info: Vec<FromFieldInfo> = Vec::new();

    for variant in &data_enum.variants {
        let error_attr = ErrorAttr::parse(&variant.attrs)?;
        // An attribute-less variant whose sole field is an `AnyError` is an implicit catch-all.
        let mut analysis = FieldAnalysis::analyze(&variant.fields, !error_attr.explicit)?;

        // `#[error(transparent)]`: the single field is both `#[source]` and `#[overrides]`.
        if error_attr.transparent {
            if analysis.bound_idents.len() != 1 {
                return Err(syn::Error::new_spanned(
                    &variant.ident,
                    "#[error(transparent)] requires the variant to have exactly one field",
                ));
            }
            let field = analysis.bound_idents[0].clone();
            if analysis.source_field.is_none() {
                analysis.source_field = Some(field.clone());
                analysis.source_pattern = Some(analysis.pattern.clone());
            }
            if analysis.override_fields.is_empty() {
                analysis.override_fields = vec![field];
                analysis.override_pattern = Some(analysis.pattern.clone());
            }
        }

        if !type_params.is_empty() {
            let mut interpolated: std::collections::HashSet<usize> =
                std::collections::HashSet::new();
            let mut templates = vec![error_attr.message.as_str()];
            if let Some(ref dev) = error_attr.developer_message {
                templates.push(dev.as_str());
            }
            for template in templates {
                let (_, placeholders) = Interpolate::scan_template(template);
                for ph in &placeholders {
                    if let Some(idx) = Interpolate::resolve_placeholder(ph, &analysis.bound_names) {
                        interpolated.insert(idx);
                    }
                }
            }

            for (idx, field) in variant.fields.iter().enumerate() {
                let ty = &field.ty;
                let is_source_like = FieldAnalysis::has_source_attr(&field.attrs)
                    || FieldAnalysis::has_from_attr(&field.attrs);
                let is_override = FieldAnalysis::has_overrides_attr(&field.attrs)?;

                for (p_idx, param) in type_params.iter().enumerate() {
                    if !mentions_ident(quote! { #ty }, param) {
                        continue;
                    }
                    debug_params.insert(p_idx);
                    if interpolated.contains(&idx) {
                        display_params.insert(p_idx);
                    }
                    if as_param(ty, &type_params) == Some(p_idx) {
                        if is_source_like {
                            error_params.insert(p_idx);
                        }
                        if is_override {
                            override_params.insert(p_idx);
                        }
                    }
                }
            }
        }

        for (from_type, is_tuple, field_name) in analysis.from_fields {
            from_impls_info.push(FromFieldInfo {
                variant_ident: variant.ident.clone(),
                from_type,
                is_tuple,
                field_name,
            });
        }

        variants_info.push(VariantInfo {
            ident: variant.ident.clone(),
            attrs: error_attr,
            pattern: analysis.pattern,
            wildcard_pattern: analysis.wildcard_pattern,
            source_field: analysis.source_field,
            source_pattern: analysis.source_pattern,
            bound_idents: analysis.bound_idents,
            bound_names: analysis.bound_names,
            bound_is_option: analysis.bound_is_option,
            bound_types: analysis.bound_types,
            bound_skip_param: analysis.bound_skip_param,
            override_fields: analysis.override_fields,
            override_pattern: analysis.override_pattern,
        });
    }

    // One code names one failure; transparent variants and `UNKNOWN` carry none of their own.
    let mut codes: std::collections::BTreeMap<&str, &syn::Ident> =
        std::collections::BTreeMap::new();
    for v in &variants_info {
        if v.attrs.transparent || v.attrs.code == "UNKNOWN" {
            continue;
        }
        if let Some(first) = codes.insert(v.attrs.code.as_str(), &v.ident) {
            return Err(syn::Error::new(
                v.ident.span(),
                format!(
                    "error code `{}` is already used by `{first}`: one code names one failure",
                    v.attrs.code
                ),
            ));
        }
    }

    // One augmented where clause for every impl: the union of all needs.
    let mut augmented_generics = generics.clone();
    if !type_params.is_empty() {
        let preds = &mut augmented_generics.make_where_clause().predicates;
        for (p_idx, param) in type_params.iter().enumerate() {
            // Supertraits of the Error trait apply to every param, phantom ones too.
            let mut bounds: Vec<TokenStream2> =
                vec![quote!(core::marker::Send), quote!(core::marker::Sync)];
            if error_params.contains(&p_idx) {
                bounds.push(quote!(core::error::Error));
            } else {
                if debug_params.contains(&p_idx) {
                    bounds.push(quote!(core::fmt::Debug));
                }
                if display_params.contains(&p_idx) {
                    bounds.push(quote!(core::fmt::Display));
                }
            }
            if override_params.contains(&p_idx) {
                bounds.push(quote!(#core::error::ErrorOverride));
            }
            preds.push(syn::parse_quote! { #param: #(#bounds)+* + 'static });
        }
    }
    let (impl_generics, ty_generics, where_clause) = augmented_generics.split_for_impl();

    let error_code_arms: Vec<_> = variants_info
        .iter()
        .map(|v| {
            let ident = &v.ident;
            let pattern = &v.wildcard_pattern;
            let code = &v.attrs.code;
            quote! { Self::#ident #pattern => #code }
        })
        .collect();

    // Routed through `i18n::translate`; the fallback keeps the exact `format!` interpolation.
    let mut display_message_arms: Vec<TokenStream2> = Vec::new();
    for v in &variants_info {
        let arm = Interpolate::make_translated_message_arm(
            v.bindings(),
            &v.attrs.translation_key(),
            &v.attrs.message,
            &v.attrs.message_args,
            &core,
        )?;
        display_message_arms.push(arm);
    }

    let mut developer_message_arms: Vec<TokenStream2> = Vec::new();
    for v in &variants_info {
        if let Some(ref dev_msg) = v.attrs.developer_message {
            let arm = Interpolate::make_message_arm(v.bindings(), dev_msg, &[], true, &core)?;
            developer_message_arms.push(arm);
        } else {
            let ident = &v.ident;
            let pattern = &v.wildcard_pattern;
            developer_message_arms.push(quote! { Self::#ident #pattern => None });
        }
    }

    let http_status_arms: Vec<_> = variants_info
        .iter()
        .map(|v| {
            let ident = &v.ident;
            let pattern = &v.wildcard_pattern;
            let status = &v.attrs.status;
            quote! { Self::#ident #pattern => #status }
        })
        .collect();

    let is_retryable_arms: Vec<_> = variants_info
        .iter()
        .map(|v| {
            let ident = &v.ident;
            let pattern = &v.wildcard_pattern;
            let retryable = v.attrs.retryable;
            quote! { Self::#ident #pattern => #retryable }
        })
        .collect();

    // `try_from` keeps any field width lossless; a negative or oversized value becomes `None`.
    let retry_after_arms: Vec<_> = variants_info
        .iter()
        .map(|v| {
            let ident = &v.ident;
            match &v.attrs.retry_after {
                Some(field) => quote! {
                    Self::#ident { #field, .. } => u64::try_from(*#field).ok()
                },
                None => {
                    let pattern = &v.wildcard_pattern;
                    quote! { Self::#ident #pattern => None }
                }
            }
        })
        .collect();

    let message_numbers_arms: Vec<TokenStream2> = variants_info
        .iter()
        .map(|v| Interpolate::make_message_numbers_arm(v.bindings(), &v.attrs.message))
        .collect();

    let field_errors_arms: Vec<TokenStream2> = variants_info
        .iter()
        .map(|v| {
            let ident = &v.ident;
            let pattern = &v.wildcard_pattern;
            match &v.attrs.field {
                Some(field) => {
                    let code = &v.attrs.code;
                    quote! {
                        Self::#ident #pattern => ::std::vec![#core::validate::ValidationError::new(
                            Some(::std::string::String::from(#field)),
                            #code,
                            #core::error::Error::display_message(self).into_owned(),
                        )]
                    }
                }
                None => quote! { Self::#ident #pattern => ::std::vec::Vec::new() },
            }
        })
        .collect();

    let source_arms: Vec<_> = variants_info
            .iter()
            .map(|v| {
                let ident = &v.ident;
                if let Some(ref source_field) = v.source_field {
                    // Binding only the source field avoids `unused variable` warnings.
                    let pattern = v.source_pattern.as_ref().unwrap_or(&v.pattern);
                    quote! { Self::#ident #pattern => Some(#source_field as &(dyn std::error::Error + 'static)) }
                } else {
                    let wildcard = &v.wildcard_pattern;
                    quote! { Self::#ident #wildcard => None }
                }
            })
            .collect();

    let mut display_arms: Vec<TokenStream2> = Vec::new();
    for v in &variants_info {
        let arm = Interpolate::make_write_arm(
            v.bindings(),
            &v.attrs.message,
            &v.attrs.message_args,
            &core,
        )?;
        display_arms.push(arm);
    }

    let unauthenticated_impl = generate_unauthenticated_impl(&variants_info, data_enum);

    // Without source fields the `std::error::Error` default (`None`) applies.
    let has_any_source = variants_info.iter().any(|v| v.source_field.is_some());

    let source_impl = if has_any_source {
        quote! {
            fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
                match self {
                    #(#source_arms,) *
                }
            }
        }
    } else {
        quote! {}
    };

    // Each variant with `#[overrides]` fields gets a prelude returning the first `Some`; the declared arms stay the fallback.
    let has_any_override = variants_info.iter().any(|v| !v.override_fields.is_empty());

    // `ret` runs when an override fires and may use the binding `__v`.
    let make_override_prelude = |method: &str, ret: TokenStream2| -> TokenStream2 {
        if !has_any_override {
            return quote! {};
        }
        let method_ident = syn::Ident::new(method, proc_macro2::Span::call_site());
        let arms: Vec<TokenStream2> = variants_info
            .iter()
            .filter(|v| !v.override_fields.is_empty())
            .map(|v| {
                let ident = &v.ident;
                let pat = v
                    .override_pattern
                    .as_ref()
                    .expect("override fields imply an override pattern");
                let first = &v.override_fields[0];
                let rest = &v.override_fields[1..];
                let chain = quote! {
                    #core::error::ErrorOverride::#method_ident(#first)
                        #( .or_else(|| #core::error::ErrorOverride::#method_ident(#rest)) )*
                };
                quote! {
                    Self::#ident #pat => {
                        if let Some(__v) = #chain { #ret }
                    }
                }
            })
            .collect();

        // The catch-all is omitted when every variant is covered: it would be unreachable.
        let all_covered = variants_info.iter().all(|v| !v.override_fields.is_empty());
        let catch_all = if all_covered {
            quote! {}
        } else {
            quote! { _ => {} }
        };

        quote! {
            match self {
                #(#arms)*
                #catch_all
            }
        }
    };

    let error_code_prelude = make_override_prelude("override_error_code", quote! { return __v; });
    let display_message_prelude =
        make_override_prelude("override_display_message", quote! { return __v; });
    let developer_message_prelude =
        make_override_prelude("override_developer_message", quote! { return Some(__v); });
    let http_status_prelude =
        make_override_prelude("override_http_status_code", quote! { return __v; });
    let is_retryable_prelude =
        make_override_prelude("override_is_retryable", quote! { return __v; });
    let retry_after_prelude =
        make_override_prelude("override_retry_after_secs", quote! { return __v; });
    let message_numbers_prelude =
        make_override_prelude("override_message_numbers", quote! { return __v; });
    let field_errors_prelude = make_override_prelude(
        "override_field_errors",
        quote! { if !__v.is_empty() { return __v; } },
    );
    let display_write_prelude = make_override_prelude(
        "override_display_message",
        quote! { return write!(f, "{}", __v); },
    );

    // `exit_code` is emitted only when a field can override it; otherwise the trait default applies.
    let exit_code_impl = if has_any_override {
        let exit_code_prelude = make_override_prelude("override_exit_code", quote! { return __v; });
        quote! {
            fn exit_code(&self) -> i32 {
                #exit_code_prelude
                1
            }
        }
    } else {
        quote! {}
    };

    let from_impls: Vec<_> = from_impls_info
        .iter()
        .map(|info| {
            let variant_ident = &info.variant_ident;
            let from_type = &info.from_type;

            if info.is_tuple {
                quote! {
                    impl #impl_generics core::convert::From<#from_type> for #name #ty_generics #where_clause {
                        fn from(source: #from_type) -> Self {
                            Self::#variant_ident(source)
                        }
                    }
                }
            } else {
                let field_name = info.field_name.as_ref().expect("named field must have name");
                quote! {
                    impl #impl_generics core::convert::From<#from_type> for #name #ty_generics #where_clause {
                        fn from(source: #from_type) -> Self {
                            Self::#variant_ident { #field_name: source }
                        }
                    }
                }
            }
        })
        .collect();

    // The target needs `From<AnyError>`; without it this impl fails to compile.
    let error_into_impls: Vec<TokenStream2> = error_into_targets
        .iter()
        .map(|target| {
            quote! {
                impl #impl_generics core::convert::From<#name #ty_generics> for #target #where_clause {
                    fn from(err: #name #ty_generics) -> Self {
                        <#target as core::convert::From<#core::error::AnyError>>::from(
                            #core::error::AnyError::new(err),
                        )
                    }
                }
            }
        })
        .collect();

    let mut to_validation_arms: Vec<TokenStream2> = Vec::new();
    for v in &variants_info {
        let ident = &v.ident;
        let pattern = &v.pattern; // this binds the field idents (e.g. { field, reason, .. })
        let code = &v.attrs.code;

        // Same renderer as `display_message()`; `by_ref` because this match consumes `err`.
        let render_string = |template: &str, args: &[TokenStream2]| -> syn::Result<TokenStream2> {
            let r = Interpolate::render(v.bindings(), template, args, true, &core)?;
            if r.is_static {
                let template = &r.static_text;
                Ok(quote! { #template.to_string() })
            } else {
                let fmt_lit = syn::LitStr::new(&r.fmt, proc_macro2::Span::call_site());
                let shadows = &r.shadows;
                let args = &r.args;
                Ok(quote! { { #(#shadows)* format!(#fmt_lit #(, #args)*) } })
            }
        };
        let display_expr = render_string(&v.attrs.message, &v.attrs.message_args)?;

        let dev_branch = if let Some(ref dev_template) = v.attrs.developer_message {
            let dev_expr = render_string(dev_template, &[])?;
            quote! { Some(#dev_expr) }
        } else {
            quote! { None::<String> }
        };

        // Only interpolated fields not marked `skip_param` become params, so an unwrapped secret is never stored.
        let mut interpolated: std::collections::HashSet<usize> = std::collections::HashSet::new();
        let mut param_templates = vec![v.attrs.message.as_str()];
        if let Some(ref dev) = v.attrs.developer_message {
            param_templates.push(dev.as_str());
        }
        for template in param_templates {
            let (_, placeholders) = Interpolate::scan_template(template);
            for ph in &placeholders {
                if let Some(idx) = Interpolate::resolve_placeholder(ph, &v.bound_names) {
                    interpolated.insert(idx);
                }
            }
        }

        let mut param_chain: TokenStream2 = quote! {};
        for (idx, (name, ident)) in v.bound_names.iter().zip(v.bound_idents.iter()).enumerate() {
            if !interpolated.contains(&idx) {
                continue;
            }
            if v.bound_skip_param.get(idx).copied().unwrap_or(false) {
                continue;
            }
            let key = name.clone();
            let val = ident.clone();
            // Secret fields must be `SecretString` (Debug `[REDACTED]`) or `skip_param`.
            param_chain = quote! {
                #param_chain
                .with_param(#key, format!("{:?}", #val))
            };
        }

        // The declared `field = "name"`, else the first bound field's name, else `None`.
        let field_arg: proc_macro2::TokenStream = if let Some(field) = &v.attrs.field {
            quote! { Some(::std::string::String::from(#field)) }
        } else if v.bound_idents.is_empty() {
            quote! { None::<String> }
        } else {
            let first_ident = v.bound_idents[0].clone();
            quote! { Some(format!("{:?}", #first_ident)) }
        };

        // `#[overrides]` variants consult the override chain first, so the conversion matches the trait methods.
        let arm = if v.override_fields.is_empty() {
            quote! {
                #name::#ident #pattern => {
                    let mut ve = #core::validate::ValidationError::new(#field_arg, #code, #display_expr) #param_chain;
                    if let Some(dev) = #dev_branch {
                        ve = ve.with_developer_message(dev);
                    }
                    ve
                }
            }
        } else {
            let make_chain = |method: &str| -> TokenStream2 {
                let method_ident = syn::Ident::new(method, proc_macro2::Span::call_site());
                let first = &v.override_fields[0];
                let rest = &v.override_fields[1..];
                quote! {
                    #core::error::ErrorOverride::#method_ident(&#first)
                        #( .or_else(|| #core::error::ErrorOverride::#method_ident(&#rest)) )*
                }
            };
            let code_chain = make_chain("override_error_code");
            let message_chain = make_chain("override_display_message");
            let dev_chain = make_chain("override_developer_message");
            quote! {
                #name::#ident #pattern => {
                    let __code: String = #code_chain
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| #code.to_string());
                    let __message: String = #message_chain
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| #display_expr);
                    let __dev: Option<String> = #dev_chain
                        .map(|s| s.to_string())
                        .or_else(|| #dev_branch);
                    let mut ve = #core::validate::ValidationError::new(#field_arg, __code, __message) #param_chain;
                    if let Some(dev) = __dev {
                        ve = ve.with_developer_message(dev);
                    }
                    ve
                }
            }
        };

        to_validation_arms.push(arm);
    }

    Ok(quote! {
        impl #impl_generics #core::error::Error for #name #ty_generics #where_clause {
            fn error_code(&self) -> &'static str {
                #error_code_prelude
                match self {
                    #(#error_code_arms,) *
                }
            }

            fn display_message(&self) -> ::std::borrow::Cow<'_, str> {
                #display_message_prelude
                match self {
                    #(#display_message_arms,) *
                }
            }

            fn developer_message(&self) -> Option<::std::borrow::Cow<'_, str>> {
                #developer_message_prelude
                match self {
                    #(#developer_message_arms,) *
                }
            }

            fn http_status_code(&self) -> u16 {
                #http_status_prelude
                match self {
                    #(#http_status_arms,) *
                }
            }

            fn is_retryable(&self) -> bool {
                #is_retryable_prelude
                match self {
                    #(#is_retryable_arms,) *
                }
            }

            fn retry_after_secs(&self) -> Option<u64> {
                #retry_after_prelude
                match self {
                    #(#retry_after_arms,) *
                }
            }

            fn message_numbers(&self) -> ::std::vec::Vec<(&'static str, i64)> {
                #message_numbers_prelude
                match self {
                    #(#message_numbers_arms,) *
                }
            }

            fn field_errors(&self) -> ::std::vec::Vec<#core::validate::ValidationError> {
                #field_errors_prelude
                match self {
                    #(#field_errors_arms,) *
                }
            }

            #exit_code_impl

            #unauthenticated_impl
        }

        impl #impl_generics core::fmt::Display for #name #ty_generics #where_clause {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                #display_write_prelude
                match self {
                    #(#display_arms,) *
                }
            }
        }

        impl #impl_generics std::error::Error for #name #ty_generics #where_clause {
            #source_impl
        }

        impl #impl_generics core::convert::From<#name #ty_generics> for #core::validate::ValidationError #where_clause {
            // Every field is bound, but only template-referenced ones become params.
            #[allow(unused_variables)]
            fn from(err: #name #ty_generics) -> Self {
                match err {
                    #(#to_validation_arms,)*
                }
            }
        }

        #(#from_impls) *

        #(#error_into_impls) *
    })
}
