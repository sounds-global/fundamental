//! Shared, dependency-free key and placeholder logic for Fundamental's i18n.

#![forbid(unsafe_code)]
#![no_std]
#![deny(missing_docs)]

extern crate alloc;

use alloc::borrow::Cow;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// Scans a `{name}` message template.
///
/// Returns `(rewritten_template, referenced_names)`: `{0}` becomes `{_0}`, and anonymous `{}` is
/// not recorded.
#[expect(clippy::excessive_nesting, reason = "readability baseline")]
pub fn scan_template(template: &str) -> (String, Vec<String>) {
    let mut out = String::new();
    let mut referenced: Vec<String> = Vec::new();
    let mut chars = template.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            '{' if chars.peek() == Some(&'{') => {
                chars.next();
                out.push_str("{{");
            }
            '}' if chars.peek() == Some(&'}') => {
                chars.next();
                out.push_str("}}");
            }
            '{' => {
                let mut inner = String::new();
                for c2 in chars.by_ref() {
                    if c2 == '}' {
                        break;
                    }
                    inner.push(c2);
                }
                let (arg, spec) = match inner.split_once(':') {
                    Some((a, s)) => (a, Some(s)),
                    None => (inner.as_str(), None),
                };

                if let Some(spec) = spec {
                    for name in dollar_names(spec) {
                        push_ref(name, &mut referenced);
                    }
                }

                let rewritten_arg = if arg.is_empty() {
                    String::new()
                } else if arg.bytes().all(|b| b.is_ascii_digit()) {
                    let name = format!("_{arg}");
                    push_ref(name.clone(), &mut referenced);
                    name
                } else if is_ident(arg) {
                    push_ref(arg.to_string(), &mut referenced);
                    arg.to_string()
                } else {
                    arg.to_string()
                };

                out.push('{');
                out.push_str(&rewritten_arg);
                if let Some(spec) = spec {
                    out.push(':');
                    out.push_str(spec);
                }
                out.push('}');
            }
            _ => out.push(ch),
        }
    }

    (out, referenced)
}

/// Names referenced via a **plain** `{name}` / `{0}` placeholder — no `:spec` —
/// each as its [`argument_name`] (`{0}` → `arg0`).
///
/// Fields referenced only with a spec (e.g. `{p:?}`) are excluded.
#[expect(clippy::excessive_nesting, reason = "readability baseline")]
pub fn plain_placeholder_names(template: &str) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    let mut chars = template.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '{' if chars.peek() == Some(&'{') => {
                chars.next();
            }
            '}' if chars.peek() == Some(&'}') => {
                chars.next();
            }
            '{' => {
                let mut inner = String::new();
                for c2 in chars.by_ref() {
                    if c2 == '}' {
                        break;
                    }
                    inner.push(c2);
                }
                if inner.contains(':') {
                    continue;
                }
                let name = if inner.is_empty() {
                    continue;
                } else if inner.bytes().all(|b| b.is_ascii_digit()) || is_ident(&inner) {
                    argument_name(&inner).into_owned()
                } else {
                    continue;
                };
                push_ref(name, &mut names);
            }
            _ => {}
        }
    }
    names
}

/// Extracts every **named** placeholder and its format spec, first occurrence
/// per name, in order: `{p:?}` → `("p", Some("?"))`; anonymous `{}` is omitted.
#[expect(clippy::excessive_nesting, reason = "readability baseline")]
pub fn named_placeholders(template: &str) -> Vec<(String, Option<String>)> {
    let mut out: Vec<(String, Option<String>)> = Vec::new();
    let mut chars = template.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '{' if chars.peek() == Some(&'{') => {
                chars.next();
            }
            '}' if chars.peek() == Some(&'}') => {
                chars.next();
            }
            '{' => {
                let mut inner = String::new();
                for c2 in chars.by_ref() {
                    if c2 == '}' {
                        break;
                    }
                    inner.push(c2);
                }
                let (arg, spec) = match inner.split_once(':') {
                    Some((a, s)) => (a, Some(s)),
                    None => (inner.as_str(), None),
                };
                let name = if arg.is_empty() {
                    continue; // anonymous — filled by a trailing arg expression.
                } else if arg.bytes().all(|b| b.is_ascii_digit()) || is_ident(arg) {
                    argument_name(arg).into_owned()
                } else {
                    continue; // an inline expression, not a binding.
                };
                if out.iter().any(|(n, _)| n == &name) {
                    continue; // first occurrence's spec wins.
                }
                out.push((name, spec.map(|s| s.to_string())));
            }
            _ => {}
        }
    }
    out
}

/// Derives the stable translation key for an error variant.
///
/// Uses `translate` when present, otherwise `error.{lowercased_code}` (`USER_001` → `error.user_001`).
pub fn error_key(code: &str, translate: Option<&str>) -> String {
    match translate {
        Some(explicit) => explicit.to_string(),
        None => format!("error.{}", code.to_ascii_lowercase()),
    }
}

/// PascalCase → snake_case, splitting before every interior capital (`YugabyteDB` → `yugabyte_d_b`).
pub fn to_snake_case(name: &str) -> String {
    let mut out = String::new();
    for (i, c) in name.chars().enumerate() {
        if c.is_ascii_uppercase() {
            if i > 0 {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

/// `OrganizationMember` → `Organization Member` (space before each interior capital).
pub fn humanize_pascal(name: &str) -> String {
    let mut out = String::new();
    for (i, c) in name.chars().enumerate() {
        if c.is_ascii_uppercase() && i > 0 {
            out.push(' ');
        }
        out.push(c);
    }
    out
}

/// `email_address` → `Email Address` (title-case each underscore-separated word).
pub fn humanize_snake(name: &str) -> String {
    let mut words: Vec<String> = Vec::new();
    for word in name.split('_').filter(|w| !w.is_empty()) {
        let mut chars = word.chars();
        let titled = match chars.next() {
            Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
            None => String::new(),
        };
        words.push(titled);
    }
    words.join(" ")
}

/// Basic English pluralization of `word`.
pub fn pluralize(word: &str) -> String {
    if word.is_empty() {
        return String::new();
    }
    if word.ends_with('s') && !word.ends_with("ss") {
        return word.to_string();
    }
    if word.ends_with("ch")
        || word.ends_with("sh")
        || word.ends_with('x')
        || word.ends_with('z')
        || word.ends_with("ss")
    {
        return format!("{word}es");
    }
    if let Some(stem) = word.strip_suffix('y') {
        let before_y = stem.as_bytes().last().copied().unwrap_or(b'a');
        if !matches!(before_y, b'a' | b'e' | b'i' | b'o' | b'u') {
            return format!("{stem}ies");
        }
    }
    format!("{word}s")
}

/// Pluralizes the last word of a humanized label: `Organization Members`.
pub fn pluralize_label(label: &str) -> String {
    match label.rsplit_once(' ') {
        Some((head, last)) => format!("{head} {}", pluralize(last)),
        None => pluralize(label),
    }
}

/// Translation key for a model's singular entity label: `model.{type}`.
pub fn model_entity_key(type_name: &str) -> String {
    format!("model.{}", to_snake_case(type_name))
}

/// Translation key for a model's plural entity label: `model.{type}.plural`.
pub fn model_entity_plural_key(type_name: &str) -> String {
    format!("model.{}.plural", to_snake_case(type_name))
}

/// Translation key for a model field label: `model.{type}.field.{field}`.
pub fn model_field_key(type_name: &str, field_name: &str) -> String {
    format!("model.{}.field.{field_name}", to_snake_case(type_name))
}

/// Translation key for an enum variant label: `enum.{type}.{variant}`.
pub fn enum_variant_key(type_name: &str, variant_name: &str) -> String {
    format!(
        "enum.{}.{}",
        to_snake_case(type_name),
        to_snake_case(variant_name)
    )
}

fn push_ref(name: String, refs: &mut Vec<String>) {
    if !refs.contains(&name) {
        refs.push(name);
    }
}

/// Identifiers immediately followed by `$` in a format spec (`width` in `{:>width$}`).
fn dollar_names(spec: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut cur = String::new();
    for ch in spec.chars() {
        if ch == '_' || ch.is_ascii_alphanumeric() {
            cur.push(ch);
        } else {
            if ch == '$' && is_ident(&cur) && !cur.bytes().all(|b| b.is_ascii_digit()) {
                names.push(cur.clone());
            }
            cur.clear();
        }
    }
    names
}

/// The translation-argument name for a placeholder or binding name.
///
/// A positional `{0}` is named `arg0`; every other name is kept.
pub fn argument_name(name: &str) -> Cow<'_, str> {
    let digits = name.strip_prefix('_').unwrap_or(name);
    if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) {
        Cow::Owned(format!("arg{digits}"))
    } else {
        Cow::Borrowed(name)
    }
}

fn is_ident(s: &str) -> bool {
    let mut bytes = s.bytes();
    match bytes.next() {
        Some(b) if b == b'_' || b.is_ascii_alphabetic() => {}
        _ => return false,
    }
    bytes.all(|b| b == b'_' || b.is_ascii_alphanumeric())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scans_named_and_numeric() {
        let (out, refs) = scan_template("User {id} at {0}");
        assert_eq!(out, "User {id} at {_0}");
        assert_eq!(refs, ["id", "_0"]);
    }

    #[test]
    fn anonymous_placeholder_not_recorded() {
        let (out, refs) = scan_template("missing: {}");
        assert_eq!(out, "missing: {}");
        assert!(refs.is_empty());
    }

    #[test]
    fn plain_excludes_spec_fields() {
        // Only `{a}` is plain; `{b:?}` (Debug) and `{c:>5}` (spec) are excluded.
        assert_eq!(plain_placeholder_names("{a} {b:?} {c:>5}"), ["a"]);
    }

    #[test]
    fn named_placeholders_keep_specs() {
        assert_eq!(
            named_placeholders("{a} {b:?} {0:>5} {} text"),
            [
                ("a".to_string(), None),
                ("b".to_string(), Some("?".to_string())),
                ("arg0".to_string(), Some(">5".to_string())),
            ]
        );
    }

    #[test]
    fn positional_placeholders_are_named_arguments() {
        assert_eq!(plain_placeholder_names("{0} and {1}"), ["arg0", "arg1"]);
        assert_eq!(named_placeholders("{_2}"), [("arg2".to_string(), None)]);
        assert_eq!(argument_name("_0"), "arg0");
        assert_eq!(argument_name("id"), "id");
    }

    #[test]
    fn named_placeholders_first_spec_wins() {
        assert_eq!(
            named_placeholders("{x} and {x:?}"),
            [("x".to_string(), None)]
        );
    }

    #[test]
    fn error_key_defaults_and_overrides() {
        assert_eq!(error_key("USER_001", None), "error.user_001");
        assert_eq!(error_key("USER_001", Some("user.gone")), "user.gone");
    }
}
