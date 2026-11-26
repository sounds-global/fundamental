//! An `#[action]`'s marker builds the action's own path from each capture's public form, so a
//! form's `action` is never formatted by hand.

use std::borrow::Cow;
use std::cell::RefCell;
use std::future::{Ready, ready};

use fundamental::AnyError;
use fundamental::address::SegmentText;
use fundamental::core::action::{ActionTransport, ClientAction};
use fundamental_macros::action;

#[derive(Clone)]
struct State;

#[action(POST, "/api/teams/{team}/aanvragen/{build}/send")]
async fn send_build(_ctx: fundamental::server::HttpContext<State>) -> Result<(), AnyError> {
    Ok(())
}

#[action(POST, "/api/sign-out")]
async fn sign_out(_ctx: fundamental::server::HttpContext<State>) -> Result<(), AnyError> {
    Ok(())
}

fn text(value: &str) -> SegmentText {
    SegmentText::new(value).expect("not empty")
}

#[test]
fn an_action_path_fills_each_capture_with_its_public_form() {
    assert_eq!(
        SendBuild::path(text("acme-books"), text("12")),
        "/api/teams/acme-books/aanvragen/12/send"
    );
    assert_eq!(SignOut::path(), "/api/sign-out");
}

#[test]
fn a_capture_is_escaped_so_it_cannot_restructure_the_path() {
    assert_eq!(
        SendBuild::path(text("a/b"), text("1?x")),
        "/api/teams/a%2Fb/aanvragen/1%3Fx/send"
    );
}

#[test]
fn an_action_path_is_typed_by_its_marker() {
    let path: fundamental::core::action::ActionPath<SendBuild> =
        SendBuild::path(text("acme-books"), text("12"));
    assert_eq!(path.as_str(), "/api/teams/acme-books/aanvragen/12/send");
    assert_eq!(path.to_string(), "/api/teams/acme-books/aanvragen/12/send");
    assert_eq!(String::from(SignOut::path()), "/api/sign-out");
}

thread_local! {
    static SENT: RefCell<Vec<(&'static str, String)>> = const { RefCell::new(Vec::new()) };
}

/// A transport that records what it was asked to send and answers `null`.
struct Recorder;

impl ActionTransport for Recorder {
    type Future<O>
        = Ready<Result<O, AnyError>>
    where
        O: 'static;

    fn call<I, O>(method: &'static str, path: Cow<'static, str>, _input: I) -> Self::Future<O>
    where
        I: serde::Serialize + 'static,
        O: serde::de::DeserializeOwned + 'static,
    {
        SENT.with(|sent| sent.borrow_mut().push((method, path.into_owned())));
        ready(Ok(
            serde_json::from_str("null").expect("the test answers unit")
        ))
    }
}

fn sent() -> Vec<(&'static str, String)> {
    SENT.with(|sent| sent.take())
}

#[test]
fn a_call_at_a_path_sends_the_filled_path() {
    SendBuild::call_at::<Recorder>(SendBuild::path(text("acme-books"), text("12")), ())
        .into_inner()
        .expect("recorded");
    assert_eq!(
        sent(),
        [("POST", "/api/teams/acme-books/aanvragen/12/send".to_owned())]
    );
}

#[test]
fn a_call_without_captures_sends_the_action_path() {
    <SignOut as ClientAction>::call::<Recorder>(())
        .into_inner()
        .expect("recorded");
    assert_eq!(sent(), [("POST", "/api/sign-out".to_owned())]);
}
