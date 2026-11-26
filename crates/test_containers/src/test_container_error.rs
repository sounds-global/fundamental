use fundamental_macros::Error;

/// What can go wrong bringing a test container up.
///
/// [`Start`](Self::Start) is usually Docker not running; [`NotReady`](Self::NotReady) means the
/// engine inside never answered. Returned, never panicked, so the test decides.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum TestContainerError {
    /// Docker refused to start, or could not find, the container.
    #[error(
        code = "TESTCONTAINER_001",
        message = "could not start the {engine} container '{name}': {detail}",
        status = 500
    )]
    Start {
        /// The engine the recipe describes, e.g. `yugabyte`.
        engine: &'static str,
        /// The container name that failed.
        name: String,
        /// The client's own account of the failure.
        detail: String,
    },

    /// The container started but never published the port the harness dials.
    #[error(
        code = "TESTCONTAINER_002",
        message = "the {engine} container '{name}' published no host port for {port}: {detail}",
        status = 500
    )]
    NoHostPort {
        /// The engine the recipe describes.
        engine: &'static str,
        /// The container name.
        name: String,
        /// The container-side port that should have been published.
        port: u16,
        /// The client's own account of the failure.
        detail: String,
    },

    /// The container is up and the engine inside it never accepted a connection.
    #[error(
        code = "TESTCONTAINER_003",
        message = "{engine} at {endpoint} did not answer within {waited_secs}s: {detail}",
        status = 503
    )]
    NotReady {
        /// The engine the recipe describes.
        engine: &'static str,
        /// Where the harness was dialling.
        endpoint: String,
        /// How long it waited.
        waited_secs: u64,
        /// The last failure the poll saw, which is the diagnostic.
        detail: String,
    },

    /// A fixture could not be provisioned on an otherwise healthy engine.
    #[error(
        code = "TESTCONTAINER_004",
        message = "could not provision {fixture} on {engine}: {detail}",
        status = 500
    )]
    Provision {
        /// The engine the fixture was provisioned on.
        engine: &'static str,
        /// What was being created — a database, a keyspace, a stream.
        fixture: String,
        /// The engine's own account of the failure.
        detail: String,
    },
}
