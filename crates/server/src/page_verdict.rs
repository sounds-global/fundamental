use crate::ReturnPath;

/// What the gate decided about one request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PageVerdict {
    /// Serve it.
    Allow,
    /// Send a person to sign in, and back afterwards when there is a page to come back to.
    Redirect(Option<ReturnPath>),
    /// Answer a programmatic caller with the framework's own `401`.
    Refuse,
}
