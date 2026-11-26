//! [`AbsoluteUrl`] — a page's address on the origin it is published at.

#[cfg(not(feature = "std"))]
use alloc::string::String;
use core::fmt;

/// An absolute URL this surface publishes: its [`WebOrigin`](crate::address::WebOrigin)
/// followed by an address its [`AddressSpace`](crate::address::AddressSpace) built.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AbsoluteUrl(String);

impl AbsoluteUrl {
    pub(crate) fn from_parts(origin: &str, address: &str) -> Self {
        let mut url = String::with_capacity(origin.len() + address.len());
        url.push_str(origin);
        url.push_str(address);
        Self(url)
    }

    /// The URL as text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The URL, owned.
    #[must_use]
    pub fn into_string(self) -> String {
        self.0
    }
}

impl fmt::Display for AbsoluteUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for AbsoluteUrl {
    fn as_ref(&self) -> &str {
        &self.0
    }
}
