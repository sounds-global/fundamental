use super::CredentialSource;
use crate::settings::TlsPosture;

/// One configured database instance, whichever engine and slot it came from.
#[derive(Debug, Clone, Copy)]
pub struct ConnectionRef<'a> {
    /// The engine that will open it — `"sql"` or `"cql"`.
    pub engine: &'static str,
    /// The domain name routing to it, or `None` for the engine's default pool.
    pub name: Option<&'a str>,
    /// Where this instance's credentials come from.
    pub credentials: &'a CredentialSource,
    /// How this instance's transport security is arranged.
    pub tls: TlsPosture,
    /// How many connections one process opens: `max_connections`, or `connections_per_host × hosts`.
    pub max_connections: u32,
}

impl ConnectionRef<'_> {
    /// How to name this instance in an operator-facing message: `sql` or `sql.layer0`.
    #[must_use]
    pub fn label(&self) -> String {
        match self.name {
            Some(name) => format!("{}.{name}", self.engine),
            None => self.engine.to_string(),
        }
    }
}
