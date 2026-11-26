/// A typed JetStream stream: a zero-sized marker carrying the stream's `NAME`.
///
/// `#[derive(Stream)]` makes a unit struct the name, so a typo is a compile error.
pub trait Stream {
    /// The stream's name on the wire.
    const NAME: &'static str;
}
