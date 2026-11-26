//! Tests for the `#[error(...)]` format passthrough, trailing arguments, positional form, `status` expressions, `transparent` and `Cow` messages.

use fundamental::Error;
use std::borrow::Cow;
use std::error::Error as StdError;
use std::path::PathBuf;

#[derive(Debug, Error)]
enum PathError {
    #[error(code = "P_DBG", message = "missing: {p:?}", status = 500)]
    Debugged { p: PathBuf },

    #[error(code = "P_DISP", message = "missing: {}", path.display(), status = 500)]
    Displayed { path: PathBuf },
}

#[test]
fn pathbuf_via_debug_spec() {
    let e = PathError::Debugged {
        p: PathBuf::from("/a/b"),
    };
    assert_eq!(e.display_message(), "missing: \"/a/b\"");
}

#[test]
fn pathbuf_via_trailing_display_arg() {
    let e = PathError::Displayed {
        path: PathBuf::from("/a/b"),
    };
    assert_eq!(e.display_message(), "missing: /a/b");
    assert_eq!(e.to_string(), "missing: /a/b");
}

#[derive(Debug, Error)]
enum SpecError {
    #[error(
        code = "SPEC",
        message = "[{n:>5}] {ratio:.2} {flags:#x}",
        status = 500
    )]
    Formatted { n: u32, ratio: f64, flags: u32 },
}

#[test]
// PI-like on purpose, to exercise `{:.2}` rounding to "3.14".
#[allow(clippy::approx_constant)]
#[expect(
    clippy::allow_attributes,
    clippy::allow_attributes_without_reason,
    reason = "readability baseline"
)]
fn format_specifiers_pass_through() {
    let e = SpecError::Formatted {
        n: 42,
        ratio: 3.14159,
        flags: 255,
    };
    assert_eq!(e.display_message(), "[   42] 3.14 0xff");
}

#[derive(Debug, Error)]
enum PositionalError {
    #[error("read '{}' after {n} tries", path.display(), code = "POS", status = 500)]
    Read { path: PathBuf, n: u32 },
}

#[test]
fn positional_message_with_args_and_metadata() {
    let e = PositionalError::Read {
        path: PathBuf::from("/x"),
        n: 3,
    };
    assert_eq!(e.error_code(), "POS");
    assert_eq!(e.http_status_code(), 500);
    assert_eq!(e.display_message(), "read '/x' after 3 tries");
}

mod status {
    pub const NOT_FOUND: u16 = 404;
}

#[derive(Debug, Error)]
enum NamedStatusError {
    #[error(code = "NF", message = "nope", status = status::NOT_FOUND)]
    Missing,
}

#[test]
fn named_status_constant() {
    assert_eq!(NamedStatusError::Missing.http_status_code(), 404);
}

#[derive(Debug, Error)]
enum Inner {
    #[error(code = "INNER", message = "inner boom: {detail}", status = 404)]
    Boom { detail: String },
}

#[derive(Debug, Error)]
enum Outer {
    #[error(transparent)]
    Wrapped(Inner),
}

#[derive(Debug, Error)]
enum OuterNamed {
    #[error(transparent)]
    Wrapped { inner: Inner },
}

#[test]
fn transparent_delegates_metadata_and_source() {
    let e = Outer::Wrapped(Inner::Boom {
        detail: "disk".into(),
    });
    assert_eq!(e.error_code(), "INNER");
    assert_eq!(e.http_status_code(), 404);
    assert_eq!(e.display_message(), "inner boom: disk");
    assert_eq!(e.to_string(), "inner boom: disk");
    let src = e.source().expect("transparent should expose a source");
    assert!(src.to_string().contains("inner boom: disk"));
}

#[test]
fn transparent_named_field() {
    let e = OuterNamed::Wrapped {
        inner: Inner::Boom {
            detail: "net".into(),
        },
    };
    assert_eq!(e.error_code(), "INNER");
    assert_eq!(e.display_message(), "inner boom: net");
}

#[derive(Debug, Error)]
enum CowError {
    #[error(code = "STATIC", message = "static message", status = 500)]
    Static,
    #[error(code = "DYN", message = "dynamic {x}", status = 500)]
    Dynamic { x: u32 },
}

#[test]
fn static_message_borrows_dynamic_owns() {
    assert!(matches!(
        CowError::Static.display_message(),
        Cow::Borrowed(_)
    ));
    assert!(matches!(
        CowError::Dynamic { x: 1 }.display_message(),
        Cow::Owned(_)
    ));
}
