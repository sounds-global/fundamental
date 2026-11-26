/// The JWS protected-header fields a caller chooses.
///
/// No `alg` field: [`sign_compact`](super::sign_compact) writes it from the signer's type, so a
/// header cannot name another algorithm (RFC 8725 §2.1).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct JwsHeader<'a> {
    /// The key identifier (`kid`).
    pub kid: Option<&'a str>,
    /// The media type of the complete JWS (`typ`).
    pub typ: Option<&'a str>,
}

impl<'a> JwsHeader<'a> {
    /// A header naming `kid` and `typ`.
    #[must_use]
    pub fn new(kid: Option<&'a str>, typ: Option<&'a str>) -> Self {
        Self { kid, typ }
    }
}
