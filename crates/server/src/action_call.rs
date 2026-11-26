/// What a page's server render sends a mounted action: its `path?query` and the request body.
#[derive(Debug, Default)]
pub(crate) struct ActionCall {
    /// The address, `path` or `path?query`, the action is called at.
    pub(crate) target: Option<String>,
    /// The request body.
    pub(crate) body: Vec<u8>,
}
