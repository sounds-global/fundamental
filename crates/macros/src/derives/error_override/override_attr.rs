//! Parsing of the variant-level `#[overrides(...)]` attribute.

use syn::{Attribute, LitStr, Meta};

/// A variant's `#[overrides(...)]` values; `None` defers to the declared value.
#[derive(Default)]
pub(super) struct OverrideAttr {
    pub code: Option<String>,
    pub message: Option<String>,
    pub developer_message: Option<String>,
    pub status: Option<u16>,
    pub retryable: Option<bool>,
    pub exit_code: Option<i32>,
}

impl OverrideAttr {
    fn is_empty(&self) -> bool {
        self.code.is_none()
            && self.message.is_none()
            && self.developer_message.is_none()
            && self.status.is_none()
            && self.retryable.is_none()
            && self.exit_code.is_none()
    }

    /// The variant's `#[overrides(...)]`, if any; a bare or empty one is an error, since it would do nothing.
    #[expect(
        clippy::too_many_lines,
        clippy::excessive_nesting,
        reason = "readability baseline"
    )]
    pub(super) fn parse(attrs: &[Attribute]) -> syn::Result<Option<OverrideAttr>> {
        let mut result = OverrideAttr::default();
        let mut found = false;

        for attr in attrs {
            if !attr.path().is_ident("overrides") {
                continue;
            }
            found = true;

            if matches!(attr.meta, Meta::Path(_)) {
                return Err(syn::Error::new_spanned(
                    attr,
                    "expected at least one override value, e.g. `#[overrides(status = 409)]`",
                ));
            }

            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("code") {
                    let value: LitStr = meta.value()?.parse()?;
                    result.code = Some(value.value());
                } else if meta.path.is_ident("message") {
                    let value: LitStr = meta.value()?.parse()?;
                    result.message = Some(value.value());
                } else if meta.path.is_ident("developer_message") {
                    let value: LitStr = meta.value()?.parse()?;
                    result.developer_message = Some(value.value());
                } else if meta.path.is_ident("status") {
                    let value: syn::LitInt = meta.value()?.parse()?;
                    result.status = Some(value.base10_parse()?);
                } else if meta.path.is_ident("retryable") {
                    // Bare `retryable` is `Some(true)`; `retryable = false` overrides to false.
                    if meta.input.peek(syn::Token![=]) {
                        let value: syn::LitBool = meta.value()?.parse()?;
                        result.retryable = Some(value.value());
                    } else {
                        result.retryable = Some(true);
                    }
                } else if meta.path.is_ident("exit_code") {
                    let value: syn::LitInt = meta.value()?.parse()?;
                    result.exit_code = Some(value.base10_parse()?);
                } else {
                    return Err(meta.error(format!(
                        "unknown override attribute: {}",
                        meta.path
                            .get_ident()
                            .map(|i| i.to_string())
                            .unwrap_or_default()
                    )));
                }
                Ok(())
            })?;
        }

        if !found {
            return Ok(None);
        }
        if result.is_empty() {
            return Err(syn::Error::new_spanned(
                attrs
                    .iter()
                    .find(|a| a.path().is_ident("overrides"))
                    .expect("found above"),
                "expected at least one override value, e.g. `#[overrides(status = 409)]`",
            ));
        }
        Ok(Some(result))
    }
}
