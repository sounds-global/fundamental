//! [`InboxContext`] — how the inbox's operator commands find the inbox in an app.

use crate::Inbox;

/// An app context that holds an [`Inbox`], so the `inbox:` commands can run against it.
pub trait InboxContext {
    /// The app's inbox.
    fn inbox(&self) -> &Inbox;
}
