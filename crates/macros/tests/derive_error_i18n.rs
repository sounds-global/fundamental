//! End-to-end tests for `#[derive(Error)]` translation routing.
//!
//! Untranslated messages render the source template byte for byte; a translator gets each variant's named arguments.

use fundamental::Error;
use fundamental::core::i18n::{self, Arg, ArgValue, Translator};
use std::sync::Arc;

#[derive(Debug, Error)]
enum UserError {
    #[error(code = "USER_001", message = "User {id} not found", status = 404)]
    NotFound { id: u64 },

    /// Must borrow, never allocate.
    #[error(code = "USER_002", message = "Email already in use", status = 409)]
    EmailExists,

    #[error(
        code = "USER_003",
        translate = "user.too_many",
        message = "Too many attempts: {count}",
        status = 429
    )]
    TooMany { count: u32 },
}

struct SpanishTranslator;

impl Translator for SpanishTranslator {
    fn lookup(&self, key: &str, args: &[Arg<'_>]) -> Option<String> {
        let arg = |name: &str| {
            args.iter()
                .find(|(n, _)| *n == name)
                .map(|(_, v)| v.to_string())
        };
        match key {
            "error.user_001" => Some(format!(
                "Usuario {} no encontrado",
                arg("id").unwrap_or_default()
            )),
            "error.user_002" => Some("Correo ya en uso".to_string()),
            "user.too_many" => Some(format!(
                "Demasiados intentos: {}",
                arg("count").unwrap_or_default()
            )),
            _ => None,
        }
    }

    fn locale_tag(&self) -> &str {
        "es"
    }
}

#[test]
fn renders_source_locale_when_no_translator() {
    assert_eq!(
        UserError::NotFound { id: 42 }.display_message(),
        "User 42 not found"
    );
    assert_eq!(
        UserError::EmailExists.display_message(),
        "Email already in use"
    );
    assert_eq!(
        UserError::TooMany { count: 5 }.display_message(),
        "Too many attempts: 5"
    );
}

#[test]
fn routes_through_installed_translator() {
    let rendered = i18n::with(Arc::new(SpanishTranslator), || {
        (
            UserError::NotFound { id: 42 }
                .display_message()
                .into_owned(),
            UserError::EmailExists.display_message().into_owned(),
            UserError::TooMany { count: 5 }
                .display_message()
                .into_owned(),
        )
    });
    assert_eq!(rendered.0, "Usuario 42 no encontrado");
    assert_eq!(rendered.1, "Correo ya en uso");
    assert_eq!(rendered.2, "Demasiados intentos: 5");
}

#[test]
fn scope_is_popped_after_with() {
    i18n::with(Arc::new(SpanishTranslator), || {
        assert_eq!(UserError::EmailExists.display_message(), "Correo ya en uso");
    });
    assert_eq!(
        UserError::EmailExists.display_message(),
        "Email already in use"
    );
}

#[test]
fn unknown_key_falls_through_to_source() {
    struct OnlyEmail;
    impl Translator for OnlyEmail {
        fn lookup(&self, key: &str, _args: &[Arg<'_>]) -> Option<String> {
            (key == "error.user_002").then(|| "Correo ya en uso".to_string())
        }
    }
    i18n::with(Arc::new(OnlyEmail), || {
        assert_eq!(
            UserError::NotFound { id: 7 }.display_message(),
            "User 7 not found"
        );
        assert_eq!(UserError::EmailExists.display_message(), "Correo ya en uso");
    });
}

#[test]
fn display_still_renders_source_locale() {
    let err = UserError::NotFound { id: 1 };
    assert_eq!(err.to_string(), "User 1 not found");
}

#[derive(Debug, Error)]
enum FileError {
    #[error(code = "FILE_001", message = "missing: {path:?}", status = 404)]
    Missing { path: std::path::PathBuf },
}

#[test]
fn spec_formatted_field_reaches_translator() {
    struct T;
    impl Translator for T {
        fn lookup(&self, key: &str, args: &[Arg<'_>]) -> Option<String> {
            (key == "error.file_001").then(|| {
                let path = args
                    .iter()
                    .find(|(n, _)| *n == "path")
                    .map(|(_, v)| v.to_string())
                    .unwrap_or_default();
                format!("falta: {path}")
            })
        }
    }

    let err = FileError::Missing {
        path: std::path::PathBuf::from("/a/b"),
    };
    assert_eq!(err.display_message(), "missing: \"/a/b\"");

    let rendered = i18n::with(Arc::new(T), || err.display_message().into_owned());
    assert_eq!(rendered, "falta: \"/a/b\"");
}

#[test]
fn arg_value_int_constructor_available() {
    let n = 3i64;
    let args = [("n", ArgValue::from(n))];
    assert_eq!(i18n::interpolate("{n} items", &args), "3 items");
}
