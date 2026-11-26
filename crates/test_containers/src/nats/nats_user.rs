use fundamental_core::secrets::SecretString;

/// A user the authenticating NATS server knows, with what it may do there.
///
/// A provisioner, an application under its own subjects, and a stream reader. The passwords
/// are fixed: they guard a disposable loopback server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NatsUser {
    /// May do anything in its account, JetStream included.
    Provisioner,
    /// May publish and subscribe under `<run>.app.>` and its reply inboxes, and nothing else.
    App,
    /// May read a stream's information, and nothing about its consumers.
    Reader,
}

impl NatsUser {
    /// The user's name on the server.
    #[must_use]
    pub const fn username(self) -> &'static str {
        match self {
            Self::Provisioner => "provisioner",
            Self::App => "app",
            Self::Reader => "reader",
        }
    }

    /// The user's password on the server.
    #[must_use]
    pub fn password(self) -> SecretString {
        SecretString::new(self.password_text().to_owned())
    }

    pub(crate) const fn password_text(self) -> &'static str {
        match self {
            Self::Provisioner => "provisioner-test-password",
            Self::App => "app-test-password",
            Self::Reader => "reader-test-password",
        }
    }
}
