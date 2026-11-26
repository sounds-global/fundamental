//! Naming for ephemeral test stores: the name says when the store was born, so a sweep can
//! tell a leftover from a store in use without a creation time the engines do not record.

mod ephemeral_name;
mod ephemeral_name_error;
mod held_name;
mod stale_cutoff;

pub use ephemeral_name::EphemeralName;
pub use ephemeral_name_error::EphemeralNameError;
pub use held_name::HeldName;
pub use stale_cutoff::{STALE_AFTER, stale_cutoff, stale_cutoff_millis};
