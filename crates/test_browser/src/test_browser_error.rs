use fundamental_macros::Error;

/// What can go wrong driving the test browser.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum TestBrowserError {
    /// The browser could not be brought up or reached.
    #[error(
        code = "TBR_001",
        message = "the test browser is unavailable: {detail}",
        status = 503
    )]
    Unavailable {
        /// What failed.
        detail: String,
    },

    /// The browser answered a command with an error.
    #[error(
        code = "TBR_002",
        message = "the browser refused {method}: {detail}",
        status = 500
    )]
    Protocol {
        /// The DevTools method.
        method: String,
        /// The browser's own account.
        detail: String,
    },

    /// Something the test waited for did not happen in time.
    #[error(
        code = "TBR_003",
        message = "waited {waited_ms} ms for {what}",
        status = 504
    )]
    Timeout {
        /// What was awaited.
        what: String,
        /// How long.
        waited_ms: u64,
    },

    /// Script the test ran in the page threw, or answered something unreadable.
    #[error(
        code = "TBR_004",
        message = "the page's script failed: {detail}",
        status = 500
    )]
    Script {
        /// The exception or the decoding failure.
        detail: String,
    },
}
