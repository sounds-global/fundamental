//! An explicit mutation role cannot be consumed as a query.

use fundamental_core::AnyError;
use fundamental_macros::{action, mount_actions};

#[action(GET, "/api/stats", reactive(mutation))]
async fn read(_ctx: fundamental_web::HttpContext<()>) -> Result<(), AnyError> {
    Ok(())
}

mount_actions! { Read }

fn require_query<S: fundamental_web::QuerySource>(_: S) {}

fn main() {
    require_query(actions::use_read());
}
