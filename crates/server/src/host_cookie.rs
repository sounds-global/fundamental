use std::time::Duration;

use crate::SameSite;

/// A `__Host-` cookie, written `Path=/; Secure; HttpOnly` with no `Domain` (RFC 6265bis §4.1.3.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HostCookie {
    name: &'static str,
    same_site: SameSite,
}

impl HostCookie {
    /// The cookie `name`, sent as `same_site` says.
    ///
    /// # Panics
    ///
    /// When `name` lacks the `__Host-` prefix or holds a byte outside an RFC 6265 token. Built
    /// in a `const`, that is a compile error rather than a panic.
    #[must_use]
    pub const fn new(name: &'static str, same_site: SameSite) -> Self {
        assert!(
            is_host_name(name.as_bytes()),
            "a host cookie is a `__Host-` token"
        );
        Self { name, same_site }
    }

    /// The cookie's name.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        self.name
    }

    /// The `Set-Cookie` value that holds `value` for `max_age`, or `None` for a byte no cookie value may hold.
    #[must_use]
    pub fn set(&self, value: &str, max_age: Duration) -> Option<String> {
        value.bytes().all(is_cookie_octet).then(|| {
            format!(
                "{}={value}; Max-Age={}; Path=/; Secure; HttpOnly; SameSite={}",
                self.name,
                max_age.as_secs(),
                self.same_site
            )
        })
    }

    /// The `Set-Cookie` value that removes the cookie.
    #[must_use]
    pub fn clear(&self) -> String {
        format!(
            "{}=; Max-Age=0; Path=/; Secure; HttpOnly; SameSite={}",
            self.name, self.same_site
        )
    }
}

const PREFIX: &[u8] = b"__Host-";

const fn is_host_name(name: &[u8]) -> bool {
    if name.len() <= PREFIX.len() {
        return false;
    }
    let mut i = 0;
    while i < PREFIX.len() {
        if name[i] != PREFIX[i] {
            return false;
        }
        i += 1;
    }
    while i < name.len() {
        if !is_token(name[i]) {
            return false;
        }
        i += 1;
    }
    true
}

/// RFC 9110 §5.6.2 `tchar`.
const fn is_token(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'!' | b'#'
                | b'$'
                | b'%'
                | b'&'
                | b'\''
                | b'*'
                | b'+'
                | b'-'
                | b'.'
                | b'^'
                | b'_'
                | b'`'
                | b'|'
                | b'~'
        )
}

fn is_cookie_octet(byte: u8) -> bool {
    matches!(byte, 0x21 | 0x23..=0x2B | 0x2D..=0x3A | 0x3C..=0x5B | 0x5D..=0x7E)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::HostCookie;
    use crate::SameSite;

    const DRAFT: HostCookie = HostCookie::new("__Host-draft", SameSite::Lax);

    #[test]
    fn every_attribute_is_written_and_domain_never_is() {
        let set = DRAFT
            .set("abc_-123", Duration::from_secs(604_800))
            .expect("a token value");
        assert_eq!(
            set,
            "__Host-draft=abc_-123; Max-Age=604800; Path=/; Secure; HttpOnly; SameSite=Lax"
        );
        assert!(!set.contains("Domain"));
    }

    #[test]
    fn clearing_writes_max_age_zero_with_the_same_attributes() {
        assert_eq!(
            HostCookie::new("__Host-x", SameSite::Strict).clear(),
            "__Host-x=; Max-Age=0; Path=/; Secure; HttpOnly; SameSite=Strict"
        );
    }

    #[test]
    fn a_value_that_could_add_an_attribute_is_refused() {
        for value in ["a;Domain=evil", "a b", "a,b", "\"a\"", "a\\b", "é"] {
            assert_eq!(DRAFT.set(value, Duration::from_secs(1)), None, "{value}");
        }
    }

    #[test]
    #[should_panic(expected = "a host cookie is a `__Host-` token")]
    fn a_name_without_the_prefix_is_refused() {
        let _ = HostCookie::new("session", SameSite::Lax);
    }

    #[test]
    #[should_panic(expected = "a host cookie is a `__Host-` token")]
    fn a_name_with_a_separator_is_refused() {
        let _ = HostCookie::new("__Host-a;b", SameSite::Lax);
    }
}
