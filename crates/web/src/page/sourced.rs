/// A loader beside the path of the `loader.rs` it was declared in.
pub struct Sourced<L> {
    pub(crate) file: &'static str,
    pub(crate) loader: L,
}
