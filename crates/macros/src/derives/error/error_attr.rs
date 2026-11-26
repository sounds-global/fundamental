//! Parsing of per-variant `#[error(...)]` attributes.

use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::parse::ParseStream;
use syn::{Attribute, Expr, Ident, Lit, LitBool, LitStr, Token};

pub(super) struct ErrorAttr {
    pub code: String,
    pub message: String,
    pub developer_message: Option<String>,
    /// Trailing format arguments for `message`, from the positional form.
    pub message_args: Vec<TokenStream2>,
    /// The status expression: an integer literal or a `u16` const path.
    pub status: TokenStream2,
    /// The status when a literal, to find the `status = 401` variant for `unauthenticated()`.
    pub status_lit: Option<u16>,
    pub retryable: bool,
    /// The field holding the retry delay, stamped as `Retry-After`.
    pub retry_after: Option<Ident>,
    /// An explicit `translate = "..."` key; defaults to one from the code.
    pub translate: Option<String>,
    /// The input field this variant refuses, from `#[error(field = "email")]`.
    pub field: Option<String>,
    pub transparent: bool,
    /// Whether an `#[error(...)]` was present; without one a variant may be the implicit catch-all.
    pub explicit: bool,
}

impl Default for ErrorAttr {
    fn default() -> Self {
        Self {
            code: "UNKNOWN".to_string(),
            message: "An error occurred".to_string(),
            developer_message: None,
            message_args: Vec::new(),
            status: quote! { 500 },
            status_lit: Some(500),
            retryable: false,
            retry_after: None,
            translate: None,
            field: None,
            transparent: false,
            explicit: false,
        }
    }
}

impl ErrorAttr {
    pub(super) fn parse(attrs: &[Attribute]) -> syn::Result<ErrorAttr> {
        let mut result = ErrorAttr::default();

        for attr in attrs {
            if !attr.path().is_ident("error") {
                continue;
            }
            result.explicit = true;
            attr.parse_args_with(|input: ParseStream| Self::parse_args(input, &mut result))?;
        }

        Ok(result)
    }

    /// The explicit `translate` key, else `error.{lowercased_code}`.
    pub(super) fn translation_key(&self) -> String {
        fundamental_i18n_keys::error_key(&self.code, self.translate.as_deref())
    }

    /// Parses one `#[error(...)]`: a leading message literal, `key = value` metadata, bare `retryable`/`transparent`, else a format argument.
    #[expect(clippy::excessive_nesting, reason = "readability baseline")]
    fn parse_args(input: ParseStream, result: &mut ErrorAttr) -> syn::Result<()> {
        let mut first = true;
        while !input.is_empty() {
            if input.peek(syn::Ident) && input.peek2(Token![=]) {
                let key: syn::Ident = input.parse()?;
                input.parse::<Token![=]>()?;
                match key.to_string().as_str() {
                    "code" => result.code = input.parse::<LitStr>()?.value(),
                    "message" => result.message = input.parse::<LitStr>()?.value(),
                    "developer_message" => {
                        result.developer_message = Some(input.parse::<LitStr>()?.value())
                    }
                    "status" => {
                        let expr: Expr = input.parse()?;
                        result.status_lit = int_literal_u16(&expr);
                        result.status = quote! { #expr };
                    }
                    "retryable" => result.retryable = input.parse::<LitBool>()?.value(),
                    "retry_after" => result.retry_after = Some(input.parse::<Ident>()?),
                    "translate" => result.translate = Some(input.parse::<LitStr>()?.value()),
                    "field" => result.field = Some(input.parse::<LitStr>()?.value()),
                    other => {
                        return Err(syn::Error::new_spanned(
                            key,
                            format!("unknown error attribute: {other}"),
                        ));
                    }
                }
            } else if first && input.peek(LitStr) {
                result.message = input.parse::<LitStr>()?.value();
            } else if is_flag(input, "retryable") {
                input.parse::<syn::Ident>()?;
                result.retryable = true;
            } else if is_flag(input, "transparent") {
                input.parse::<syn::Ident>()?;
                result.transparent = true;
            } else {
                let expr: Expr = input.parse()?;
                result.message_args.push(quote! { #expr });
            }

            first = false;
            if input.is_empty() {
                break;
            }
            input.parse::<Token![,]>()?;
        }
        Ok(())
    }
}

/// Whether the next token is the bare identifier `name`, not `name = ...`.
fn is_flag(input: ParseStream, name: &str) -> bool {
    if !input.peek(syn::Ident) || input.peek2(Token![=]) {
        return false;
    }
    input
        .fork()
        .parse::<syn::Ident>()
        .map(|id| id == name)
        .unwrap_or(false)
}

fn int_literal_u16(expr: &Expr) -> Option<u16> {
    if let Expr::Lit(lit) = expr
        && let Lit::Int(int) = &lit.lit
    {
        return int.base10_parse::<u16>().ok();
    }
    None
}
