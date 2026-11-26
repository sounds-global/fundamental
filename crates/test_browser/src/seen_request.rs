/// A request the page made, as the browser saw it leave.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SeenRequest {
    /// The method.
    pub method: String,
    /// The whole URL, query included — a fragment never leaves a browser.
    pub url: String,
    /// The body, when it had one the browser would show.
    pub body: Option<String>,
}
