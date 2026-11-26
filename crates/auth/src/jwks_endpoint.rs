//! [`JwksEndpoint`] — where a key set is fetched from, and the bounds on fetching it.

use std::net::IpAddr;
use std::time::Duration;

use reqwest::redirect::Policy;
use reqwest::{Client, Url};

use crate::AuthError;
use crate::claim_pins::ClaimPins;
use crate::jwks_snapshot::JwksSnapshot;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// How long one fetch may take end to end.
pub(crate) const FETCH_TIMEOUT: Duration = Duration::from_secs(10);

/// The largest key set accepted.
const MAX_BODY_BYTES: usize = 256 * 1024;

/// A configured JWKS URL and the client that fetches it: `https` unless loopback, no redirects.
pub(crate) struct JwksEndpoint {
    url: Url,
    client: Client,
}

impl JwksEndpoint {
    /// Checks `url` and builds the client that fetches it.
    ///
    /// # Errors
    /// [`AuthError::Config`] if `url` does not parse, carries credentials, is neither `https`
    /// nor `http` to a loopback host, or the client cannot be built.
    pub(crate) fn new(url: &str) -> Result<Self, AuthError> {
        Self::with_timeouts(url, CONNECT_TIMEOUT, FETCH_TIMEOUT)
    }

    pub(crate) fn with_timeouts(
        url: &str,
        connect: Duration,
        total: Duration,
    ) -> Result<Self, AuthError> {
        // Not echoed: until it parses, nothing says where a credential in it would sit.
        let url = Url::parse(url)
            .map_err(|e| AuthError::Config(format!("token_jwks_url is not a URL: {e}")))?;
        if !url.username().is_empty() || url.password().is_some() {
            return Err(AuthError::Config(
                "token_jwks_url must not carry credentials: the URL is logged on every fetch"
                    .to_owned(),
            ));
        }
        let https = match url.scheme() {
            "https" => true,
            "http" if is_loopback(&url) => false,
            _ => {
                return Err(AuthError::Config(format!(
                    "token_jwks_url `{url}` must be https unless its host is a loopback address"
                )));
            }
        };
        let client = Client::builder()
            .user_agent("fundamental-auth")
            .connect_timeout(connect)
            .timeout(total)
            .redirect(Policy::none())
            .https_only(https)
            .build()
            .map_err(|e| AuthError::Config(format!("building JWKS HTTP client: {e}")))?;
        Ok(Self { url, client })
    }

    pub(crate) fn url(&self) -> &Url {
        &self.url
    }

    ///
    /// # Errors
    /// [`AuthError::Jwks`] if the endpoint cannot be reached, redirects, answers other than
    /// `2xx`, sends more than 256 KiB, or sends something that is not a key set.
    pub(crate) async fn fetch(&self, pins: &ClaimPins) -> Result<JwksSnapshot, AuthError> {
        let url = &self.url;
        let mut response = self
            .client
            .get(url.clone())
            .send()
            .await
            .map_err(|e| AuthError::Jwks(format!("fetching JWKS from `{url}`: {e}")))?;
        let status = response.status();
        if status.is_redirection() {
            return Err(AuthError::Jwks(format!(
                "JWKS endpoint `{url}` redirected (HTTP {status}); a key set is fetched from the configured URL only"
            )));
        }
        if !status.is_success() {
            return Err(AuthError::Jwks(format!(
                "JWKS endpoint `{url}` returned HTTP {status}"
            )));
        }
        let too_large = || {
            AuthError::Jwks(format!(
                "JWKS from `{url}` is larger than {MAX_BODY_BYTES} bytes"
            ))
        };
        if response
            .content_length()
            .is_some_and(|length| length > MAX_BODY_BYTES as u64)
        {
            return Err(too_large());
        }
        let mut body = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|e| AuthError::Jwks(format!("reading JWKS from `{url}`: {e}")))?
        {
            if body.len() + chunk.len() > MAX_BODY_BYTES {
                return Err(too_large());
            }
            body.extend_from_slice(&chunk);
        }
        let body = String::from_utf8(body)
            .map_err(|_| AuthError::Jwks(format!("JWKS from `{url}` is not UTF-8")))?;
        JwksSnapshot::parse(&body, pins, url.as_str())
    }
}

fn is_loopback(url: &Url) -> bool {
    match url.host_str() {
        Some(host) if host.eq_ignore_ascii_case("localhost") => true,
        Some(host) => host
            .trim_start_matches('[')
            .trim_end_matches(']')
            .parse::<IpAddr>()
            .is_ok_and(|ip| ip.is_loopback()),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::*;

    #[test]
    fn a_plain_http_jwks_url_is_refused() {
        for url in [
            "http://iam:8001/.well-known/jwks.json",
            "http://iam.example.com/jwks",
            "http://10.0.0.7/jwks",
            "ftp://iam.example.com/jwks",
        ] {
            assert!(
                matches!(JwksEndpoint::new(url), Err(AuthError::Config(_))),
                "{url} must be refused"
            );
        }
    }

    #[test]
    fn a_loopback_http_jwks_url_is_accepted() {
        for url in [
            "http://127.0.0.1:8001/jwks",
            "http://localhost/jwks",
            "http://[::1]:9000/jwks",
            "https://iam.example.com/.well-known/jwks.json",
        ] {
            assert!(JwksEndpoint::new(url).is_ok(), "{url} must be accepted");
        }
    }

    #[test]
    fn a_url_carrying_credentials_is_refused() {
        for url in [
            "https://svc:hunter2@iam.example.com/jwks",
            "https://token@iam.example.com/jwks",
        ] {
            let refused = JwksEndpoint::new(url).err().expect("refused");
            assert!(matches!(refused, AuthError::Config(_)), "{url}");
            assert!(!refused.to_string().contains("hunter2"), "{refused}");
        }
    }

    #[tokio::test]
    async fn a_slow_jwks_endpoint_is_cut_off() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("addr");
        let held = tokio::spawn(async move { listener.accept().await });
        let endpoint = JwksEndpoint::with_timeouts(
            &format!("http://{addr}/jwks"),
            Duration::from_millis(200),
            Duration::from_millis(300),
        )
        .expect("a loopback URL");
        let started = Instant::now();
        let refused = endpoint.fetch(&ClaimPins::default()).await;
        assert!(matches!(refused, Err(AuthError::Jwks(_))));
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "the fetch was cut off at its bound, not left to hang"
        );
        held.abort();
    }

    #[test]
    fn a_url_that_does_not_parse_is_a_configuration_error() {
        assert!(matches!(
            JwksEndpoint::new("not a url"),
            Err(AuthError::Config(_))
        ));
    }
}
