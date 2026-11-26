/// The default source state for an action hook.
#[doc(hidden)]
pub struct NoQuerySource;

impl Copy for NoQuerySource {}

impl Clone for NoQuerySource {
    fn clone(&self) -> Self {
        *self
    }
}
