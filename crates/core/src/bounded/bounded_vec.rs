//! [`BoundedVec`] — a list that cannot hold more than `MAX` elements, from the moment it is read.

#[cfg(not(feature = "std"))]
use alloc::vec::Vec;
use core::fmt;
use core::marker::PhantomData;
use core::ops::Deref;

use serde::de::{self, Deserialize, Deserializer, SeqAccess, Visitor};
use serde::{Serialize, Serializer};

/// A list of at most `MAX` elements.
///
/// The bound holds while the list is being read: element `MAX + 1` is refused before allocation.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct BoundedVec<T, const MAX: usize>(Vec<T>);

impl<T, const MAX: usize> BoundedVec<T, MAX> {
    const NON_EMPTY_BOUND: () = assert!(MAX > 0, "a BoundedVec bound must be at least 1");

    /// The most elements this type can hold.
    pub const MAX: usize = MAX;

    /// An empty list.
    #[must_use]
    pub const fn new() -> Self {
        let () = Self::NON_EMPTY_BOUND;
        Self(Vec::new())
    }

    /// Wraps `items` if there are at most `MAX` of them.
    ///
    /// # Errors
    /// Returns the items back if there are more than `MAX`.
    pub fn try_from_vec(items: Vec<T>) -> Result<Self, Vec<T>> {
        let () = Self::NON_EMPTY_BOUND;
        if items.len() <= MAX {
            Ok(Self(items))
        } else {
            Err(items)
        }
    }

    /// Appends `item` if the list is not full.
    ///
    /// # Errors
    /// Returns `item` back if the list already holds `MAX` elements.
    pub fn try_push(&mut self, item: T) -> Result<(), T> {
        if self.0.len() < MAX {
            self.0.push(item);
            Ok(())
        } else {
            Err(item)
        }
    }

    /// The elements, as a slice.
    #[must_use]
    pub fn as_slice(&self) -> &[T] {
        &self.0
    }

    /// Takes the elements out.
    #[must_use]
    pub fn into_vec(self) -> Vec<T> {
        self.0
    }

    /// Each element through `f`, under the same bound: a list that fit still fits.
    #[must_use]
    pub fn map<U>(self, f: impl FnMut(T) -> U) -> BoundedVec<U, MAX> {
        BoundedVec(self.0.into_iter().map(f).collect())
    }
}

impl<T, const MAX: usize> Default for BoundedVec<T, MAX> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T, const MAX: usize> Deref for BoundedVec<T, MAX> {
    type Target = [T];

    fn deref(&self) -> &[T] {
        &self.0
    }
}

impl<T: fmt::Debug, const MAX: usize> fmt::Debug for BoundedVec<T, MAX> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(&self.0).finish()
    }
}

impl<T, const MAX: usize> IntoIterator for BoundedVec<T, MAX> {
    type Item = T;
    type IntoIter = <Vec<T> as IntoIterator>::IntoIter;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl<'a, T, const MAX: usize> IntoIterator for &'a BoundedVec<T, MAX> {
    type Item = &'a T;
    type IntoIter = core::slice::Iter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

impl<T: Serialize, const MAX: usize> Serialize for BoundedVec<T, MAX> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

impl<'de, T: Deserialize<'de>, const MAX: usize> Deserialize<'de> for BoundedVec<T, MAX> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let () = Self::NON_EMPTY_BOUND;
        deserializer.deserialize_seq(BoundedVisitor::<T, MAX>(PhantomData))
    }
}

struct BoundedVisitor<T, const MAX: usize>(PhantomData<T>);

impl<'de, T: Deserialize<'de>, const MAX: usize> Visitor<'de> for BoundedVisitor<T, MAX> {
    type Value = BoundedVec<T, MAX>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "a list of at most {MAX} elements")
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
        // The input's own size hint is untrusted: it only ever lowers the reservation.
        let mut items = Vec::with_capacity(seq.size_hint().unwrap_or(0).min(MAX));
        while let Some(item) = seq.next_element()? {
            if items.len() == MAX {
                return Err(de::Error::invalid_length(MAX + 1, &self));
            }
            items.push(item);
        }
        Ok(BoundedVec(items))
    }
}

#[cfg(test)]
mod tests {
    use core::cell::Cell;

    use super::*;

    #[test]
    fn a_mapped_list_keeps_its_order_under_the_same_bound() {
        let list = BoundedVec::<u8, 3>::try_from_vec(vec![1, 2, 3]).expect("at the bound");
        let doubled: BoundedVec<u16, 3> = list.map(|n| u16::from(n) * 2);
        assert_eq!(doubled.as_slice(), &[2, 4, 6]);
    }

    #[test]
    fn a_list_at_the_bound_deserialises() {
        let list: BoundedVec<u8, 3> = serde_json::from_str("[1,2,3]").expect("at the bound");
        assert_eq!(list.as_slice(), &[1, 2, 3]);
    }

    #[test]
    fn one_over_the_bound_is_refused_before_it_is_allocated() {
        thread_local! { static SEEN: Cell<usize> = const { Cell::new(0) }; }
        #[derive(Debug)]
        struct Counted;
        impl<'de> Deserialize<'de> for Counted {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                u8::deserialize(deserializer)?;
                SEEN.with(|seen| seen.set(seen.get() + 1));
                Ok(Counted)
            }
        }
        SEEN.with(|seen| seen.set(0));
        let error = serde_json::from_str::<BoundedVec<Counted, 3>>("[1,2,3,4,5,6,7,8]")
            .expect_err("over the bound");
        assert!(error.to_string().contains("at most 3"), "{error}");
        assert_eq!(
            SEEN.with(Cell::get),
            4,
            "element four is read to learn the list is too long, and nothing after it"
        );
    }

    #[test]
    fn size_hint_cannot_preallocate_past_the_bound() {
        let list: BoundedVec<u8, 2> = serde_json::from_str("[]").expect("empty");
        assert!(list.0.capacity() <= 2);
    }

    #[test]
    fn construction_and_push_respect_the_bound() {
        assert!(BoundedVec::<u8, 2>::try_from_vec(Vec::from([1, 2, 3])).is_err());
        let mut list = BoundedVec::<u8, 2>::new();
        assert!(list.try_push(1).is_ok());
        assert!(list.try_push(2).is_ok());
        assert_eq!(list.try_push(3), Err(3));
    }

    #[test]
    fn it_serialises_as_a_plain_list() {
        let list = BoundedVec::<u8, 4>::try_from_vec(Vec::from([7, 8])).expect("fits");
        assert_eq!(serde_json::to_string(&list).expect("serialises"), "[7,8]");
    }
}
