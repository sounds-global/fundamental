//! Singleton work behind a fencing lease.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod acquisition;
mod decision;
mod fenced;
mod held_lease;
mod lease;
mod lease_config;
mod lease_error;
mod lease_holder;
mod lease_migrations;
mod lease_name;
mod lease_observer;
mod lease_row;
mod lease_settings;
mod lease_state;
mod lease_work_row;
mod leased_guard;
mod leased_surface;
mod leased_work;
mod loss_reason;
mod released;
mod work_key;

pub use acquisition::Acquisition;
pub use fenced::Fenced;
pub use held_lease::HeldLease;
pub use lease::Lease;
pub use lease_config::LeaseConfig;
pub use lease_error::LeaseError;
pub use lease_holder::LeaseHolder;
pub use lease_migrations::LEASE_MIGRATIONS;
pub use lease_name::LeaseName;
pub use lease_observer::LeaseObserver;
pub use lease_row::LeaseRow;
pub use lease_settings::LeaseSettings;
pub use lease_state::LeaseState;
pub use lease_work_row::LeaseWorkRow;
pub use leased_surface::LeasedSurface;
pub use leased_work::LeasedWork;
pub use loss_reason::LossReason;
pub use released::Released;
pub use work_key::WorkKey;
