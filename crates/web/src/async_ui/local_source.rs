/// Selects browser-local query and mutation execution for a generated action hook.
#[doc(hidden)]
pub struct LocalSource<S>(pub(super) S);

impl<S: Copy> Copy for LocalSource<S> {}

impl<S: Clone> Clone for LocalSource<S> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}
