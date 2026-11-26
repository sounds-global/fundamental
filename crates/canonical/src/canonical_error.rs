use core::fmt::Display;
use fundamental_macros::Error;

/// Why bytes are not a canonical document, or why a value cannot become one.
///
/// A refusal names a byte offset, never the attacker-chosen input itself.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum CanonicalError {
    /// The input is longer than the limit, refused before a byte of it was read.
    #[error(
        code = "CANON_001",
        message = "The document is {len} bytes; at most {max} are accepted",
        status = 413
    )]
    TooLarge {
        /// The input's length.
        len: usize,
        /// The limit it exceeded.
        max: usize,
    },

    /// A container would nest deeper than the limit.
    #[error(
        code = "CANON_002",
        message = "The document nests deeper than {max} levels at byte {offset}",
        status = 422
    )]
    TooDeep {
        /// Where the container that exceeded the limit opens.
        offset: usize,
        /// The limit.
        max: u32,
    },

    /// The document holds more values than the limit.
    #[error(
        code = "CANON_003",
        message = "The document holds more than {max} values; the next one starts at byte {offset}",
        status = 422
    )]
    TooManyValues {
        /// Where the value that exceeded the limit starts.
        offset: usize,
        /// The limit.
        max: u32,
    },

    /// A string is longer than the limit, refused before it was decoded.
    #[error(
        code = "CANON_004",
        message = "A string starting at byte {offset} is longer than {max} bytes",
        status = 422
    )]
    StringTooLong {
        /// Where the string opens.
        offset: usize,
        /// The limit.
        max: usize,
    },

    /// An object or array has more members than the limit.
    #[error(
        code = "CANON_005",
        message = "A container at byte {offset} has more than {max} members",
        status = 422
    )]
    TooManyMembers {
        /// Where the member that exceeded the limit starts.
        offset: usize,
        /// The limit.
        max: u32,
    },

    /// The bytes are not valid UTF-8 — including overlong forms and encoded surrogates.
    #[error(
        code = "CANON_006",
        message = "The document is not valid UTF-8 at byte {offset}",
        status = 422
    )]
    InvalidUtf8 {
        /// Where the invalid sequence starts.
        offset: usize,
    },

    /// The document starts with a byte order mark.
    #[error(
        code = "CANON_007",
        message = "The document starts with a byte order mark",
        status = 422
    )]
    ByteOrderMark,

    /// Whitespace outside a string; the canonical form has none.
    #[error(
        code = "CANON_008",
        message = "The document has whitespace at byte {offset}; the canonical form has none",
        status = 422
    )]
    Whitespace {
        /// Where the whitespace is.
        offset: usize,
    },

    /// A byte that no JSON grammar allows here, or the input ended early.
    #[error(
        code = "CANON_009",
        message = "The document expected {expected} at byte {offset}",
        status = 422
    )]
    Syntax {
        /// Where the unexpected byte, or the end of input, is.
        offset: usize,
        /// What the grammar allowed there.
        expected: &'static str,
    },

    /// Bytes after the one top-level value — a trailing newline included.
    #[error(
        code = "CANON_010",
        message = "The document continues after its value, at byte {offset}",
        status = 422
    )]
    TrailingBytes {
        /// Where the first extra byte is.
        offset: usize,
    },

    /// An object names the same member twice.
    #[error(
        code = "CANON_011",
        message = "An object repeats a member name at byte {offset}",
        status = 422
    )]
    DuplicateKey {
        /// Where the repeated name starts.
        offset: usize,
    },

    /// An object's members are not in ascending UTF-16 code-unit order (RFC 8785 §3.2.3).
    #[error(
        code = "CANON_012",
        message = "An object's members are out of canonical order at byte {offset}",
        status = 422
    )]
    KeyOrder {
        /// Where the out-of-order name starts.
        offset: usize,
    },

    /// A number with a fraction or an exponent; this profile carries integers only.
    #[error(
        code = "CANON_013",
        message = "The number at byte {offset} is not an integer; decimals travel as strings",
        status = 422
    )]
    Float {
        /// Where the number starts.
        offset: usize,
    },

    /// An integer not in its one canonical spelling: a leading zero, `-0` or a `+`.
    #[error(
        code = "CANON_014",
        message = "The integer at byte {offset} is not in canonical form",
        status = 422
    )]
    NonCanonicalInteger {
        /// Where the number starts.
        offset: usize,
    },

    /// An integer outside ±(2⁵³−1), the range every JSON implementation represents exactly.
    #[error(
        code = "CANON_015",
        message = "The integer at byte {offset} is outside the exactly representable range",
        status = 422
    )]
    IntegerOutOfRange {
        /// Where the number starts.
        offset: usize,
    },

    /// An escape the canonical form does not write: `\/`, `\u0041`, an escaped non-control.
    #[error(
        code = "CANON_016",
        message = "The string escape at byte {offset} is not the canonical spelling",
        status = 422
    )]
    ForbiddenEscape {
        /// Where the backslash is.
        offset: usize,
    },

    /// A control character written raw inside a string; the canonical form escapes it.
    #[error(
        code = "CANON_017",
        message = "A control character is not escaped at byte {offset}",
        status = 422
    )]
    UnescapedControl {
        /// Where the character is.
        offset: usize,
    },

    /// A `\u` escape naming half of a surrogate pair on its own.
    #[error(
        code = "CANON_018",
        message = "A string names an unpaired surrogate at byte {offset}",
        status = 422
    )]
    LoneSurrogate {
        /// Where the escape starts.
        offset: usize,
    },

    /// A string that is not in Unicode Normalization Form C (registry profile only).
    #[error(
        code = "CANON_019",
        message = "The string starting at byte {offset} is not in Unicode Normalization Form C",
        status = 422
    )]
    NotNfc {
        /// Where the string opens.
        offset: usize,
    },

    /// A string using a code point Unicode has not assigned (registry profile only).
    #[error(
        code = "CANON_020",
        message = "The string starting at byte {offset} uses an unassigned code point",
        status = 422
    )]
    Unassigned {
        /// Where the string opens.
        offset: usize,
    },

    /// A comment; JSON has none.
    #[error(
        code = "CANON_021",
        message = "The document has a comment at byte {offset}",
        status = 422
    )]
    Comment {
        /// Where the comment starts.
        offset: usize,
    },

    /// Canonical bytes that do not have the shape the reading type expects; the detail is for the log.
    #[error(
        code = "CANON_022",
        message = "The document does not have the expected shape",
        developer_message = "canonical document shape: {detail}",
        status = 422
    )]
    Shape {
        /// What the reading type refused.
        detail: String,
    },

    /// A value that has no canonical encoding, raised when writing: a defect in the caller.
    #[error(
        code = "CANON_023",
        message = "A value could not be written in canonical form",
        developer_message = "unrepresentable in canonical form: {detail}",
        status = 500
    )]
    Unrepresentable {
        /// What could not be written.
        detail: String,
    },

    /// A canonical document that re-encodes to different bytes.
    #[error(
        code = "CANON_024",
        message = "The document is not in its normal form",
        status = 422
    )]
    NotNormal,

    /// Bytes whose SHA-256 is not the digest they were expected to have.
    #[error(
        code = "CANON_025",
        message = "The document does not match its digest",
        status = 422
    )]
    DigestMismatch,

    /// A digest string that is not one canonical SRI, pin or bare base64url spelling.
    #[error(
        code = "CANON_026",
        message = "The digest is not in its canonical form",
        status = 422
    )]
    MalformedDigest,
}

impl CanonicalError {
    /// The refusal's name, from a closed set — for a metric label or a log field.
    #[must_use]
    pub const fn rule(&self) -> &'static str {
        match self {
            Self::TooLarge { .. } => "too_large",
            Self::TooDeep { .. } => "too_deep",
            Self::TooManyValues { .. } => "too_many_values",
            Self::StringTooLong { .. } => "string_too_long",
            Self::TooManyMembers { .. } => "too_many_members",
            Self::InvalidUtf8 { .. } => "invalid_utf8",
            Self::ByteOrderMark => "byte_order_mark",
            Self::Whitespace { .. } => "whitespace",
            Self::Syntax { .. } => "syntax",
            Self::TrailingBytes { .. } => "trailing_bytes",
            Self::DuplicateKey { .. } => "duplicate_key",
            Self::KeyOrder { .. } => "key_order",
            Self::Float { .. } => "float",
            Self::NonCanonicalInteger { .. } => "non_canonical_integer",
            Self::IntegerOutOfRange { .. } => "integer_out_of_range",
            Self::ForbiddenEscape { .. } => "forbidden_escape",
            Self::UnescapedControl { .. } => "unescaped_control",
            Self::LoneSurrogate { .. } => "lone_surrogate",
            Self::NotNfc { .. } => "not_nfc",
            Self::Unassigned { .. } => "unassigned",
            Self::Comment { .. } => "comment",
            Self::Shape { .. } => "shape",
            Self::Unrepresentable { .. } => "unrepresentable",
            Self::NotNormal => "not_normal",
            Self::DigestMismatch => "digest_mismatch",
            Self::MalformedDigest => "malformed_digest",
        }
    }
}

impl serde::de::Error for CanonicalError {
    fn custom<T: Display>(detail: T) -> Self {
        Self::Shape {
            detail: detail.to_string(),
        }
    }
}

impl serde::ser::Error for CanonicalError {
    fn custom<T: Display>(detail: T) -> Self {
        Self::Unrepresentable {
            detail: detail.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::CanonicalError;
    use fundamental_core::Error as _;

    #[test]
    fn an_input_refusal_is_422_and_names_no_content() {
        let error = CanonicalError::KeyOrder { offset: 17 };
        assert_eq!(error.error_code(), "CANON_012");
        assert_eq!(error.http_status_code(), 422);
        assert_eq!(error.rule(), "key_order");
        assert!(error.to_string().contains("17"));
    }

    #[test]
    fn an_oversized_input_is_413() {
        let error = CanonicalError::TooLarge {
            len: 70_000,
            max: 65_536,
        };
        assert_eq!(error.error_code(), "CANON_001");
        assert_eq!(error.http_status_code(), 413);
    }

    #[test]
    fn a_writer_refusal_is_the_callers_defect() {
        let error = <CanonicalError as serde::ser::Error>::custom("a float");
        assert_eq!(error.error_code(), "CANON_023");
        assert_eq!(error.http_status_code(), 500);
    }

    #[test]
    fn a_shape_refusal_keeps_its_detail_out_of_the_message() {
        let error = <CanonicalError as serde::de::Error>::custom("unknown field `secret`");
        assert_eq!(error.error_code(), "CANON_022");
        assert!(!error.to_string().contains("secret"));
    }
}
