//! An index must name the model's own columns.
//!
//! The handle the derive emits is the model's statement that an index exists and which column leads
//! it. A misspelt column would produce an `IndexKey` whose pinned predicate names a column the table
//! lacks — a runtime failure at best — so the derive refuses it where it was written.
//!
//! Expected to fail to compile; the paired `.stderr` snapshot pins the error.

use fundamental_macros::Model;

#[derive(Clone, Model)]
#[model(sql, index(name = "memberships_by_account", columns = ["staff_acount_id"]))]
struct Membership {
    #[model(id)]
    organisation_id: String,
    #[model(id)]
    staff_account_id: String,
}

fn main() {}
