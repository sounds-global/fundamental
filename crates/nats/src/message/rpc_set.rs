/// A closed set of request/reply RPCs: an enum whose variants each pair a request message
/// with its reply, both on subjects derived from the enum and variant names. Derive it with
/// `#[derive(RpcSet)]`.
///
/// The derive emits the [`Message`](crate::Message) impls: requests on `{enum}.{variant}` and
/// replies on `{enum}.{variant}.reply`, unless a `#[subject = "…"]` overrides. The enum is never
/// constructed.
pub trait RpcSet {
    /// Every request subject in the set, one per variant; never empty.
    const REQUEST_SUBJECTS: &'static [&'static str];

    /// Every reply subject in the set: each request subject with `.reply`.
    const REPLY_SUBJECTS: &'static [&'static str];
}
