//! A crate's messages said in the reader's language.

mod brand_terms;
mod message;
mod words;

pub use brand_terms::{BrandTerm, BrandTerms, BrandTermsInstalled, install_brand_terms};
pub use fundamental_macros::Messages;
pub use message::{Catalogs, FUNCTION_NAMES, Message, Value};
pub use words::{FieldWords, Noun};

#[doc(hidden)]
pub use fundamental_core::Variant as __Variant;

/// Whether a variant's `fields` are the inputs its message `declared`, by name and in order.
#[doc(hidden)]
#[must_use]
pub const fn __inputs_are(declared: &[&str], fields: &[&str]) -> bool {
    if declared.len() != fields.len() {
        return false;
    }
    let mut at = 0;
    while at < declared.len() {
        let (one, other) = (declared[at].as_bytes(), fields[at].as_bytes());
        if one.len() != other.len() {
            return false;
        }
        let mut byte = 0;
        while byte < one.len() {
            if one[byte] != other[byte] {
                return false;
            }
            byte += 1;
        }
        at += 1;
    }
    true
}

/// The crate's messages, as its build generated them: the module `m`.
#[macro_export]
macro_rules! messages {
    () => {
        /// This crate's messages, one function per message, each said in the reader's language.
        pub mod m {
            #[doc(hidden)]
            pub use $crate as __messages;
            include!(concat!(env!("OUT_DIR"), "/messages.rs"));
        }
    };
}
