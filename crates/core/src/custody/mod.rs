//! How a signing key is held, as a value every target can name.

mod key_protection;
mod unknown_key_protection;

pub use key_protection::KeyProtection;
pub use unknown_key_protection::UnknownKeyProtection;
