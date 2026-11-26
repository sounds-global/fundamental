use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use fundamental_core::address::WebOrigin;
use fundamental_core::listen::Listen;

use crate::{
    Admission, CredentialSource, InboundRequestId, InboundTraceContext, LocalePolicy,
    PreferencePolicy, SecurityHeaders,
};

/// Backend-neutral server configuration: where to bind and the request limits a backend enforces.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    listen: Listen,
    request_timeout: Duration,
    max_body_size: usize,
    cors_allowed_origins: Vec<String>,
    cors_allowed_headers: Vec<String>,
    cors_exposed_headers: Vec<String>,
    allow_dev_tools_cors: bool,
    credential_source: CredentialSource,
    trusted_write_origins: Vec<String>,
    /// Named origin sets, each answered across origins by the routes declaring it.
    cross_origins: Vec<(&'static str, Vec<String>)>,
    locale_policy: Option<LocalePolicy>,
    preferences: Vec<PreferencePolicy>,
    inbound_request_id: InboundRequestId,
    inbound_trace_context: InboundTraceContext,
    admission: Option<Arc<dyn Admission>>,
    security_headers: Option<SecurityHeaders>,
    served_origins: Vec<WebOrigin>,
    header_read_timeout: Duration,
    write_stall_timeout: Duration,
}

impl ServerConfig {
    /// Default request timeout when one is not set: 30 seconds.
    pub const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
    /// Default maximum request body size when one is not set: 10 MiB.
    pub const DEFAULT_MAX_BODY_SIZE: usize = 10 * 1024 * 1024;

    /// The default for [`with_header_read_timeout`](Self::with_header_read_timeout).
    pub const DEFAULT_HEADER_READ_TIMEOUT: Duration = Duration::from_secs(10);

    /// The default for [`with_write_stall_timeout`](Self::with_write_stall_timeout).
    pub const DEFAULT_WRITE_STALL_TIMEOUT: Duration = Duration::from_secs(30);

    /// The request headers a cross-origin caller may send by default: `Content-Type`, `Authorization`, `DPoP`.
    pub const DEFAULT_CORS_ALLOWED_HEADERS: [&'static str; 3] =
        ["content-type", "authorization", "dpop"];

    /// The response headers a cross-origin caller may read by default: `DPoP-Nonce`, `WWW-Authenticate`.
    pub const DEFAULT_CORS_EXPOSED_HEADERS: [&'static str; 2] = ["dpop-nonce", "www-authenticate"];

    /// Creates a config that binds `bind`, with default limits and no CORS origins.
    #[must_use]
    pub fn new(bind: SocketAddr) -> Self {
        Self::listening(Listen::Address(bind))
    }

    /// Creates a config that listens where `listen` says, with default limits and no CORS origins.
    #[must_use]
    pub fn listening(listen: Listen) -> Self {
        Self {
            listen,
            request_timeout: Self::DEFAULT_REQUEST_TIMEOUT,
            max_body_size: Self::DEFAULT_MAX_BODY_SIZE,
            cors_allowed_origins: Vec::new(),
            cors_allowed_headers: Self::DEFAULT_CORS_ALLOWED_HEADERS
                .iter()
                .map(|header| (*header).to_string())
                .collect(),
            cors_exposed_headers: Self::DEFAULT_CORS_EXPOSED_HEADERS
                .iter()
                .map(|header| (*header).to_string())
                .collect(),
            allow_dev_tools_cors: false,
            credential_source: CredentialSource::BearerHeader,
            trusted_write_origins: Vec::new(),
            cross_origins: Vec::new(),
            locale_policy: None,
            preferences: Vec::new(),
            inbound_request_id: InboundRequestId::default(),
            inbound_trace_context: InboundTraceContext::default(),
            admission: None,
            security_headers: None,
            served_origins: Vec::new(),
            header_read_timeout: Self::DEFAULT_HEADER_READ_TIMEOUT,
            write_stall_timeout: Self::DEFAULT_WRITE_STALL_TIMEOUT,
        }
    }

    /// Sets the per-request timeout a backend enforces.
    #[must_use]
    pub fn with_request_timeout(mut self, timeout: Duration) -> Self {
        self.request_timeout = timeout;
        self
    }

    /// Sets the maximum accepted request body size, in bytes.
    #[must_use]
    pub fn with_max_body_size(mut self, max_body_size: usize) -> Self {
        self.max_body_size = max_body_size;
        self
    }

    /// Sets the CORS origins a backend allows.
    #[must_use]
    pub fn with_cors_allowed_origins(mut self, origins: Vec<String>) -> Self {
        self.cors_allowed_origins = origins;
        self
    }

    /// Sets the request headers a cross-origin caller may send (`Access-Control-Allow-Headers`).
    #[must_use]
    pub fn with_cors_allowed_headers(mut self, headers: Vec<String>) -> Self {
        self.cors_allowed_headers = headers;
        self
    }

    /// Sets the response headers a cross-origin caller may read (`Access-Control-Expose-Headers`).
    #[must_use]
    pub fn with_cors_exposed_headers(mut self, headers: Vec<String>) -> Self {
        self.cors_exposed_headers = headers;
        self
    }

    /// Where the server listens.
    #[must_use]
    pub const fn listen(&self) -> &Listen {
        &self.listen
    }

    /// The per-request timeout.
    #[must_use]
    pub fn request_timeout(&self) -> Duration {
        self.request_timeout
    }

    /// The maximum accepted request body size, in bytes.
    #[must_use]
    pub fn max_body_size(&self) -> usize {
        self.max_body_size
    }

    /// The CORS origins a backend allows.
    #[must_use]
    pub fn cors_allowed_origins(&self) -> &[String] {
        &self.cors_allowed_origins
    }

    /// The request headers a cross-origin caller may send.
    #[must_use]
    pub fn cors_allowed_headers(&self) -> &[String] {
        &self.cors_allowed_headers
    }

    /// The response headers a cross-origin caller may read.
    #[must_use]
    pub fn cors_exposed_headers(&self) -> &[String] {
        &self.cors_exposed_headers
    }

    /// Allows any loopback origin to make cross-origin requests; development only, never in a release.
    #[must_use]
    pub fn with_allow_dev_tools_cors(mut self, allow: bool) -> Self {
        self.allow_dev_tools_cors = allow;
        self
    }

    /// Whether any loopback origin is allowed to make cross-origin requests.
    #[must_use]
    pub fn allow_dev_tools_cors(&self) -> bool {
        self.allow_dev_tools_cors
    }

    /// Sets where the backend reads the caller's credential from; a cookie source requires a verifier.
    #[must_use]
    pub fn with_credential_source(mut self, source: CredentialSource) -> Self {
        self.credential_source = source;
        self
    }

    /// Where the backend reads the caller's credential from.
    #[must_use]
    pub fn credential_source(&self) -> &CredentialSource {
        &self.credential_source
    }

    /// Sets the origins allowed to make cookie-authenticated writes across origins; empty means same-origin only.
    #[must_use]
    pub fn with_trusted_write_origins(mut self, origins: Vec<String>) -> Self {
        self.trusted_write_origins = origins;
        self
    }

    /// The origins allowed to make cookie-authenticated cross-origin writes.
    #[must_use]
    pub fn trusted_write_origins(&self) -> &[String] {
        &self.trusted_write_origins
    }

    /// Names a set of origins that the routes declaring it answer across origins; naming it again replaces it.
    #[must_use]
    pub fn with_cross_origin(mut self, set: &'static str, origins: Vec<WebOrigin>) -> Self {
        let origins = origins
            .iter()
            .map(|origin| origin.as_str().to_owned())
            .collect();
        self.cross_origins.retain(|(named, _)| *named != set);
        self.cross_origins.push((set, origins));
        self
    }

    /// The origins named `set`, or `None` when no set has that name.
    #[must_use]
    pub fn cross_origins(&self, set: &str) -> Option<&[String]> {
        self.cross_origins
            .iter()
            .find(|(named, _)| *named == set)
            .map(|(_, origins)| origins.as_slice())
    }

    /// Sets the languages this surface answers in, and how a request selects one; unset negotiates nothing.
    #[must_use]
    pub fn with_locale_policy(mut self, policy: LocalePolicy) -> Self {
        self.locale_policy = Some(policy);
        self
    }

    /// The languages this surface answers in, if it negotiates at all.
    #[must_use]
    pub fn locale_policy(&self) -> Option<&LocalePolicy> {
        self.locale_policy.as_ref()
    }

    /// Offers one more display preference on this surface.
    #[must_use]
    pub fn with_preference_policy(mut self, policy: PreferencePolicy) -> Self {
        self.preferences.push(policy);
        self
    }

    /// The display preferences this surface offers.
    #[must_use]
    pub fn preferences(&self) -> &[PreferencePolicy] {
        &self.preferences
    }

    /// Sets whether a caller's `X-Request-Id` may become the request's id; [`InboundRequestId::Ignore`] by default.
    #[must_use]
    pub fn with_inbound_request_id(mut self, policy: InboundRequestId) -> Self {
        self.inbound_request_id = policy;
        self
    }

    /// Whether a caller's `X-Request-Id` may become this request's id.
    #[must_use]
    pub fn inbound_request_id(&self) -> InboundRequestId {
        self.inbound_request_id
    }

    /// Sets what a caller's W3C `traceparent` may do; [`InboundTraceContext::Link`] by default.
    #[must_use]
    pub fn with_inbound_trace_context(mut self, policy: InboundTraceContext) -> Self {
        self.inbound_trace_context = policy;
        self
    }

    /// What a caller's `traceparent` may do to the request's span.
    #[must_use]
    pub fn inbound_trace_context(&self) -> InboundTraceContext {
        self.inbound_trace_context
    }

    /// Sets the [`Admission`] every request meets first; a second replaces the first.
    #[must_use]
    pub fn with_admission(mut self, admission: impl Admission) -> Self {
        self.admission = Some(Arc::new(admission));
        self
    }

    /// The admission every request meets first, if one is set.
    #[must_use]
    pub fn admission(&self) -> Option<&dyn Admission> {
        self.admission.as_deref()
    }

    /// Sets the security headers every response carries, replacing the API or web default.
    #[must_use]
    pub fn with_security_headers(mut self, headers: SecurityHeaders) -> Self {
        self.security_headers = Some(headers);
        self
    }

    /// The security headers configured for this server, if any were set.
    #[must_use]
    pub fn security_headers(&self) -> Option<&SecurityHeaders> {
        self.security_headers.as_ref()
    }

    /// Answers only requests addressed to one of `origins`, and every other with `421 Misdirected Request`.
    #[must_use]
    pub fn with_served_origins(mut self, origins: Vec<WebOrigin>) -> Self {
        self.served_origins = origins;
        self
    }

    /// Closes a connection whose request headers have not all arrived within `timeout`.
    #[must_use]
    pub fn with_header_read_timeout(mut self, timeout: Duration) -> Self {
        self.header_read_timeout = timeout;
        self
    }

    /// How long a connection may take to send a request's headers.
    #[must_use]
    pub fn header_read_timeout(&self) -> Duration {
        self.header_read_timeout
    }

    /// Closes a connection whose writes make no progress for `timeout`.
    #[must_use]
    pub fn with_write_stall_timeout(mut self, timeout: Duration) -> Self {
        self.write_stall_timeout = timeout;
        self
    }

    /// How long a connection's writes may make no progress.
    #[must_use]
    pub fn write_stall_timeout(&self) -> Duration {
        self.write_stall_timeout
    }

    /// Whether this server answers a request addressed to `authority`.
    #[must_use]
    pub fn serves(&self, authority: Option<&str>) -> bool {
        if self.served_origins.is_empty() {
            return true;
        }
        authority.is_some_and(|authority| {
            self.served_origins
                .iter()
                .any(|origin| origin.serves_authority(authority))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn served_origins_admit_only_their_authorities() {
        let config = ServerConfig::new(addr());
        assert!(
            config.serves(None),
            "no origins configured answers anything"
        );
        let config = config.with_served_origins(vec![
            WebOrigin::parse("https://files.example.nl").expect("an origin"),
        ]);
        assert!(config.serves(Some("files.example.nl")));
        assert!(!config.serves(Some("app.example.nl")));
        assert!(!config.serves(None));
    }

    fn addr() -> SocketAddr {
        "127.0.0.1:8080".parse().expect("valid addr")
    }

    #[test]
    fn defaults_are_applied() {
        let config = ServerConfig::new(addr());
        assert_eq!(config.listen(), &Listen::Address(addr()));
        assert_eq!(
            config.request_timeout(),
            ServerConfig::DEFAULT_REQUEST_TIMEOUT
        );
        assert_eq!(config.max_body_size(), ServerConfig::DEFAULT_MAX_BODY_SIZE);
        assert_eq!(
            config.inbound_request_id(),
            InboundRequestId::Ignore,
            "a caller's id is never recorded unless the deployment trusts its callers"
        );
        assert_eq!(
            config.inbound_trace_context(),
            InboundTraceContext::Link,
            "a caller's trace context is linked, never parented, by default"
        );
        assert!(
            config.admission().is_none(),
            "every request is admitted until a surface sets an admission"
        );
        assert!(config.cors_allowed_origins().is_empty());
        assert_eq!(
            config.cors_allowed_headers(),
            ["content-type", "authorization", "dpop"],
            "DPoP is a first-class cross-origin header by default"
        );
        assert_eq!(
            config.cors_exposed_headers(),
            ["dpop-nonce", "www-authenticate"],
            "the RFC 9449 challenge headers are readable cross-origin by default"
        );
        assert!(
            !config.allow_dev_tools_cors(),
            "dev-tools CORS is off by default (never in release)"
        );
        assert_eq!(
            config.credential_source(),
            &CredentialSource::BearerHeader,
            "the default credential source is exactly the pre-cookie behaviour"
        );
        assert!(
            config.trusted_write_origins().is_empty(),
            "a cookie-authenticated cross-origin write is trusted only when configured"
        );
        assert!(
            config.locale_policy().is_none(),
            "a surface negotiates no language until it says which ones it speaks"
        );
    }

    #[test]
    fn the_locale_policy_is_configurable() {
        let english = "en".parse().expect("a language tag");
        let config = ServerConfig::new(addr()).with_locale_policy(LocalePolicy::new(english));
        assert_eq!(
            config
                .locale_policy()
                .map(|policy| policy.default_locale().to_string()),
            Some("en".to_owned())
        );
    }

    #[test]
    fn the_credential_source_and_write_origins_are_configurable() {
        let config = ServerConfig::new(addr())
            .with_credential_source(CredentialSource::cookie("__Host-session"))
            .with_trusted_write_origins(vec!["https://partner.test".to_owned()]);
        assert_eq!(
            config.credential_source().cookie_name(),
            Some("__Host-session")
        );
        assert_eq!(config.trusted_write_origins(), ["https://partner.test"]);
    }

    #[test]
    fn a_cross_origin_set_is_named_and_widens_nothing_else() {
        let site = WebOrigin::parse("https://site.test").expect("an origin");
        let config = ServerConfig::new(addr()).with_cross_origin("site", vec![site]);

        assert_eq!(
            config.cross_origins("site"),
            Some(&["https://site.test".to_owned()][..])
        );
        assert_eq!(config.cross_origins("other"), None);
        assert!(config.cors_allowed_origins().is_empty());
        assert!(config.trusted_write_origins().is_empty());
    }

    #[test]
    fn builder_overrides_each_field() {
        let config = ServerConfig::new(addr())
            .with_request_timeout(Duration::from_secs(5))
            .with_max_body_size(1024)
            .with_cors_allowed_origins(vec!["https://example.com".to_owned()])
            .with_cors_allowed_headers(vec!["content-type".to_owned(), "dpop".to_owned()])
            .with_allow_dev_tools_cors(true);
        assert_eq!(config.request_timeout(), Duration::from_secs(5));
        assert_eq!(config.max_body_size(), 1024);
        assert_eq!(config.cors_allowed_origins(), ["https://example.com"]);
        assert_eq!(config.cors_allowed_headers(), ["content-type", "dpop"]);
        assert!(config.allow_dev_tools_cors());
    }

    #[test]
    fn the_inbound_policies_can_be_set() {
        let config = ServerConfig::new(addr())
            .with_inbound_request_id(InboundRequestId::Adopt)
            .with_inbound_trace_context(InboundTraceContext::Ignore);
        assert_eq!(config.inbound_request_id(), InboundRequestId::Adopt);
        assert_eq!(config.inbound_trace_context(), InboundTraceContext::Ignore);
    }

    #[derive(Debug)]
    struct Refusing(&'static str);

    impl Admission for Refusing {
        fn admit(
            &self,
            _request: &crate::AdmissionRequest<'_>,
        ) -> Result<(), fundamental_core::AnyError> {
            Err(crate::ServerError::Internal(self.0.to_owned()).into())
        }
    }

    #[test]
    fn a_second_admission_replaces_the_first() {
        let config = ServerConfig::new(addr())
            .with_admission(Refusing("first"))
            .with_admission(Refusing("second"));
        let rendered = format!("{:?}", config.admission());
        assert!(
            rendered.contains("second") && !rendered.contains("first"),
            "{rendered}"
        );
    }
}
