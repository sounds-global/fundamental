use fundamental_core::i18n::Locale;
use tracing::error;

use crate::LocalePolicy;

/// How long a recorded choice lasts: a year, in seconds.
const A_YEAR: u32 = 31_536_000;

/// The `Set-Cookie` a recorded language choice is written with, named by the [`LocalePolicy`] that reads it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LanguageCookie {
    name: String,
    secure: bool,
}

impl LanguageCookie {
    /// The cookie `policy` negotiates from, or `None` when it reads none.
    #[must_use]
    pub fn of(policy: &LocalePolicy) -> Option<Self> {
        let name = policy.source().cookie_name()?;
        if !is_token(name) {
            error!(
                length = name.len(),
                "the locale policy's cookie name is not an RFC 6265 token; no language choice can be recorded"
            );
            return None;
        }
        Some(Self {
            name: name.to_owned(),
            secure: true,
        })
    }

    /// Drops `Secure` on a developer's own machine: a browser silently discards it over plain `http`.
    #[must_use]
    pub fn for_development(mut self, development: bool) -> Self {
        self.secure = !development;
        self
    }

    /// The cookie's name, as the boundary will look for it.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The header value recording `language`: `Path=/`, `Max-Age`, `HttpOnly`, `SameSite=Lax`, `Secure`.
    ///
    /// `Lax` so it survives arrival from a link; the whole tag is written, region included.
    #[must_use]
    pub fn set(&self, language: &Locale) -> String {
        let mut value = format!(
            "{}={}; Path=/; Max-Age={A_YEAR}; HttpOnly; SameSite=Lax",
            self.name, language
        );
        if self.secure {
            value.push_str("; Secure");
        }
        value
    }
}

/// Whether `name` is an RFC 6265 `token`: visible ASCII with no separator.
fn is_token(name: &str) -> bool {
    !name.is_empty()
        && name.bytes().all(|byte| {
            byte.is_ascii_graphic()
                && !matches!(
                    byte,
                    b'(' | b')'
                        | b'<'
                        | b'>'
                        | b'@'
                        | b','
                        | b';'
                        | b':'
                        | b'\\'
                        | b'"'
                        | b'/'
                        | b'['
                        | b']'
                        | b'?'
                        | b'='
                        | b'{'
                        | b'}'
                )
        })
}

#[cfg(test)]
mod tests {
    use fundamental_core::i18n::Locale;

    use super::{LanguageCookie, is_token};
    use crate::{LocalePolicy, LocaleSource};

    fn policy(cookie: &str) -> LocalePolicy {
        let en: Locale = match "en".parse() {
            Ok(locale) => locale,
            Err(error) => panic!("en is a language tag: {error:?}"),
        };
        LocalePolicy::new(en).with_source(LocaleSource::cookie_then_accept_language(cookie))
    }

    #[test]
    fn a_regional_choice_is_recorded_as_chosen() {
        let locale = |tag: &str| match tag.parse::<Locale>() {
            Ok(locale) => locale,
            Err(error) => panic!("{tag}: {error:?}"),
        };
        let policy = LocalePolicy::new(locale("en"))
            .with_supported(vec![locale("nl-BE"), locale("nl-NL"), locale("en")])
            .with_source(LocaleSource::cookie_then_accept_language("app-locale"));
        let Some(cookie) = LanguageCookie::of(&policy) else {
            panic!("the policy reads a cookie");
        };
        let written = cookie.set(&locale("nl-NL"));
        assert!(written.starts_with("app-locale=nl-NL;"), "{written}");
    }

    #[test]
    fn a_token_name_makes_a_cookie() {
        assert!(LanguageCookie::of(&policy("app-locale")).is_some());
    }

    #[test]
    fn a_name_that_would_add_attributes_is_refused() {
        for name in ["", "a b", "a;Domain=evil.test", "a=b", "\"x\"", "caf\u{e9}"] {
            assert!(!is_token(name), "`{name}` is not a token");
            assert!(
                LanguageCookie::of(&policy(name)).is_none(),
                "`{name}` made a cookie"
            );
        }
    }
}
