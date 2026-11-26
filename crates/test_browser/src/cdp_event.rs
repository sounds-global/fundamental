use serde_json::Value;

/// One event the browser sent: which page it is about, what happened, and its parameters.
#[derive(Clone, Debug)]
pub struct CdpEvent {
    /// The page session it belongs to; `None` for the browser itself.
    pub session: Option<String>,
    /// The DevTools event, e.g. `Network.requestWillBeSent`.
    pub method: String,
    /// Its parameters.
    pub params: Value,
}
