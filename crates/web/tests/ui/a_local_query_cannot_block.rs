//! A browser-only query cannot hold the server's answer: `.local()` offers no `.blocking()`.

use fundamental_core::AnyError;
use fundamental_macros::{action, mount_actions};

#[action(GET, "/api/stats", reactive(query))]
async fn read(_ctx: fundamental_web::HttpContext<()>) -> Result<String, AnyError> {
    Ok(String::new())
}

mount_actions! { Read }

fn main() {
    let _ = actions::use_read().local().blocking();
}
