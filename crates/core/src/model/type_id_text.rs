//! [`TypeIdText`] — a TypeID v0.3, read strictly.

use super::handle_prefix::is_type_id_prefix;
use super::{HandleRefusal, Opaque128};

/// A TypeID v0.3 in its one spelling: a prefix, `_`, and 26 Crockford characters, or the 26 characters alone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TypeIdText<'a> {
    prefix: &'a str,
    value: Opaque128,
}

impl<'a> TypeIdText<'a> {
    /// The longest TypeID: a 63-character prefix, the separator and the suffix.
    pub const MAX_LEN: usize = 90;

    /// Read `text` strictly.
    ///
    /// # Errors
    ///
    /// [`HandleRefusal::Shape`] for a text longer than 90 bytes, a separator with nothing
    /// before it, or a prefix TypeID does not allow; [`HandleRefusal::Suffix`] when the
    /// suffix is not 26 lower-case Crockford characters of at most 128 bits.
    pub fn parse(text: &'a str) -> Result<Self, HandleRefusal> {
        let (prefix, suffix) = split(text)?;
        if !is_type_id_prefix(prefix) {
            return Err(HandleRefusal::Shape);
        }
        let value = Opaque128::parse_crockford(suffix).map_err(|_| HandleRefusal::Suffix)?;
        Ok(Self { prefix, value })
    }

    /// The prefix, empty for a bare suffix.
    #[must_use]
    pub const fn prefix(&self) -> &'a str {
        self.prefix
    }

    /// The 128 bits.
    #[must_use]
    pub const fn value(&self) -> Opaque128 {
        self.value
    }
}

/// Splits at the last `_`: a prefix may contain underscores and a suffix never does.
pub(crate) fn split(text: &str) -> Result<(&str, &str), HandleRefusal> {
    if text.len() > TypeIdText::MAX_LEN {
        return Err(HandleRefusal::Shape);
    }
    match text.rfind('_') {
        Some(0) => Err(HandleRefusal::Shape),
        Some(at) => Ok((&text[..at], &text[at + 1..])),
        None => Ok(("", text)),
    }
}

#[cfg(test)]
mod tests {
    use super::TypeIdText;
    use crate::model::HandleRefusal;

    #[test]
    fn a_prefixed_typeid_reads() {
        let read = TypeIdText::parse("user_01h455vb4pex5vsknk084sn02q");
        assert_eq!(read.map(|typeid| typeid.prefix()), Ok("user"));
    }

    #[test]
    fn a_separator_with_no_prefix_is_a_shape_refusal() {
        assert_eq!(
            TypeIdText::parse("_00000000000000000000000000"),
            Err(HandleRefusal::Shape)
        );
    }

    #[test]
    fn a_long_text_is_refused_before_it_is_split() {
        let long = "a".repeat(200);
        assert_eq!(TypeIdText::parse(&long), Err(HandleRefusal::Shape));
    }
}
