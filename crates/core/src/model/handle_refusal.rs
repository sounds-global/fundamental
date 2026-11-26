//! [`HandleRefusal`] — why a text is not a handle of the kind that was asked for.

use core::fmt;

use super::HandlePrefix;

/// Why a text is not a handle of the expected kind.
///
/// No variant carries the text read: a refusal is logged, and the text is whatever a caller typed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum HandleRefusal {
    /// Not `prefix_suffix` with a TypeID prefix: a wrong length, a misplaced separator, or a
    /// prefix outside `[a-z_]` that starts or ends with `_`.
    Shape,
    /// A well-formed handle of another kind.
    Prefix {
        /// The kind that was asked for.
        expected: HandlePrefix,
    },
    /// The 26 characters after the separator are not 128 bits in Crockford base32.
    Suffix,
}

impl fmt::Display for HandleRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Shape => formatter.write_str("not a TypeID: prefix, `_`, then 26 characters"),
            Self::Prefix { expected } => {
                write!(
                    formatter,
                    "a handle of another kind; expected `{expected}_`"
                )
            }
            Self::Suffix => formatter.write_str("the suffix is not 128 bits of Crockford base32"),
        }
    }
}

impl core::error::Error for HandleRefusal {}

#[cfg(test)]
mod tests {
    use super::HandleRefusal;
    use crate::model::HandlePrefix;

    #[test]
    fn a_prefix_refusal_names_the_expected_kind_only() {
        let expected = HandlePrefix::new("team").unwrap_or(HandlePrefix::EMPTY);
        let text = HandleRefusal::Prefix { expected }.to_string();
        assert_eq!(text, "a handle of another kind; expected `team_`");
    }
}
