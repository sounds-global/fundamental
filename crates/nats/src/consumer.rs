/// A typed JetStream consumer: a zero-sized marker carrying the consumer's `NAME`.
///
/// `#[derive(Consumer)]` makes a unit struct the name, so a typo is a compile error.
pub trait Consumer {
    /// The consumer's name on the wire.
    const NAME: &'static str;
}
