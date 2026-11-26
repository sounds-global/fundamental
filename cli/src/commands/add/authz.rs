//! `fun add authz`: scaffold an authorization schema and wiring into an app.

use super::AddAuthzArgs;
use crate::helpers::{Project, write_file};
use crate::{CliResult, CommandContext};

/// Create `apps/<app>/src/authz.rs`: a sample schema, a `NativeEngine` builder and `HasAuthorizer`.
///
/// # Errors
/// A [`CliError`](crate::CliError) if the project cannot be found or the file cannot be written.
pub(crate) fn run(args: AddAuthzArgs, ctx: CommandContext) -> CliResult<()> {
    let project = Project::discover()?;
    let path = project.app_dir(&args.app).join("src").join("authz.rs");

    write_file(&path, AUTHZ_MODULE)?;

    let ui = ctx.ui();
    ui.blank();
    ui.section(format!("Created authorization module for {}", args.app));
    ui.detail(format!("apps/{}/src/authz.rs", args.app));
    ui.blank();
    ui.section("Next steps");
    ui.note("add `mod authz;` to the app's lib.rs / main.rs");
    ui.note("enable the `authz` umbrella feature plus a database backend (sql / cql / offline)");
    ui.note("add an `[authz]` section to fun.toml to tune the engine");
    ui.note("guard a route with `authorize(Document::Viewer)` (needs the `authz-server` feature)");
    ui.blank();
    Ok(())
}

const AUTHZ_MODULE: &str = r#"//! Authorization schema and wiring for this app.

use std::sync::Arc;

use fundamental::Namespace;
use fundamental::authz::authorizer::{DynAuthorizer, HasAuthorizer, NativeEngine};
use fundamental::authz::schema::SchemaIr;
use fundamental::authz::authz_schema;
use fundamental::authz::tuple::TupleStore;

/// A bare principal, only ever a subject.
#[derive(Namespace)]
pub enum User {}

/// A document: `owner` is a direct grant, `viewer` an owner or a direct grant.
#[derive(Namespace)]
pub enum Document {
    /// Direct owners of the document.
    #[relation(direct(User))]
    Owner,
    /// Owners, plus anyone directly granted view access.
    #[relation(union(Owner, direct(User)))]
    Viewer,
}

/// The compiled schema; build it once and share it.
///
/// # Panics
/// If a `#[relation(...)]` does not compile.
#[must_use]
pub fn schema() -> Arc<SchemaIr> {
    Arc::new(authz_schema![User, Document].expect("the authorization schema compiles"))
}

/// Builds the authorizer over `store`.
pub fn authorizer<S: TupleStore + 'static>(store: S) -> Arc<dyn DynAuthorizer> {
    Arc::new(NativeEngine::new(store, schema()))
}

/// Your application state; `authorize(...)` reaches the authorizer through it.
#[derive(Clone)]
pub struct AppState {
    authz: Arc<dyn DynAuthorizer>,
}

impl AppState {
    /// Builds the state over an authorizer.
    #[must_use]
    pub fn new(authz: Arc<dyn DynAuthorizer>) -> Self {
        Self { authz }
    }
}

impl HasAuthorizer for AppState {
    fn authorizer(&self) -> &dyn DynAuthorizer {
        self.authz.as_ref()
    }
}

// Ask and manage permissions with `AuthorizerExt`:
//
//     use fundamental::authz::authorizer::AuthorizerExt;
//     use fundamental::authz::schema::{NamespaceExt, RelationExt};
//
//     authz.grant(User::subject("alice"), Document::Owner, "readme").await?;
//     if authz.can(User::subject("alice"), Document::Viewer, "readme").await? { /* … */ }
//     let docs = authz.list(&principal, Document::Viewer).await?;
//
//     // A route guard, checked against the schema at compile time (`authz-server`):
//     use fundamental::authz::middleware::authorize;
//
//     #[action(GET, "/documents/{id}", auth, authorize(Document::Viewer))]
//     async fn read_document(ctx: HttpContext<AppState>, _: ()) -> Result<Document, AnyError> {
//     }
"#;
