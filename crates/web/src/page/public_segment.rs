/// A path parameter whose value is one of a fixed set of words, so it never carries a record id.
///
/// Derive it (`#[derive(PublicSegment)]`) on a fieldless enum; never implement it by hand.
pub trait PublicSegment: Sized + Send + Sync + 'static {
    /// The variant `segment` names, or `None` for a word that is not one of them.
    fn parse(segment: &str) -> Option<Self>;

    /// This variant's word in an address.
    fn segment(&self) -> &'static str;

    #[doc(hidden)]
    fn __derived_for_a_fieldless_enum();
}
