//! [`PastTheBound`] — the failure a bounded read or write gives past its bound.

/// The failure a bounded sink or source gives past its bound, carried inside an
/// [`std::io::Error`] so the bound, not the I/O, is named.
#[derive(Debug)]
pub(crate) struct PastTheBound;

impl PastTheBound {
    /// Whether `cause` is this bound's failure.
    pub(crate) fn caused(cause: &std::io::Error) -> bool {
        cause.get_ref().is_some_and(|inner| inner.is::<Self>())
    }
}

impl std::fmt::Display for PastTheBound {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("the package passes its bound")
    }
}

impl std::error::Error for PastTheBound {}
