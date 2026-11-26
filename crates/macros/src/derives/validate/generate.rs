//! Per-field validation code generation for `#[derive(Validate)]`.

use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::Field;

use super::Validation;

pub(super) struct ValidationGenerate;

impl ValidationGenerate {
    /// One field's checks; `access` is a `&T`, for a struct field and an enum binding alike.
    pub fn field_validation(
        field: &Field,
        access: &TokenStream2,
    ) -> syn::Result<Option<TokenStream2>> {
        let field_name = field
            .ident
            .as_ref()
            .ok_or_else(|| syn::Error::new_spanned(field, "Expected named field"))?;
        let field_name_str = field_name.to_string();

        let validations = Validation::parse_attributes(field)?;

        if validations.is_empty() || validations.iter().any(|v| matches!(v, Validation::Skip)) {
            return Ok(None);
        }

        let is_option = Validation::is_option_type(&field.ty);

        let validation_calls: Vec<TokenStream2> = validations
            .iter()
            .filter_map(|v| Self::validation_call(&field_name_str, access, v, is_option))
            .collect();

        if validation_calls.is_empty() {
            return Ok(None);
        }

        Ok(Some(quote! {
            #(#validation_calls)*
        }))
    }

    /// One rule's call; a by-value rule dereferences the `&T` access.
    #[expect(
        clippy::too_many_lines,
        clippy::excessive_nesting,
        reason = "readability baseline"
    )]
    fn validation_call(
        field_name_str: &str,
        access: &TokenStream2,
        validation: &Validation,
        is_option: bool,
    ) -> Option<TokenStream2> {
        match validation {
            Validation::Required => {
                if is_option {
                    Some(quote! {
                        __ctx.field(#field_name_str).required_opt(#access);
                    })
                } else {
                    Some(quote! {
                        __ctx.field(#field_name_str).required(#access);
                    })
                }
            }
            Validation::Length { min, max } => {
                let min_val = min.unwrap_or(0);
                let max_val = max.unwrap_or(usize::MAX);

                if is_option {
                    Some(quote! {
                        if let Some(__val) = #access {
                            __ctx.field(#field_name_str).length(__val, #min_val, #max_val);
                        }
                    })
                } else {
                    Some(quote! {
                        __ctx.field(#field_name_str).length(#access, #min_val, #max_val);
                    })
                }
            }
            Validation::Range { min, max } => {
                let min_check = min.as_ref().map(|m| {
                    let m: TokenStream2 = m.parse().unwrap();
                    quote! { .min(*#access, #m) }
                });
                let max_check = max.as_ref().map(|m| {
                    let m: TokenStream2 = m.parse().unwrap();
                    quote! { .max(*#access, #m) }
                });

                if min_check.is_none() && max_check.is_none() {
                    return None;
                }

                if is_option {
                    let min_check = min.as_ref().map(|m| {
                        let m: TokenStream2 = m.parse().unwrap();
                        quote! { .min(__val, #m) }
                    });
                    let max_check = max.as_ref().map(|m| {
                        let m: TokenStream2 = m.parse().unwrap();
                        quote! { .max(__val, #m) }
                    });
                    Some(quote! {
                        if let Some(__val) = *#access {
                            __ctx.field(#field_name_str)
                                #min_check
                                #max_check;
                        }
                    })
                } else {
                    Some(quote! {
                        __ctx.field(#field_name_str)
                            #min_check
                            #max_check;
                    })
                }
            }
            Validation::Email => Some(Self::string_rule(
                field_name_str,
                access,
                is_option,
                quote! { email },
            )),
            Validation::Url => Some(Self::string_rule(
                field_name_str,
                access,
                is_option,
                quote! { url },
            )),
            Validation::Uuid => Some(Self::string_rule(
                field_name_str,
                access,
                is_option,
                quote! { uuid },
            )),
            Validation::Alphanumeric => Some(Self::string_rule(
                field_name_str,
                access,
                is_option,
                quote! { alphanumeric },
            )),
            Validation::Ascii => Some(Self::string_rule(
                field_name_str,
                access,
                is_option,
                quote! { ascii },
            )),
            Validation::Identifier => Some(Self::string_rule(
                field_name_str,
                access,
                is_option,
                quote! { identifier },
            )),
            Validation::Pattern(pattern) => {
                if is_option {
                    Some(quote! {
                        if let Some(__val) = #access {
                            __ctx.field(#field_name_str).pattern_str(__val, #pattern);
                        }
                    })
                } else {
                    Some(quote! {
                        __ctx.field(#field_name_str).pattern_str(#access, #pattern);
                    })
                }
            }
            Validation::OneOf(values) => {
                let values_array: Vec<_> = values.iter().map(|v| quote! { #v }).collect();
                if is_option {
                    Some(quote! {
                        if let Some(__val) = #access {
                            __ctx.field(#field_name_str).one_of(__val, &[#(#values_array),*]);
                        }
                    })
                } else {
                    Some(quote! {
                        __ctx.field(#field_name_str).one_of(#access, &[#(#values_array),*]);
                    })
                }
            }
            Validation::Nested => {
                if is_option {
                    Some(quote! {
                        __ctx.validate_nested_opt(#field_name_str, #access);
                    })
                } else {
                    Some(quote! {
                        __ctx.validate_nested(#field_name_str, #access);
                    })
                }
            }
            Validation::Custom(func_path) => Some(quote! {
                if let Err(__errs) = #func_path(#access) {
                    __ctx.merge_errors_with_prefix(#field_name_str, __errs);
                }
            }),
            Validation::Skip => None,
        }
    }

    /// The rules that differ only in the `ValidationContext` method they call.
    fn string_rule(
        field_name_str: &str,
        access: &TokenStream2,
        is_option: bool,
        method: TokenStream2,
    ) -> TokenStream2 {
        if is_option {
            quote! {
                if let Some(__val) = #access {
                    __ctx.field(#field_name_str).#method(__val);
                }
            }
        } else {
            quote! {
                __ctx.field(#field_name_str).#method(#access);
            }
        }
    }
}
