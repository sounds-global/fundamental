/// Stores the source closure that feeds changing input values to a query action.
#[doc(hidden)]
pub struct ActionQuerySource<F>(pub(super) F);
