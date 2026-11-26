/// How the server's value for a header meets a value a handler set for the same name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeaderCombine {
    /// The policy's value replaces any a handler set, for a header a browser reads once.
    Replace,
    /// The policy's value is added once beside a handler's own, for `Content-Security-Policy` and `X-Robots-Tag`.
    Alongside,
}
