//! [`Noun`] and [`FieldWords`]: a record type's words, as its crate's messages say them.

use crate::Message;

/// A record type's name in each form a sentence needs, implemented by `#[derive(Messages)]`.
pub trait Noun {
    /// One record: *article*.
    fn one() -> Message;
    /// Several records: *articles*.
    fn many() -> Message;
    /// One record with its definite article: *the article*.
    fn the() -> Message;
    /// A record yet to be made: *new article*.
    fn new_one() -> Message;
    /// The record before the reader: *this article*.
    fn this() -> Message;
}

/// The words of a type's fields, implemented by `#[derive(Messages)]` on a struct.
pub trait FieldWords {
    /// The words of the field named `name`; `None` for a name that is no field.
    fn field(name: &str) -> Option<Message>;

    /// How `value` of the closed field `field` reads; `None` for an open field or unknown value.
    fn value(field: &str, value: &str) -> Option<Message>;
}
