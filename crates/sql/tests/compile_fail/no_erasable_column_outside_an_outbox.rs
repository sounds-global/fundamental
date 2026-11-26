//! `#[model(erasable)]` belongs to an outbox.
//!
//! `erasable` leaves a column an erasure clears out of an outbox row's seal. On a model that
//! carries no seal it would promise an exemption from nothing, so it is refused rather than
//! ignored.
//!
//! Expected to fail to compile; the paired `.stderr` snapshot pins the error.

use fundamental_macros::Model;

#[derive(Model)]
#[model(sql)]
struct AuditEntry {
    #[model(id)]
    id: uuid::Uuid,
    #[model(erasable)]
    subject: Option<String>,
}

fn main() {}
