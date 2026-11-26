/// Selects a blocking server query for a generated action hook.
#[doc(hidden)]
pub struct BlockingSource<S>(pub(super) S);

impl<S: Copy> Copy for BlockingSource<S> {}

impl<S: Clone> Clone for BlockingSource<S> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}
