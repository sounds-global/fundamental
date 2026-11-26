//! [`HasAuthorizer`]: the seam that lets a permission guard reach the authorizer.

use super::DynAuthorizer;

/// Application state that carries an authorizer.
///
/// The `authorize(...)` middleware sees only the app's state, so it reaches the authorizer
/// through this.
#[diagnostic::on_unimplemented(
    message = "`{Self}` carries no authorizer, and a guarded route asks one",
    note = "an `authorize(..)` guard — on an action, or in a page's `policy.rs` — checks through the state's authorizer: implement `HasAuthorizer` for `{Self}`"
)]
pub trait HasAuthorizer {
    /// The authorizer this state carries.
    fn authorizer(&self) -> &dyn DynAuthorizer;
}
