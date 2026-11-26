use fundamental_core::AnyError;
use fundamental_macros::Error;

/// Errors raised by the native surface's bridges.
///
/// Transport-level failures only; an application failure is an ordinary
/// [`BridgeResponse`](crate::BridgeResponse).
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum NativeError {
    /// A wrapped error that does not map onto a more specific variant.
    Other(AnyError),

    /// The principal source failed before in-process dispatch.
    #[error(
        code = "NATIVE_001",
        message = "Principal resolution failed",
        status = 401
    )]
    Principal {
        /// The principal source's underlying failure.
        #[source]
        source: AnyError,
    },

    /// The remote API could not be reached or the exchange failed mid-flight.
    ///
    /// Carries the configured origin, never the request URL, which is the webview's.
    #[error(
        code = "NATIVE_002",
        message = "Remote API call to {origin} failed",
        status = 503,
        retryable
    )]
    RemoteTransport {
        /// The configured API origin, `scheme://host[:port]`.
        origin: String,
        /// The transport-level cause.
        #[source]
        source: reqwest::Error,
    },

    /// The remote bridge's base URL is not a valid absolute URL.
    #[error(
        code = "NATIVE_003",
        message = "Invalid remote API base URL: {url}",
        status = 500
    )]
    InvalidUrl {
        /// The rejected URL.
        url: String,
    },

    /// The token source failed before a remote call.
    #[error(
        code = "NATIVE_004",
        message = "Bearer token resolution failed",
        status = 401
    )]
    Token {
        /// The token source's underlying failure.
        #[source]
        source: AnyError,
    },

    /// The Tauri app could not be built or exited its run loop with an error.
    #[error(
        code = "NATIVE_005",
        message = "Native app failed to run",
        status = 500
    )]
    Run {
        /// The windowing runtime's underlying failure.
        #[source]
        source: tauri::Error,
    },

    /// A window could not be created or a window operation failed.
    #[error(
        code = "NATIVE_006",
        message = "Window operation on {label} failed",
        status = 500
    )]
    Window {
        /// The window's label.
        label: String,
        /// The windowing runtime's underlying failure.
        #[source]
        source: tauri::Error,
    },

    /// A window operation named a label no open window carries.
    #[error(
        code = "NATIVE_007",
        message = "No window labeled {label}",
        status = 404
    )]
    WindowNotFound {
        /// The unknown label.
        label: String,
    },

    /// `run` was called without a router or bridge.
    #[error(
        code = "NATIVE_008",
        message = "NativeBuilder needs a router or bridge before run; call .router(router, state) or .bridge(...)",
        status = 500
    )]
    MissingBridge,

    /// An event could not be emitted to the webview.
    #[error(code = "NATIVE_009", message = "Emit of {event} failed", status = 500)]
    Emit {
        /// The event name.
        event: String,
        /// The windowing runtime's underlying failure.
        #[source]
        source: tauri::Error,
    },

    /// The system tray could not be built.
    #[error(
        code = "NATIVE_011",
        message = "System tray setup failed",
        status = 500
    )]
    Tray {
        /// The windowing runtime's underlying failure.
        #[source]
        source: tauri::Error,
    },

    /// The tray was configured but the app bundle carries no window icon.
    #[error(
        code = "NATIVE_012",
        message = "System tray needs a bundled window icon; add one under icons/ in tauri.conf.json",
        status = 500
    )]
    MissingTrayIcon,

    /// The application menu could not be built.
    #[error(
        code = "NATIVE_013",
        message = "Application menu setup failed",
        status = 500
    )]
    Menu {
        /// The windowing runtime's underlying failure.
        #[source]
        source: tauri::Error,
    },

    /// A platform hook's setup failed, aborting startup.
    #[error(
        code = "NATIVE_010",
        message = "Platform hook {hook} failed during setup",
        status = 500
    )]
    Setup {
        /// The failing hook's name.
        hook: String,
        /// The hook's underlying failure.
        #[source]
        source: AnyError,
    },

    /// The sync host's runtime or worker thread could not start.
    #[cfg(feature = "sync-host")]
    #[error(
        code = "NATIVE_014",
        message = "Sync host failed to start",
        status = 500
    )]
    SyncHostStart {
        /// The runtime/thread build failure.
        #[source]
        source: std::io::Error,
    },

    /// The sync host is gone (its thread exited or its channel closed).
    #[cfg(feature = "sync-host")]
    #[error(
        code = "NATIVE_015",
        message = "Sync host is unavailable",
        status = 503,
        retryable
    )]
    SyncHostUnavailable,

    /// A live request named a slice the host's registry does not carry.
    #[cfg(feature = "sync-host")]
    #[error(
        code = "NATIVE_016",
        message = "No device slice named {slice}; list it in the host's device slices",
        status = 404
    )]
    UnknownSlice {
        /// The unknown slice name.
        slice: String,
    },

    /// The hosted offline database could not be opened or read.
    #[cfg(feature = "sync-host")]
    #[error(
        code = "NATIVE_017",
        message = "Hosted offline database operation failed",
        status = 500
    )]
    SyncHostDatabase {
        /// The database failure.
        #[source]
        source: fundamental_offline::db::OfflineDbError,
    },

    /// A live bind carried parameters the slice rejects.
    #[cfg(feature = "sync-host")]
    #[error(
        code = "NATIVE_018",
        message = "Invalid parameters for slice {slice}",
        status = 422
    )]
    SliceParams {
        /// The slice whose parameters failed.
        slice: String,
        /// The validation/serialization failure.
        #[source]
        source: fundamental_sync::SliceBindError,
    },

    /// A live request's body was not the expected `{slice, params}` JSON.
    #[cfg(feature = "sync-host")]
    #[error(
        code = "NATIVE_019",
        message = "Malformed live request: {detail}",
        status = 400
    )]
    MalformedLiveRequest {
        /// What the JSON parser rejected.
        detail: String,
    },

    /// A live read produced rows that could not be encoded for the webview.
    #[cfg(feature = "sync-host")]
    #[error(
        code = "NATIVE_020",
        message = "Live rows for {slice} could not be encoded: {detail}",
        status = 500
    )]
    LiveEncoding {
        /// The slice whose rows failed to encode.
        slice: String,
        /// The encoder's failure.
        detail: String,
    },

    /// Opening a URL through the OS failed (no handler, or the open was refused).
    #[cfg(feature = "deep-link")]
    #[error(
        code = "NATIVE_021",
        message = "Opening a URL through the OS failed: {detail}",
        status = 500
    )]
    DeepLinkOpen {
        /// The opener's failure detail, never the URL, which may carry a secret.
        detail: String,
    },

    /// Push platform code ran in an app that never activated the push seam.
    #[cfg(feature = "push")]
    #[error(
        code = "NATIVE_022",
        message = "Push is not active; call NativeBuilder::push_notifications(..) before run",
        status = 500
    )]
    PushInactive,

    /// A push notification's payload was not a JSON object, or did not match the
    /// type the app asked for.
    #[cfg(feature = "push")]
    #[error(
        code = "NATIVE_023",
        message = "The push payload could not be read: {detail}",
        status = 422
    )]
    PushPayload {
        /// Which kind of failure, and where; never the payload's contents.
        detail: String,
    },

    /// No loopback port could be bound for an RFC 8252 redirect.
    ///
    /// Fatal, so the browser is never sent to a port nothing listens on.
    #[error(
        code = "NATIVE_024",
        message = "No loopback port could be bound for the redirect",
        status = 500
    )]
    LoopbackBind {
        /// The operating system's refusal.
        #[source]
        source: std::io::Error,
    },

    /// The redirect did not arrive before the callback's deadline.
    ///
    /// Usually a person who closed the tab: read it as unfinished.
    #[error(
        code = "NATIVE_025",
        message = "The redirect did not arrive in time",
        status = 408,
        retryable
    )]
    LoopbackExpired,

    /// The callback answered its whole connection budget without the redirect.
    ///
    /// Distinct from [`LoopbackExpired`](Self::LoopbackExpired): something local is answering.
    #[error(
        code = "NATIVE_026",
        message = "The redirect never arrived, and something local answered instead",
        status = 409
    )]
    LoopbackFlooded,

    /// A certificate chain could not be installed on a
    /// [`ClientIdentity`](crate::ClientIdentity): it did not parse, was empty, or its leaf names
    /// another key.
    #[cfg(feature = "client-identity")]
    #[error(
        code = "NATIVE_027",
        message = "The certificate cannot be used with this device's key",
        developer_message = "client certificate refused: {detail}",
        status = 400
    )]
    ClientCertificate {
        /// Why it was refused; names no key material.
        detail: String,
    },

    /// The client's TLS configuration could not be built: the operating system's trust
    /// store held no usable root, or the TLS provider refused the protocol versions.
    #[cfg(feature = "client-identity")]
    #[error(
        code = "NATIVE_028",
        message = "The secure connection could not be configured",
        developer_message = "client TLS configuration failed: {detail}",
        status = 500
    )]
    ClientTls {
        /// What failed.
        detail: String,
    },
}

#[cfg(test)]
mod tests {
    use super::NativeError;
    use fundamental_core::{AnyError, Error as _};

    #[derive(Debug, fundamental_macros::Error)]
    enum StubError {
        #[error(code = "STUB_001", message = "keychain unavailable", status = 500)]
        Keychain,
    }

    #[test]
    fn principal_carries_code_status_and_source_chain() {
        use std::error::Error as _;
        let err = NativeError::Principal {
            source: AnyError::new(StubError::Keychain),
        };
        assert_eq!(err.error_code(), "NATIVE_001");
        assert_eq!(err.http_status_code(), 401);
        let source = err.source().expect("Principal must expose its source");
        assert!(source.to_string().contains("keychain unavailable"));
    }

    #[test]
    fn invalid_url_interpolates_the_offending_value() {
        let err = NativeError::InvalidUrl {
            url: "not a url".into(),
        };
        assert_eq!(err.error_code(), "NATIVE_003");
        assert_eq!(err.http_status_code(), 500);
        assert!(format!("{err}").contains("not a url"));
    }

    #[test]
    fn remote_transport_is_retryable() {
        // A guaranteed-invalid scheme yields a `reqwest::Error` without a network.
        let source = reqwest::Client::new()
            .get("http://")
            .build()
            .expect_err("invalid URL");
        let err = NativeError::RemoteTransport {
            origin: "http://api.example".into(),
            source,
        };
        assert_eq!(err.error_code(), "NATIVE_002");
        assert_eq!(err.http_status_code(), 503);
        assert!(err.is_retryable());
    }
}
