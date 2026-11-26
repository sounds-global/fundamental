//! An action's `path()` takes each capture's public form, never a record's raw id.
//!
//! Expected to fail to compile; the paired `.stderr` snapshot pins the error.

use fundamental::AnyError;
use fundamental_macros::action;

#[derive(Clone)]
struct State;

#[action(POST, "/api/teams/{team}/leave")]
async fn leave_team(_ctx: fundamental::server::HttpContext<State>) -> Result<(), AnyError> {
    Ok(())
}

/// A second `Uuid` pins how the error spells the raw one: rustc shortens a type's path only
/// while its name is unique in the crate graph, which the features a build unifies change.
#[allow(dead_code)]
struct Uuid;

fn main() {
    let _ = LeaveTeam::path(uuid::Uuid::nil());
}
