//! [`RefusalKind`] — why a stored row could not be decoded into its model.

use core::fmt;

/// Why a stored row could not be decoded into its model, as a closed set a store can record.
///
/// A code is a contract checked by a `CHECK` constraint. [`KeyNotHeld`](Self::KeyNotHeld) describes
/// the process, not the row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum RefusalKind {
    /// The stored envelope is not one this framework writes.
    EnvelopeRefused,
    /// The envelope did not authenticate under the key it names.
    DecryptRefused,
    /// The envelope names a key, or a key version, this process does not hold.
    KeyNotHeld,
    /// A plain column holds a value its field's type cannot take.
    ValueRefused,
}

impl RefusalKind {
    /// Every kind, in declaration order.
    pub const ALL: [Self; 4] = [
        Self::EnvelopeRefused,
        Self::DecryptRefused,
        Self::KeyNotHeld,
        Self::ValueRefused,
    ];

    /// The stable code a store records this kind under.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::EnvelopeRefused => "envelope_refused",
            Self::DecryptRefused => "decrypt_refused",
            Self::KeyNotHeld => "key_not_held",
            Self::ValueRefused => "value_refused",
        }
    }

    /// The kind `code` names, or `None` for a code this framework never writes.
    #[must_use]
    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.code() == code)
    }

    /// This kind's position in [`ALL`](Self::ALL), for a fixed-size tally.
    pub(crate) const fn index(self) -> usize {
        match self {
            Self::EnvelopeRefused => 0,
            Self::DecryptRefused => 1,
            Self::KeyNotHeld => 2,
            Self::ValueRefused => 3,
        }
    }
}

impl fmt::Display for RefusalKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.code())
    }
}

#[cfg(test)]
mod tests {
    use super::RefusalKind;

    #[test]
    fn every_kind_has_a_code_that_parses_back_to_it() {
        for kind in RefusalKind::ALL {
            assert_eq!(RefusalKind::from_code(kind.code()), Some(kind));
        }
    }

    #[test]
    fn a_code_this_framework_never_writes_names_no_kind() {
        assert_eq!(RefusalKind::from_code("tampered"), None);
        assert_eq!(RefusalKind::from_code("Decrypt_Refused"), None);
        assert_eq!(RefusalKind::from_code(""), None);
    }

    #[test]
    fn every_kind_sits_at_its_own_position() {
        for (position, kind) in RefusalKind::ALL.into_iter().enumerate() {
            assert_eq!(kind.index(), position);
        }
    }

    #[test]
    fn a_kind_displays_as_its_code() {
        assert_eq!(RefusalKind::KeyNotHeld.to_string(), "key_not_held");
    }
}
