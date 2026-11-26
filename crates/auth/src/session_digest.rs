//! [`SessionDigest`] — the stored form of a session handle.

use fundamental_encryption::KeyedTag;

use crate::SessionHandleKey;

/// A session handle's digest: a keyed tag under [`SessionHandleKey`], the only stored form.
pub type SessionDigest = KeyedTag<SessionHandleKey>;
