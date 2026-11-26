use leptos::prelude::provide_context;

use super::live_context::{LiveContext, LiveMode};

/// Provides the app-wide [`LiveContext`](super::LiveContext) over a database the native host owns.
pub fn provide_live_hosted() {
    provide_context(LiveContext {
        mode: LiveMode::Hosted,
    });
}
