use std::sync::{Arc, Mutex};

use crate::Redirect;

/// A per-request, shared sink for a [`Redirect`], honoured on the error arm as well as the success arm.
#[derive(Debug, Clone, Default)]
pub struct ResponseRedirect {
    // At most one redirect per request; the last write wins.
    redirect: Arc<Mutex<Option<Redirect>>>,
}

impl ResponseRedirect {
    /// Creates an empty sink — no redirect recorded, so the reply is unaffected.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Records `redirect` as this reply's destination; a poisoned lock is a silent no-op.
    pub fn set(&self, redirect: Redirect) {
        if let Ok(mut slot) = self.redirect.lock() {
            *slot = Some(redirect);
        }
    }

    /// Reads the recorded redirect without consuming it.
    #[must_use]
    pub fn get(&self) -> Option<Redirect> {
        self.redirect.lock().ok().and_then(|slot| slot.clone())
    }

    /// Takes the recorded redirect, leaving the sink empty.
    #[must_use]
    pub fn take(&self) -> Option<Redirect> {
        self.redirect.lock().ok().and_then(|mut slot| slot.take())
    }
}

#[cfg(test)]
mod tests {
    use super::ResponseRedirect;
    use crate::Redirect;

    #[test]
    fn an_unset_sink_takes_none() {
        assert_eq!(ResponseRedirect::new().take(), None);
    }

    #[test]
    fn a_recorded_redirect_is_taken_once() {
        let sink = ResponseRedirect::new();
        sink.set(Redirect::see_other("/login").expect("valid"));
        assert_eq!(
            sink.take().map(|redirect| redirect.location().to_owned()),
            Some("/login".to_owned())
        );
        assert_eq!(sink.take(), None, "take drains the sink");
    }

    #[test]
    fn the_last_write_wins() {
        let sink = ResponseRedirect::new();
        sink.set(Redirect::see_other("/first").expect("valid"));
        sink.set(Redirect::see_other("/second").expect("valid"));
        assert_eq!(
            sink.take().map(|redirect| redirect.location().to_owned()),
            Some("/second".to_owned())
        );
    }

    #[test]
    fn a_clone_shares_the_handle() {
        let handler_side = ResponseRedirect::new();
        let router_side = handler_side.clone();
        handler_side.set(Redirect::see_other("/login").expect("valid"));
        assert!(router_side.take().is_some());
    }

    #[test]
    fn reading_does_not_disturb_what_the_router_will_take() {
        let sink = ResponseRedirect::new();
        sink.set(Redirect::see_other("/login").expect("valid"));
        assert!(sink.get().is_some(), "an observer reads it");
        assert!(sink.take().is_some(), "and the router still gets it");
    }
}
