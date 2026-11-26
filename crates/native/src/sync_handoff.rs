use fundamental_core::secrets::SecretString;
use serde_json::{Map, Value};

/// The sync connection a native host hands its webview, as an initialization script.
///
/// [`into_init_script`](Self::into_init_script) renders a frozen `window.__FUNDAMENTAL_SYNC__`
/// for [`NativeBuilder::initialization_script`](crate::NativeBuilder::initialization_script),
/// which runs before any page script.
///
/// # Example
///
/// ```
/// # use fundamental_core::secrets::SecretString;
/// # use fundamental_native::SyncHandoff;
/// let script = SyncHandoff::new("ws://localhost:9100")
///     .with_token(SecretString::new("dev-token".into()))
///     .into_init_script();
/// assert!(script.starts_with("Object.freeze(window.__FUNDAMENTAL_SYNC__"));
/// ```
#[derive(Clone, Debug)]
#[must_use = "a SyncHandoff does nothing until rendered with into_init_script"]
pub struct SyncHandoff {
    url: String,
    token: Option<SecretString>,
}

impl SyncHandoff {
    /// A handoff for the sync server at `url` (`ws://…` or `wss://…`), with no token.
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            token: None,
        }
    }

    /// Sets the token the webview presents when it connects.
    pub fn with_token(mut self, token: SecretString) -> Self {
        self.token = Some(token);
        self
    }

    /// Renders the handoff as an initialization script that freezes
    /// `window.__FUNDAMENTAL_SYNC__` with `url` (and `token` when set).
    ///
    /// Values are JSON-encoded, so nothing escapes the string literal.
    #[must_use]
    pub fn into_init_script(self) -> String {
        let mut fields = Map::new();
        fields.insert("url".to_owned(), Value::String(self.url));
        if let Some(token) = self.token {
            fields.insert("token".to_owned(), Value::String(token.expose_owned()));
        }
        format!(
            "Object.freeze(window.__FUNDAMENTAL_SYNC__ = {});",
            Value::Object(fields)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::SyncHandoff;
    use fundamental_core::secrets::SecretString;

    #[test]
    fn renders_the_frozen_global_with_url_and_token() {
        let script = SyncHandoff::new("wss://sync.example.com")
            .with_token(SecretString::new("abc123".into()))
            .into_init_script();
        assert_eq!(
            script,
            "Object.freeze(window.__FUNDAMENTAL_SYNC__ = \
             {\"token\":\"abc123\",\"url\":\"wss://sync.example.com\"});"
        );
    }

    #[test]
    fn omits_the_token_field_when_no_token_is_set() {
        let script = SyncHandoff::new("ws://localhost:9100").into_init_script();
        assert_eq!(
            script,
            "Object.freeze(window.__FUNDAMENTAL_SYNC__ = {\"url\":\"ws://localhost:9100\"});"
        );
    }

    #[test]
    fn json_encoding_keeps_hostile_values_inside_the_string_literal() {
        // JSON escaping keeps quotes, backslashes and a closing-tag lookalike inert.
        let script = SyncHandoff::new("ws://x/\"quote\\path")
            .with_token(SecretString::new("</script>\"; alert(1); //".into()))
            .into_init_script();
        assert!(
            script.contains(r#""url":"ws://x/\"quote\\path""#),
            "{script}"
        );
        assert!(
            script.contains(r#""token":"</script>\"; alert(1); //""#),
            "{script}"
        );
        assert!(!script.contains(r#"alert(1); //"});"#), "{script}");
    }

    #[test]
    fn debug_never_renders_the_token() {
        let handoff =
            SyncHandoff::new("ws://x").with_token(SecretString::new("super-secret".into()));
        let rendered = format!("{handoff:?}");
        assert!(!rendered.contains("super-secret"), "{rendered}");
    }
}
