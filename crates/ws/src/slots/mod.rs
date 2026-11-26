//! How a connection takes, keeps and gives back its slot.

mod admission;
mod admission_control;
mod anonymous_limits;
mod anonymous_registry;
mod displacement;
mod instant_after;
mod pool;
mod slot;
mod slot_cell;
mod slot_guard;

pub use admission::Admission;
pub(crate) use admission_control::AdmissionControl;
pub use anonymous_limits::AnonymousLimits;
pub(crate) use anonymous_registry::AnonymousRegistry;
pub(crate) use displacement::Displacement;
pub(crate) use instant_after::instant_after;
pub(crate) use pool::Pool;
pub(crate) use slot::Slot;
pub(crate) use slot_cell::SlotCell;
pub(crate) use slot_guard::SlotGuard;
