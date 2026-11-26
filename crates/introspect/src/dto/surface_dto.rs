use serde::{Deserialize, Serialize};

use crate::dto::action_dto::ActionDto;
use crate::dto::doc_page_dto::DocPageDto;
use crate::dto::message_dto::MessageDto;

/// One application's HTTP and messaging surface, served as `surface.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SurfaceDto {
    /// The app's name.
    pub app: String,
    /// The app's API version (shown in the status bar).
    pub version: String,
    /// The authored narrative pages.
    #[serde(default)]
    pub pages: Vec<DocPageDto>,
    /// The HTTP actions.
    #[serde(default)]
    pub actions: Vec<ActionDto>,
    /// The messages.
    #[serde(default)]
    pub messages: Vec<MessageDto>,
}

#[cfg(feature = "ssr")]
impl SurfaceDto {
    /// Builds the surface from the server-side descriptors and authored pages.
    #[must_use]
    pub fn build(
        app: &str,
        version: &str,
        pages: &[crate::DocPage],
        actions: &[fundamental_server::ActionDescriptor],
        messages: &[fundamental_nats::MessageDescriptor],
    ) -> Self {
        Self {
            app: app.to_owned(),
            version: version.to_owned(),
            pages: pages.iter().map(DocPageDto::from).collect(),
            actions: actions.iter().map(ActionDto::from).collect(),
            messages: messages.iter().map(MessageDto::from).collect(),
        }
    }
}

#[cfg(all(test, feature = "ssr"))]
mod tests {
    use super::*;
    use crate::DocPage;
    use fundamental_nats::{MessageDescriptor, MessageTransport};
    use fundamental_schema::SchemaNode;
    use fundamental_server::ActionDescriptor;

    #[test]
    fn builds_from_descriptors_and_round_trips() {
        let surface = SurfaceDto::build(
            "demo",
            "1.0.0",
            &[DocPage::new("Overview", "<p>Hi</p>")],
            &[ActionDescriptor::new(
                "POST",
                "/users",
                "Create a user.",
                &["auth"],
                None,
                SchemaNode::Any,
                SchemaNode::Any,
            )],
            &[MessageDescriptor::new(
                "events.user.created",
                MessageTransport::Nats,
                "",
                SchemaNode::Any,
            )],
        );
        assert_eq!(surface.app, "demo");
        assert_eq!(surface.version, "1.0.0");
        assert_eq!(surface.actions.len(), 1);
        assert_eq!(surface.actions[0].path, "/users");
        assert_eq!(surface.messages[0].subject, "events.user.created");
        assert_eq!(surface.pages[0].title, "Overview");

        let json = serde_json::to_string(&surface).expect("serialize");
        assert_eq!(serde_json::from_str::<SurfaceDto>(&json).unwrap(), surface);
    }
}
