use std::fmt;

use ring::digest::{SHA256, digest};

/// A root certificate's SHA-256 fingerprint over its DER bytes.
///
/// SHA-256 because that is what `openssl`, `certutil` and Keychain show a person.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RootFingerprint([u8; 32]);

impl RootFingerprint {
    /// The fingerprint of a certificate's DER encoding.
    #[must_use]
    pub fn of_der(der: &[u8]) -> Self {
        let mut bytes = [0_u8; 32];
        bytes.copy_from_slice(digest(&SHA256, der).as_ref());
        Self(bytes)
    }

    /// Parses 64 lower-case hexadecimal digits.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        if text.len() != 64 {
            return None;
        }
        let mut bytes = [0_u8; 32];
        for (at, byte) in bytes.iter_mut().enumerate() {
            let pair = text.get(at * 2..at * 2 + 2)?;
            if pair.bytes().any(|digit| digit.is_ascii_uppercase()) {
                return None;
            }
            *byte = u8::from_str_radix(pair, 16).ok()?;
        }
        Some(Self(bytes))
    }

    /// The colon-separated upper-case form certificate tools print.
    #[must_use]
    pub fn colons(&self) -> String {
        self.0
            .iter()
            .map(|byte| format!("{byte:02X}"))
            .collect::<Vec<_>>()
            .join(":")
    }
}

impl fmt::Display for RootFingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.iter().try_for_each(|byte| write!(f, "{byte:02x}"))
    }
}

#[cfg(test)]
mod tests {
    use super::RootFingerprint;

    #[test]
    fn a_fingerprint_reads_back_what_it_writes() {
        let fingerprint = RootFingerprint::of_der(b"der");
        let written = fingerprint.to_string();
        assert_eq!(written.len(), 64);
        assert_eq!(RootFingerprint::parse(&written), Some(fingerprint));
        assert_eq!(RootFingerprint::parse(&written.to_uppercase()), None);
        assert_eq!(RootFingerprint::parse("ab"), None);
        assert_eq!(fingerprint.colons().len(), 95);
    }
}
