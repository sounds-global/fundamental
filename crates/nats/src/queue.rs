/// A typed NATS queue group: a zero-sized marker carrying the group's `NAME`.
///
/// `#[derive(Queue)]` makes a unit struct the name, so a misspelled group is a compile error.
pub trait Queue {
    /// The queue group's name on the wire.
    const NAME: &'static str;
}
