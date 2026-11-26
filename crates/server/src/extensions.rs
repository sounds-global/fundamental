use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

/// Values a [`Router`](crate::Router) hands every handler, at most one of each type.
#[derive(Clone, Default)]
pub(crate) struct Extensions {
    values: Arc<HashMap<TypeId, Arc<dyn Any + Send + Sync>>>,
}

impl Extensions {
    /// These extensions with `value` in place of any earlier value of its type.
    pub(crate) fn with<T: Any + Send + Sync>(mut self, value: T) -> Self {
        Arc::make_mut(&mut self.values).insert(TypeId::of::<T>(), Arc::new(value));
        self
    }

    /// The value of type `T`, if one was added.
    pub(crate) fn get<T: Any + Send + Sync>(&self) -> Option<&T> {
        self.values
            .get(&TypeId::of::<T>())
            .and_then(|value| value.downcast_ref::<T>())
    }
}

impl fmt::Debug for Extensions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Extensions")
            .field("count", &self.values.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_later_value_of_a_type_replaces_the_earlier() {
        let extensions = Extensions::default().with(1_u8).with(2_u8).with("word");
        assert_eq!(extensions.get::<u8>(), Some(&2));
        assert_eq!(extensions.get::<&str>(), Some(&"word"));
        assert_eq!(extensions.get::<u16>(), None);
    }
}
