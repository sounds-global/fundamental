use crate::error::NatsResult;
use crate::message::Publish;

/// A closed set of messages: an enum whose variants each carry a payload on their own
/// subject. Derive it with `#[derive(MessageSet)]`.
///
/// The receiving counterpart to [`Publish`]: [`from_parts`](MessageSet::from_parts) routes a
/// delivery to its variant. Each variant's wire form is its inner payload, never the enum.
pub trait MessageSet: Publish + Sized {
    /// Every subject any variant maps to; never empty.
    const SUBJECTS: &'static [&'static str];

    /// Routes a raw delivery on `subject` to its variant, decoding the inner payload.
    ///
    /// # Errors
    /// Returns [`NatsError::UnknownSubject`](crate::NatsError::UnknownSubject) if no variant
    /// claims `subject`, or [`NatsError::Serialization`](crate::NatsError::Serialization) if
    /// the payload fails to decode.
    fn from_parts(subject: &str, bytes: &[u8]) -> NatsResult<Self>;
}
