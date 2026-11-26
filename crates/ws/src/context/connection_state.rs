//! [`ConnectionState`]: a single value shared across a connection's messages.

use std::any::Any;
use std::fmt;
use std::sync::OnceLock;

/// One type-erased value set once per connection and read by every later message on it.
#[derive(Default)]
pub(crate) struct ConnectionState {
    value: OnceLock<Box<dyn Any + Send + Sync>>,
}

impl ConnectionState {
    pub(crate) fn set<T: Any + Send + Sync>(&self, value: T) -> Result<(), AlreadySet> {
        self.value.set(Box::new(value)).map_err(|_| AlreadySet)
    }

    pub(crate) fn get<T: Any + Send + Sync>(&self) -> Option<&T> {
        self.value.get().and_then(|boxed| boxed.downcast_ref::<T>())
    }

    /// Whether a value of any type has been stored.
    pub(crate) fn is_set(&self) -> bool {
        self.value.get().is_some()
    }
}

/// Returned when [`ConnectionState::set`] is called a second time.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct AlreadySet;

impl fmt::Debug for ConnectionState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ConnectionState")
            .field("set", &self.value.get().is_some())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stores_and_reads_a_value_by_type() {
        let state = ConnectionState::default();
        assert!(state.get::<u32>().is_none());
        assert!(!state.is_set());
        state.set(7u32).expect("set");
        assert!(state.is_set());
        assert_eq!(state.get::<u32>(), Some(&7));
    }

    #[test]
    fn a_second_set_is_rejected() {
        let state = ConnectionState::default();
        state.set(1u32).expect("first set");
        assert_eq!(state.set(2u32), Err(AlreadySet));
        assert_eq!(state.get::<u32>(), Some(&1));
    }

    #[test]
    fn get_with_the_wrong_type_returns_none() {
        let state = ConnectionState::default();
        state.set(1u32).expect("set");
        assert!(state.get::<String>().is_none());
    }
}
