//! [`CommandAddressObjection`], the reason a `<namespace>:<name>` address is refused.

use core::fmt;

/// Why a command address, or one half of one, is not legal.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum CommandAddressObjection {
    /// The segment had no characters at all.
    Empty,
    /// The segment did not begin with a lower-case ASCII letter.
    BadFirstCharacter,
    /// The segment ended with a hyphen.
    TrailingHyphen,
    /// The segment contained `--`.
    DoubleHyphen,
    /// The segment contained a character outside `[a-z0-9-]`.
    IllegalCharacter(char),
    /// The address did not contain exactly one `:`.
    NotOneColon,
}

impl fmt::Display for CommandAddressObjection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("cannot be empty"),
            Self::BadFirstCharacter => f.write_str("must start with a lower-case letter"),
            Self::TrailingHyphen => f.write_str("cannot end with a hyphen"),
            Self::DoubleHyphen => f.write_str("cannot contain a double hyphen"),
            Self::IllegalCharacter(c) => write!(f, "cannot contain `{c}`"),
            Self::NotOneColon => {
                f.write_str("must be written `<namespace>:<name>`, with exactly one colon")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_objection_says_what_is_wrong() {
        for objection in [
            CommandAddressObjection::Empty,
            CommandAddressObjection::BadFirstCharacter,
            CommandAddressObjection::TrailingHyphen,
            CommandAddressObjection::DoubleHyphen,
            CommandAddressObjection::IllegalCharacter('_'),
            CommandAddressObjection::NotOneColon,
        ] {
            assert!(!objection.to_string().is_empty(), "{objection:?}");
        }
    }

    #[test]
    fn an_illegal_character_is_quoted_in_the_message() {
        assert_eq!(
            CommandAddressObjection::IllegalCharacter('_').to_string(),
            "cannot contain `_`"
        );
    }
}
