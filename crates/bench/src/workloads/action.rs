//! An action dispatched through the router, as a request reaches it.

use fundamental_api::{AnyError, Router};
use fundamental_macros::{action, mount_actions};
use fundamental_server::{Method, ServerRequest};
use serde::{Deserialize, Serialize};

use crate::workloads::ready::ready;

/// The app state the action runs with.
#[derive(Clone, Debug, Default)]
pub struct State;

/// What the action is given.
#[derive(Debug, Serialize, Deserialize)]
pub struct ProjectName {
    /// The project's id.
    pub id: u64,
    /// Its new name.
    pub name: String,
    /// Its tags.
    pub tags: Vec<String>,
}

/// What it answers.
#[derive(Debug, Serialize, Deserialize)]
pub struct Renamed {
    /// The project's id.
    pub id: u64,
    /// Its name as stored.
    pub name: String,
}

#[action(POST, "/api/projects/rename")]
async fn rename(_ctx: HttpContext<State>, input: ProjectName) -> Result<Renamed, AnyError> {
    Ok(Renamed {
        id: input.id,
        name: input.name.trim().to_owned(),
    })
}

#[action(POST, "/api/projects/archive")]
async fn archive(_ctx: HttpContext<State>, input: ProjectName) -> Result<Renamed, AnyError> {
    Ok(Renamed {
        id: input.id,
        name: input.name,
    })
}

mount_actions! { Rename, Archive }

/// A router with two `#[action]`s mounted, and the JSON body of a call to one.
pub struct ActionDispatch {
    router: Router<State>,
    body: Vec<u8>,
}

impl ActionDispatch {
    /// The router, mounted once as an app mounts it.
    ///
    /// # Panics
    ///
    /// When the two fixed actions do not mount: a programmer error in this workload.
    #[must_use]
    #[expect(
        clippy::expect_used,
        reason = "a fixed workload whose mount cannot fail"
    )]
    pub fn new() -> Self {
        let router = register_actions(Router::<State>::new()).expect("two distinct actions mount");
        let body = serde_json::to_vec(&ProjectName {
            id: 42,
            name: "  Sounds  ".to_owned(),
            tags: vec!["hardware".to_owned(), "audio".to_owned()],
        })
        .unwrap_or_default();
        Self { router, body }
    }

    /// Dispatches one call; the response status, or none if it waited.
    #[must_use]
    pub fn dispatch(&self) -> Option<u16> {
        let request = ServerRequest::new(Method::Post, "/api/projects/rename", State)
            .with_body(self.body.clone());
        ready(self.router.dispatch(request)).map(|response| response.status())
    }
}

impl std::fmt::Debug for ActionDispatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ActionDispatch")
            .field("routes", &self.router.len())
            .finish_non_exhaustive()
    }
}

impl Default for ActionDispatch {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_action_answers_ok() {
        assert_eq!(ActionDispatch::new().dispatch(), Some(200));
    }
}
