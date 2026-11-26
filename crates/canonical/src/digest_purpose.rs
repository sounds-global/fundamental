use crate::{CanonicalJson, DigestContext};

/// A canonical document that is hashed for exactly one purpose.
pub trait DigestPurpose: CanonicalJson {
    /// The context every digest of this document is derived under.
    type Context: DigestContext;
}
