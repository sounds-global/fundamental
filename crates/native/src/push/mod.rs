mod push_authorization;
mod push_delivery;
mod push_environment;
mod push_message;
mod push_provider;
mod push_registrar;
mod push_registration;
mod push_state;
mod push_unregistered;

pub use push_authorization::PushAuthorization;
pub use push_delivery::PushDelivery;
pub use push_environment::PushEnvironment;
pub use push_message::PushMessage;
pub use push_provider::PushProvider;
pub use push_registrar::PushRegistrar;
pub use push_registration::PushRegistration;
pub use push_unregistered::PushUnregistered;

pub(crate) use push_state::{PendingOutcome, PushDecision, PushInner, PushState};

use std::sync::Arc;

use serde_json::json;
use tracing::{debug, info, warn};

use crate::{NativeEvent, NativeHandle, PlatformHooks};

/// The webview event an inbound push is re-emitted as.
pub(crate) const WEBVIEW_EVENT: &str = "fundamental:push";

pub(crate) fn dispatch(
    handle: &NativeHandle,
    hooks: &Arc<[Box<dyn PlatformHooks>]>,
    event: &NativeEvent,
) {
    for hook in hooks.iter() {
        hook.on_event(handle, event);
    }
}

/// Delivers one inbound message to the hooks and to the webview.
///
/// Synchronous, because the app is suspended once a background push returns.
pub(crate) fn deliver_message(
    handle: &NativeHandle,
    hooks: &Arc<[Box<dyn PlatformHooks>]>,
    message: PushMessage,
) {
    let delivery = message.delivery().label();
    let payload = webview_payload(&message);
    dispatch(handle, hooks, &NativeEvent::PushReceived { message });
    emit_to_webview(handle, payload);
    info!(delivery, "push message delivered");
}

/// What the webview receives of a message: its delivery, chosen action and data.
fn webview_payload(message: &PushMessage) -> serde_json::Value {
    json!({
        "delivery": message.delivery().label(),
        "action": match message.delivery() {
            PushDelivery::Activated { action } => action.clone(),
            _ => None,
        },
        "data": message.data(),
    })
}

/// Hands `payload` to the page; a miss on a cold start is logged.
fn emit_to_webview(handle: &NativeHandle, payload: serde_json::Value) {
    if let Err(error) = handle.emit(WEBVIEW_EVENT, payload) {
        warn!(error = ?error, "the push message did not reach the webview");
    }
}

/// Delivers the notifications that arrived before the hooks existed.
pub(crate) fn release_pending(
    app: &tauri::App,
    handle: &NativeHandle,
    hooks: &Arc<[Box<dyn PlatformHooks>]>,
) {
    use tauri::Manager;

    let Some(state) = app.try_state::<PushState>() else {
        return;
    };
    let released = state.0.release_pending();
    if released.is_empty() {
        return;
    }
    let count = released.len();
    for message in released {
        deliver_message(handle, hooks, message);
    }
    info!(count, "cold-start push notifications delivered");
}

/// Acts on the cold-start queue's decision: deliver, hold, or report the drop.
pub(crate) fn settle(
    handle: &NativeHandle,
    hooks: &Arc<[Box<dyn PlatformHooks>]>,
    outcome: PendingOutcome,
) {
    match outcome {
        PendingOutcome::Deliver(message) => deliver_message(handle, hooks, message),
        PendingOutcome::Queued => held(),
        PendingOutcome::Dropped => dropped(),
    }
}

fn held() {
    debug!("push message held until the platform hooks are running");
}

fn dropped() {
    warn!("the cold-start push queue is full; the message was dropped");
}
