//! The source scan [`ErrorCodeIndex`](super::ErrorCodeIndex) reads: every `.rs` file under a
//! root, which enums derive `Error`, and the code an `#[error(...)]` attribute declares.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use syn::parse::ParseStream;
use syn::{LitStr, Token};

/// Recursively collects `.rs` files under `dir`, skipping build, hidden and `excluded` ones.
pub(super) fn collect_rust_files_excluding(
    dir: &Path,
    excluded: &[&str],
    out: &mut Vec<PathBuf>,
) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if path.is_dir() {
            if matches!(name.as_ref(), "target" | ".git")
                || name.starts_with('.')
                || excluded.contains(&name.as_ref())
            {
                continue;
            }
            collect_rust_files_excluding(&path, excluded, out)?;
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(path);
        }
    }
    Ok(())
}

/// True if the attribute list contains `#[derive(… Name …)]`.
pub(super) fn derives(attrs: &[syn::Attribute], name: &str) -> bool {
    attrs.iter().any(|attr| {
        if !attr.path().is_ident("derive") {
            return false;
        }
        let mut found = false;
        // The last segment, so `fundamental_macros::Error` is read as `Error`.
        let _ = attr.parse_nested_meta(|meta| {
            if meta
                .path
                .segments
                .last()
                .is_some_and(|segment| segment.ident == name)
            {
                found = true;
            }
            Ok(())
        });
        found
    })
}

/// The `code = "…"` of the first `#[error(...)]` attribute; every other argument is skipped.
pub(super) fn error_code(attrs: &[syn::Attribute]) -> Option<String> {
    let attr = attrs.iter().find(|a| a.path().is_ident("error"))?;
    let parser = |input: ParseStream| -> syn::Result<Option<String>> {
        let mut code = None;
        let mut first = true;
        while !input.is_empty() {
            if let Some(found) = argument(input, first)? {
                code = Some(found);
            }
            first = false;
            if input.is_empty() {
                break;
            }
            input.parse::<Token![,]>()?;
        }
        Ok(code)
    };
    attr.parse_args_with(parser).ok().flatten()
}

/// One argument of `#[error(...)]`, consumed: the code when it is `code = "…"`.
fn argument(input: ParseStream, first: bool) -> syn::Result<Option<String>> {
    if input.peek(syn::Ident) && input.peek2(Token![=]) {
        let key: syn::Ident = input.parse()?;
        input.parse::<Token![=]>()?;
        if key == "code" {
            return Ok(Some(input.parse::<LitStr>()?.value()));
        }
        let _: syn::Expr = input.parse()?;
    } else if first && input.peek(LitStr) {
        let _: LitStr = input.parse()?;
    } else {
        let _: syn::Expr = input.parse()?;
    }
    Ok(None)
}
