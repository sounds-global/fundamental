//! The refusal a migration set passes before any of it runs.
//!
//! [`MigrationGuard`] refuses keys that do not declare their sharding, keys that cannot, and
//! hash components the migration's own `CHECK` proves hot, then judges every migration lint.
//! A destructive statement passes only as its owner declares it ([`Retirements`]) or by an
//! [`Exemption::drop_of`]. The whole set is judged before its first statement runs.

mod exemption;
mod guard_error;
mod migration_guard;
mod retirements;

pub use exemption::Exemption;
pub use guard_error::GuardError;
pub use migration_guard::MigrationGuard;
pub use retirements::Retirements;
