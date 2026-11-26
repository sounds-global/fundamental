use core::fmt;

#[cfg(not(feature = "std"))]
use alloc::string::String;

use crate::MailAddressProblem;

const MAX_ADDRESS: usize = 254;
const MAX_LOCAL: usize = 64;
const MAX_DOMAIN: usize = 253;
const MAX_LABEL: usize = 63;

/// A mail address that can stand alone in a header: `local@domain`, and nothing else.
///
/// No input that parses can add a recipient; `Debug` shows only the domain.
///
/// ```
/// use fundamental_core::{MailAddress, MailAddressProblem};
///
/// let address = MailAddress::parse("Ada.Lovelace@Example.NL")?;
/// assert_eq!(address.expose(), "Ada.Lovelace@example.nl");
/// assert_eq!(format!("{address:?}"), "MailAddress(<redacted>@example.nl)");
///
/// let refused = MailAddress::parse("eve@evil.nl>, victim@bank.nl");
/// assert_eq!(refused, Err(MailAddressProblem::ForbiddenCharacter));
/// # Ok::<(), MailAddressProblem>(())
/// ```
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MailAddress {
    normalised: String,
    at: usize,
}

impl MailAddress {
    /// Parses `raw` exactly as given: nothing is trimmed.
    ///
    /// # Errors
    /// The first [`MailAddressProblem`] found, checking the whole value before its parts.
    pub fn parse(raw: &str) -> Result<Self, MailAddressProblem> {
        if !raw.is_ascii() {
            return Err(MailAddressProblem::NotAscii);
        }
        if raw.bytes().any(is_forbidden) {
            return Err(MailAddressProblem::ForbiddenCharacter);
        }
        let mut halves = raw.split('@');
        let (Some(local), Some(domain), None) = (halves.next(), halves.next(), halves.next())
        else {
            return Err(MailAddressProblem::Shape);
        };
        if local.is_empty() || domain.is_empty() {
            return Err(MailAddressProblem::Shape);
        }
        if raw.len() > MAX_ADDRESS {
            return Err(MailAddressProblem::TooLong);
        }
        check_local(local)?;
        check_domain(domain)?;
        let mut normalised = String::with_capacity(raw.len());
        normalised.push_str(local);
        normalised.push('@');
        normalised.push_str(&domain.to_ascii_lowercase());
        Ok(Self {
            normalised,
            at: local.len(),
        })
    }

    /// The address, as it goes into a header or a provider's request.
    #[must_use]
    pub fn expose(&self) -> &str {
        &self.normalised
    }

    /// The part before the `@`, with its case kept.
    #[must_use]
    pub fn local_part(&self) -> &str {
        &self.normalised[..self.at]
    }

    /// The domain, lowercased.
    #[must_use]
    pub fn domain(&self) -> &str {
        &self.normalised[self.at + 1..]
    }
}

impl fmt::Debug for MailAddress {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "MailAddress(<redacted>@{})", self.domain())
    }
}

/// Control characters, whitespace and RFC 5322's specials other than `@` and `.`.
fn is_forbidden(byte: u8) -> bool {
    byte.is_ascii_control()
        || byte == b' '
        || matches!(
            byte,
            b'"' | b'(' | b')' | b',' | b':' | b';' | b'<' | b'>' | b'[' | b'\\' | b']'
        )
}

fn check_local(local: &str) -> Result<(), MailAddressProblem> {
    if local.len() > MAX_LOCAL {
        return Err(MailAddressProblem::LocalTooLong);
    }
    if local.starts_with('.') || local.ends_with('.') || local.contains("..") {
        return Err(MailAddressProblem::DotPlacement);
    }
    Ok(())
}

fn check_domain(domain: &str) -> Result<(), MailAddressProblem> {
    if domain.len() > MAX_DOMAIN {
        return Err(MailAddressProblem::DomainTooLong);
    }
    let mut labels = 0usize;
    for label in domain.split('.') {
        labels += 1;
        if label.len() > MAX_LABEL {
            return Err(MailAddressProblem::DomainTooLong);
        }
        let well_formed = !label.is_empty()
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-');
        if !well_formed {
            return Err(MailAddressProblem::DomainLabel);
        }
    }
    if labels < 2 {
        return Err(MailAddressProblem::DomainLabel);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{MailAddress, MailAddressProblem};

    #[cfg(not(feature = "std"))]
    use alloc::{format, string::String};

    fn refused(raw: &str) -> MailAddressProblem {
        match MailAddress::parse(raw) {
            Ok(address) => panic!("{raw:?} parsed as {address:?}"),
            Err(problem) => problem,
        }
    }

    #[test]
    fn a_mail_address_refuses_every_header_shape() {
        for raw in [
            "eve@evil.nl>, victim@bank.nl",
            "victim@bank.nl\r\nBcc: eve@evil.nl",
            "victim@bank.nl\nBcc: eve@evil.nl",
            "a b@example.nl",
            "<a@example.nl>",
            "\"quoted\"@example.nl",
            "a@example.nl;b@example.nl",
            "a(comment)@example.nl",
            "a@[192.0.2.1]",
            "a\\b@example.nl",
            "a:b@example.nl",
            "a@example.nl\0",
            "a@example.nl\t",
        ] {
            assert_eq!(
                refused(raw),
                MailAddressProblem::ForbiddenCharacter,
                "{raw:?}"
            );
        }
    }

    #[test]
    fn a_mail_address_keeps_its_local_part_case() {
        let address = MailAddress::parse("Ada.Lovelace@Example.NL").expect("valid");
        assert_eq!(address.local_part(), "Ada.Lovelace");
        assert_eq!(address.domain(), "example.nl");
        assert_eq!(address.expose(), "Ada.Lovelace@example.nl");
        assert_eq!(
            address,
            MailAddress::parse("Ada.Lovelace@example.nl").expect("valid")
        );
        assert_ne!(
            address,
            MailAddress::parse("ada.lovelace@example.nl").expect("valid")
        );
    }

    #[test]
    fn the_length_limits_are_rfc_5321s() {
        let local = "l".repeat(64);
        let label = "d".repeat(63);
        let at_the_limit = format!("{local}@{label}.nl");
        assert!(MailAddress::parse(&at_the_limit).is_ok());

        assert_eq!(
            refused(&format!("{}@example.nl", "l".repeat(65))),
            MailAddressProblem::LocalTooLong
        );
        assert_eq!(
            refused(&format!("a@{}.nl", "d".repeat(64))),
            MailAddressProblem::DomainTooLong
        );

        let mut long_domain = String::new();
        while long_domain.len() < 252 {
            long_domain.push_str("abcdefghi.");
        }
        long_domain.push_str("nl");
        assert_eq!(
            refused(&format!("a@{long_domain}")),
            MailAddressProblem::TooLong
        );
    }

    #[test]
    fn the_shape_is_one_at_with_both_halves() {
        for raw in ["", "plain", "@example.nl", "a@", "a@b@example.nl"] {
            assert_eq!(refused(raw), MailAddressProblem::Shape, "{raw:?}");
        }
    }

    #[test]
    fn dots_and_labels_are_checked() {
        for raw in [".a@example.nl", "a.@example.nl", "a..b@example.nl"] {
            assert_eq!(refused(raw), MailAddressProblem::DotPlacement, "{raw:?}");
        }
        for raw in [
            "a@localhost",
            "a@-example.nl",
            "a@example-.nl",
            "a@example..nl",
            "a@exa_mple.nl",
            "a@example.nl.",
        ] {
            assert_eq!(refused(raw), MailAddressProblem::DomainLabel, "{raw:?}");
        }
    }

    #[test]
    fn a_non_ascii_address_is_refused_with_its_own_code() {
        assert_eq!(refused("jürgen@example.de"), MailAddressProblem::NotAscii);
        assert_eq!(refused("a@bücher.de"), MailAddressProblem::NotAscii);
        assert_eq!(
            refused("a@example.nl\u{202e}"),
            MailAddressProblem::NotAscii
        );
    }

    #[test]
    fn unusual_but_valid_addresses_parse() {
        for raw in [
            "o'brien@example.ie",
            "user+tag@example.nl",
            "x@a.b.c.example.co.uk",
            "1234@123.example",
            "!#$%&'*+-/=?^_`{|}~@example.nl",
            "a@xn--bcher-kva.de",
        ] {
            assert!(MailAddress::parse(raw).is_ok(), "{raw:?}");
        }
    }

    #[test]
    #[expect(clippy::excessive_nesting, reason = "readability baseline")]
    fn no_byte_that_breaks_a_header_is_ever_accepted() {
        let base = "ab.cd@example.nl";
        for byte in 0u8..=127 {
            for position in 0..=base.len() {
                let mut raw = String::from(base);
                raw.insert(position, char::from(byte));
                if let Ok(address) = MailAddress::parse(&raw) {
                    let accepted = address.expose();
                    assert!(
                        !accepted.bytes().any(|seen| seen.is_ascii_control()
                            || matches!(seen, b' ' | b'<' | b'>' | b',' | b';' | b'"')),
                        "{raw:?} was accepted"
                    );
                    assert_eq!(accepted.matches('@').count(), 1, "{raw:?}");
                }
            }
        }
    }

    #[test]
    fn debug_never_prints_the_local_part() {
        let address = MailAddress::parse("secret.person@example.nl").expect("valid");
        let shown = format!("{address:?}");
        assert!(!shown.contains("secret.person"), "{shown}");
        assert!(shown.contains("example.nl"), "{shown}");
    }
}
