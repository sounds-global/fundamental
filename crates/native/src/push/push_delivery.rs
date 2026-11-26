/// How a push message arrived, which decides what the app may do with it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PushDelivery {
    /// Delivered while the app was in the foreground.
    Foreground,
    /// Delivered to a backgrounded app, which is suspended once the hook returns.
    Background,
    /// The user opened the notification, launching or foregrounding the app.
    Activated {
        /// The chosen action's identifier, when the user picked one of its buttons.
        action: Option<String>,
    },
}

impl PushDelivery {
    /// The stable name carried in spans and in the webview event.
    pub(crate) fn label(&self) -> &'static str {
        match self {
            Self::Foreground => "foreground",
            Self::Background => "background",
            Self::Activated { .. } => "activated",
        }
    }
}
