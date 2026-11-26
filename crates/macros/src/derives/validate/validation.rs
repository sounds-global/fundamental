//! Parsing of `#[validate(...)]` attributes into validation rules.

use syn::{Expr, Field, Ident, Lit, Meta, Token, Type, punctuated::Punctuated};

use super::{LengthArgs, OneOfArgs, RangeArgs};

#[derive(Debug)]
pub(super) enum Validation {
    Required,
    Length {
        min: Option<usize>,
        max: Option<usize>,
    },
    Range {
        min: Option<String>,
        max: Option<String>,
    },
    Email,
    Url,
    Uuid,
    Pattern(String),
    Alphanumeric,
    Ascii,
    Identifier,
    OneOf(Vec<String>),
    Nested,
    Custom(syn::Path),
    Skip,
}

impl Validation {
    pub fn parse_attributes(field: &Field) -> syn::Result<Vec<Validation>> {
        let mut validations = Vec::new();

        for attr in &field.attrs {
            if !attr.path().is_ident("validate") {
                continue;
            }

            let nested = attr.parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)?;

            for meta in nested {
                let validation = Self::parse_meta(&meta)?;
                validations.push(validation);
            }
        }

        Ok(validations)
    }

    pub fn is_option_type(ty: &Type) -> bool {
        if let Type::Path(type_path) = ty
            && let Some(segment) = type_path.path.segments.last()
        {
            return segment.ident == "Option";
        }
        false
    }
}

impl Validation {
    #[expect(
        clippy::too_many_lines,
        clippy::excessive_nesting,
        reason = "readability baseline"
    )]
    fn parse_meta(meta: &Meta) -> syn::Result<Validation> {
        match meta {
            Meta::Path(path) => {
                let ident = path
                    .get_ident()
                    .ok_or_else(|| syn::Error::new_spanned(path, "Expected identifier"))?;

                match ident.to_string().as_str() {
                    "required" => Ok(Validation::Required),
                    "email" => Ok(Validation::Email),
                    "url" => Ok(Validation::Url),
                    "uuid" => Ok(Validation::Uuid),
                    "alphanumeric" => Ok(Validation::Alphanumeric),
                    "ascii" => Ok(Validation::Ascii),
                    "identifier" => Ok(Validation::Identifier),
                    "nested" => Ok(Validation::Nested),
                    "skip" => Ok(Validation::Skip),
                    other => Err(Self::unknown_validation_rule_error(ident, other)),
                }
            }
            Meta::List(list) => {
                let ident = list
                    .path
                    .get_ident()
                    .ok_or_else(|| syn::Error::new_spanned(&list.path, "Expected identifier"))?;

                match ident.to_string().as_str() {
                    "length" => Self::parse_length(list),
                    "range" => Self::parse_range(list),
                    "one_of" => Self::parse_one_of(list),
                    other => Err(Self::unknown_validation_rule_error(ident, other)),
                }
            }
            Meta::NameValue(nv) => {
                let ident = nv
                    .path
                    .get_ident()
                    .ok_or_else(|| syn::Error::new_spanned(&nv.path, "Expected identifier"))?;

                match ident.to_string().as_str() {
                    "custom" => {
                        let func_name = match &nv.value {
                            Expr::Lit(lit) => match &lit.lit {
                                // Parsed here, at the literal's span, so a non-path name is a compile error rather than a rule silently dropped.
                                Lit::Str(s) => s.parse::<syn::Path>().map_err(|_| {
                                    syn::Error::new_spanned(
                                        &nv.value,
                                        "`custom` names a function by its path, e.g. \"check\" or \"crate::rules::check\"",
                                    )
                                })?,
                                _ => {
                                    return Err(syn::Error::new_spanned(
                                        &nv.value,
                                        "Expected string literal",
                                    ));
                                }
                            },
                            _ => {
                                return Err(syn::Error::new_spanned(
                                    &nv.value,
                                    "Expected string literal",
                                ));
                            }
                        };
                        Ok(Validation::Custom(func_name))
                    }
                    "pattern" => {
                        let pattern = match &nv.value {
                            Expr::Lit(lit) => match &lit.lit {
                                Lit::Str(s) => s.value(),
                                _ => {
                                    return Err(syn::Error::new_spanned(
                                        &nv.value,
                                        "Expected string literal for pattern",
                                    ));
                                }
                            },
                            _ => {
                                return Err(syn::Error::new_spanned(
                                    &nv.value,
                                    "Expected string literal for pattern",
                                ));
                            }
                        };
                        Ok(Validation::Pattern(pattern))
                    }
                    other => Err(Self::unknown_validation_rule_error(ident, other)),
                }
            }
        }
    }

    fn parse_length(list: &syn::MetaList) -> syn::Result<Validation> {
        let args: LengthArgs = syn::parse2(list.tokens.clone())?;
        Ok(Validation::Length {
            min: args.min,
            max: args.max,
        })
    }

    fn parse_range(list: &syn::MetaList) -> syn::Result<Validation> {
        let args: RangeArgs = syn::parse2(list.tokens.clone())?;
        Ok(Validation::Range {
            min: args.min,
            max: args.max,
        })
    }

    fn parse_one_of(list: &syn::MetaList) -> syn::Result<Validation> {
        let args: OneOfArgs = syn::parse2(list.tokens.clone())?;
        Ok(Validation::OneOf(args.values))
    }
}

impl Validation {
    /// An error for an unknown rule, listing the known ones.
    fn unknown_validation_rule_error(ident: &Ident, rule: &str) -> syn::Error {
        syn::Error::new_spanned(
            ident,
            format!(
                "Unknown validation rule '{}'.\n\n\
                 Available rules:\n\n\
                 Simple (no arguments):\n  \
                 required, email, url, uuid, alphanumeric, ascii, identifier, nested, skip\n\n\
                 With parameters:\n  \
                 length(min = N, max = M), range(min = N, max = M),\n  \
                 pattern = \"regex\", one_of(\"a\", \"b\"), custom = \"fn_name\"",
                rule
            ),
        )
    }
}
