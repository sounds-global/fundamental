#[cfg(not(feature = "std"))]
use alloc::string::String;
use core::fmt;
use core::marker::PhantomData;

use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::Admit;

/// A `T` as it arrived on a wire, not yet admitted where it is being read.
///
/// Deserializing checks only the length against [`CanonicalText::MAX_LEN`]; [`admit`](Self::admit) is the only
/// way to a `T`, so a handler cannot reach one without naming its context.
///
/// [`CanonicalText::MAX_LEN`]: crate::model::CanonicalText::MAX_LEN
pub struct Unadmitted<T> {
    text: String,
    target: PhantomData<fn() -> T>,
}

impl<T: Admit> Unadmitted<T> {
    /// Text as it was received, kept unadmitted; `None` when it is longer than `T` allows.
    #[must_use]
    pub fn received(text: &str) -> Option<Self> {
        (text.len() <= T::MAX_LEN).then(|| Self {
            text: String::from(text),
            target: PhantomData,
        })
    }

    /// Admit the text in `context`.
    ///
    /// # Errors
    ///
    /// `T`'s refusal when the text is malformed or not valid in `context`.
    pub fn admit(&self, context: &T::Context) -> Result<T, T::Refusal> {
        T::admit(&self.text, context)
    }

    /// Whether the text is exactly `value`'s canonical spelling, admitting nothing.
    #[must_use]
    pub fn is(&self, value: &T) -> bool {
        self.text == value.as_canonical()
    }
}

/// A value already admitted, going back onto a wire — what a client sends.
impl<T: Admit> From<&T> for Unadmitted<T> {
    fn from(value: &T) -> Self {
        Self {
            text: String::from(value.as_canonical()),
            target: PhantomData,
        }
    }
}

impl<T> Clone for Unadmitted<T> {
    fn clone(&self) -> Self {
        Self {
            text: self.text.clone(),
            target: PhantomData,
        }
    }
}

impl<T> PartialEq for Unadmitted<T> {
    fn eq(&self, other: &Self) -> bool {
        self.text == other.text
    }
}

impl<T> Eq for Unadmitted<T> {}

/// Shows the length, never the text: until it is admitted, it is whatever a caller sent.
impl<T> fmt::Debug for Unadmitted<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "Unadmitted({} bytes)", self.text.len())
    }
}

impl<T> Serialize for Unadmitted<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.text)
    }
}

impl<'de, T: Admit> Deserialize<'de> for Unadmitted<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_str(Bounded::<T>(PhantomData))
    }
}

struct Bounded<T>(PhantomData<fn() -> T>);

impl<T: Admit> Visitor<'_> for Bounded<T> {
    type Value = Unadmitted<T>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "a string of at most {} bytes", T::MAX_LEN)
    }

    fn visit_str<E: de::Error>(self, text: &str) -> Result<Unadmitted<T>, E> {
        if text.len() > T::MAX_LEN {
            return Err(E::invalid_length(text.len(), &self));
        }
        Ok(Unadmitted {
            text: String::from(text),
            target: PhantomData,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::Unadmitted;
    use crate::model::{Admit, CanonicalText};

    #[derive(Debug, PartialEq)]
    struct Region(&'static str);

    impl CanonicalText for Region {
        const MAX_LEN: usize = 4;
        fn as_canonical(&self) -> &str {
            self.0
        }
    }

    impl Admit for Region {
        type Context = [&'static str];
        type Refusal = &'static str;
        fn admit(text: &str, allowed: &[&'static str]) -> Result<Self, &'static str> {
            allowed
                .iter()
                .find(|region| **region == text)
                .map(|region| Region(region))
                .ok_or("not admitted here")
        }
    }

    #[test]
    fn a_value_reaches_its_type_only_through_admission() {
        let wire: Unadmitted<Region> = serde_json::from_str("\"eu\"").expect("a bounded string");
        assert_eq!(wire.admit(&["eu"]), Ok(Region("eu")));
        assert_eq!(wire.admit(&["us"]), Err("not admitted here"));
    }

    #[test]
    fn a_text_is_compared_with_a_value_without_being_admitted() {
        let wire: Unadmitted<Region> = serde_json::from_str("\"eu\"").expect("a bounded string");
        assert!(wire.is(&Region("eu")));
        assert!(!wire.is(&Region("us")));
    }

    #[test]
    fn an_oversized_text_is_refused_before_it_is_kept() {
        assert!(serde_json::from_str::<Unadmitted<Region>>("\"europe\"").is_err());
    }

    #[test]
    fn received_text_is_bounded_and_still_unadmitted() {
        let received = Unadmitted::<Region>::received("eu").expect("within the bound");
        assert_eq!(received.admit(&["us"]), Err("not admitted here"));
        assert!(Unadmitted::<Region>::received("europe").is_none());
    }

    #[test]
    fn debug_never_prints_what_a_caller_sent() {
        let wire: Unadmitted<Region> = serde_json::from_str("\"eu\"").expect("a bounded string");
        assert_eq!(format!("{wire:?}"), "Unadmitted(2 bytes)");
    }

    #[test]
    fn an_admitted_value_goes_back_onto_the_wire_as_its_canonical_text() {
        let wire = Unadmitted::from(&Region("eu"));
        assert_eq!(serde_json::to_string(&wire).ok().as_deref(), Some("\"eu\""));
    }
}
