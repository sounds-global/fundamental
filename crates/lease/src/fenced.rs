/// What a fenced transaction did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Fenced<T> {
    /// The work ran and committed under the lease.
    Done(T),
    /// The work key was already committed, so the work did not run again.
    AlreadyDone,
}
