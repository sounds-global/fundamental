/// Whether the server's answer to a commit reached this process, kept apart as `E` may flatten it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum CommitAnswer {
    /// The server answered — success, a refusal, or no commit was attempted.
    Given,
    /// The commit was sent and its answer lost; whether it applied is unknown.
    Lost,
}
