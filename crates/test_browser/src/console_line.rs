/// A line the page wrote to its console, or an exception nobody caught.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConsoleLine {
    /// `log`, `warning`, `error`, … or `exception`.
    pub level: String,
    /// The text.
    pub text: String,
}
