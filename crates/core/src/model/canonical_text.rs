/// A value with exactly one text spelling, which every codec writes.
///
/// It carries no parse on purpose: reading back goes through the derive's codecs or
/// [`Admit::admit`](crate::model::Admit::admit), and a public `parse` would bypass both.
pub trait CanonicalText {
    /// The longest canonical spelling, in bytes; a decoder refuses anything longer before
    /// it parses a byte of it.
    const MAX_LEN: usize;

    /// The one canonical spelling.
    fn as_canonical(&self) -> &str;
}
