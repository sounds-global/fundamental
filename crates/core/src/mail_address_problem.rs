/// Why a string is not a [`MailAddress`](crate::MailAddress).
///
/// Each reason has a stable [`code`](Self::code), reported without echoing the address.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum MailAddressProblem {
    /// Not exactly one `@`, or an empty part on either side of it.
    Shape,
    /// Longer than 254 octets, the most a forward path can carry (RFC 5321 §4.5.3.1.3).
    TooLong,
    /// A local part longer than 64 octets (RFC 5321 §4.5.3.1.1).
    LocalTooLong,
    /// A domain longer than 253 octets, or a label longer than 63 (RFC 1035 §2.3.4).
    DomainTooLong,
    /// A character outside ASCII; RFC 6531 addresses are refused.
    NotAscii,
    /// A control character, whitespace, or one of `" ( ) , : ; < > [ \ ]`.
    ForbiddenCharacter,
    /// A dot at the start or end of the local part, or two dots in a row.
    DotPlacement,
    /// A domain label that is empty, starts or ends with `-`, or holds another character; or one label.
    DomainLabel,
}

impl MailAddressProblem {
    /// The stable code for this reason.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Shape => "address_shape",
            Self::TooLong => "address_too_long",
            Self::LocalTooLong => "address_local_too_long",
            Self::DomainTooLong => "address_domain_too_long",
            Self::NotAscii => "address_not_ascii",
            Self::ForbiddenCharacter => "address_forbidden_character",
            Self::DotPlacement => "address_dot_placement",
            Self::DomainLabel => "address_domain_label",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::MailAddressProblem;

    #[test]
    fn every_code_is_distinct_and_prefixed() {
        let all = [
            MailAddressProblem::Shape,
            MailAddressProblem::TooLong,
            MailAddressProblem::LocalTooLong,
            MailAddressProblem::DomainTooLong,
            MailAddressProblem::NotAscii,
            MailAddressProblem::ForbiddenCharacter,
            MailAddressProblem::DotPlacement,
            MailAddressProblem::DomainLabel,
        ];
        for (index, problem) in all.iter().enumerate() {
            assert!(problem.code().starts_with("address_"));
            for other in &all[index + 1..] {
                assert_ne!(problem.code(), other.code());
            }
        }
    }
}
