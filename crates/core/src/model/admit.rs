use core::fmt::{Debug, Display};

use super::CanonicalText;

/// A value whose validity depends on where it is read, so it can only be read into a context.
///
/// It implements no `Deserialize`: a wire field is [`Unadmitted<T>`](crate::model::Unadmitted), and `admit` is the
/// only way to a `T`.
pub trait Admit: CanonicalText + Sized {
    /// What admission is judged against — a deployment's cell, a host.
    type Context: ?Sized;

    /// Why a text was not admitted; its `Display` must not echo the text.
    type Refusal: Display + Debug;

    /// Parse `text` and admit it in `context`, or refuse.
    ///
    /// # Errors
    ///
    /// The type's refusal when `text` is malformed or not valid in `context`.
    fn admit(text: &str, context: &Self::Context) -> Result<Self, Self::Refusal>;
}
