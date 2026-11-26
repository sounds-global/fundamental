use std::sync::Arc;

use axum::http::{HeaderName, HeaderValue, Response};
use fundamental_server::{HeaderCombine, INTERNAL_CSP_NONCE_HEADER, SecurityHeaders, ServerError};
use tower::util::MapResponseLayer;
use tracing::{error, info};

/// A server's [`SecurityHeaders`] as wire headers, applied to every response; the outermost layer.
#[derive(Clone)]
pub(crate) struct SecurityHeaderLayer {
    fields: Arc<[(HeaderName, HeaderValue, HeaderCombine)]>,
}

impl SecurityHeaderLayer {
    /// Converts `policy` into wire headers, refusing one the wire cannot carry.
    ///
    /// # Errors
    /// [`ServerError::SecurityHeaderInvalid`] (`API_018`) when a name or value is not a
    /// valid HTTP field.
    pub(crate) fn new(policy: &SecurityHeaders) -> Result<Self, ServerError> {
        let headers = policy.headers();
        Self::from_fields(
            headers
                .iter()
                .map(|header| (header.name(), header.value(), header.combine())),
        )
    }

    /// The layer over already-rendered `(name, value, combine)` fields.
    fn from_fields<'a>(
        headers: impl Iterator<Item = (&'static str, &'a str, HeaderCombine)>,
    ) -> Result<Self, ServerError> {
        let mut fields = Vec::new();
        for (header, value, combine) in headers {
            let name = HeaderName::from_static(header);
            let value = HeaderValue::from_str(value).map_err(|_| {
                error!(
                    error_code = "API_018",
                    header, "refusing to serve: a security header cannot be sent"
                );
                ServerError::SecurityHeaderInvalid {
                    header,
                    reason: "its value is not a valid HTTP field value",
                }
            })?;
            info!(
                header,
                value = ?value,
                combine = ?combine,
                "every response carries this security header"
            );
            fields.push((name, value, combine));
        }
        Ok(Self {
            fields: fields.into(),
        })
    }

    #[expect(clippy::excessive_nesting, reason = "readability baseline")]
    pub(crate) fn layer<B>(
        &self,
    ) -> MapResponseLayer<impl Fn(Response<B>) -> Response<B> + Clone + use<B>> {
        let fields = Arc::clone(&self.fields);
        MapResponseLayer::new(move |mut response: Response<B>| {
            let headers = response.headers_mut();
            let nonce = headers
                .remove(INTERNAL_CSP_NONCE_HEADER)
                .and_then(|value| value.to_str().ok().map(str::to_owned));
            for (name, value, combine) in fields.iter() {
                let value = if name.as_str() == "content-security-policy" {
                    nonce
                        .as_deref()
                        .map_or_else(|| value.clone(), |nonce| csp_with_nonce(value, nonce))
                } else {
                    value.clone()
                };
                match combine {
                    HeaderCombine::Replace => {
                        headers.insert(name.clone(), value);
                    }
                    HeaderCombine::Alongside => {
                        headers.append(name.clone(), value);
                    }
                }
            }
            response
        })
    }
}

/// Adds a per-response script nonce to the existing `script-src` directive.
fn csp_with_nonce(value: &HeaderValue, nonce: &str) -> HeaderValue {
    let Ok(value) = value.to_str() else {
        return value.clone();
    };
    let Some(start) = value.find("script-src ") else {
        return HeaderValue::from_str(value)
            .unwrap_or_else(|_| HeaderValue::from_static("default-src 'none'"));
    };
    let end = value[start..]
        .find(';')
        .map_or(value.len(), |offset| start + offset);
    let mut policy = String::with_capacity(value.len() + nonce.len() + 10);
    policy.push_str(&value[..end]);
    policy.push_str(" 'nonce-");
    policy.push_str(nonce);
    policy.push('\'');
    policy.push_str(&value[end..]);
    HeaderValue::from_str(&policy)
        .unwrap_or_else(|_| HeaderValue::from_static("default-src 'none'"))
}

#[cfg(test)]
mod tests {
    use fundamental_core::Error as _;

    use super::*;

    #[test]
    fn a_value_carrying_a_line_break_refuses_to_serve_as_api_018() {
        let refusal = SecurityHeaderLayer::from_fields(
            [(
                "content-security-policy",
                "default-src 'none'\r\nSet-Cookie: stolen=1",
                HeaderCombine::Alongside,
            )]
            .into_iter(),
        )
        .err()
        .expect("a line break is not a field value");
        assert!(matches!(
            refusal,
            ServerError::SecurityHeaderInvalid {
                header: "content-security-policy",
                ..
            }
        ));
        assert_eq!(refusal.error_code(), "API_018");
    }

    #[test]
    fn every_header_of_a_policy_goes_on_the_wire() {
        let layer = SecurityHeaderLayer::new(&SecurityHeaders::api()).expect("a valid policy");
        assert_eq!(layer.fields.len(), SecurityHeaders::api().headers().len());
    }

    #[test]
    fn a_leptos_stream_nonce_is_added_to_the_existing_script_policy() {
        let input = HeaderValue::from_static(
            "default-src 'none'; script-src 'self' 'wasm-unsafe-eval' sha256-test; style-src 'self'",
        );
        let output = csp_with_nonce(&input, "requestNonce_123");
        let output = output.to_str().expect("valid CSP");
        assert!(output.contains("sha256-test"), "{output}");
        assert!(output.contains("'nonce-requestNonce_123'"), "{output}");
        assert!(output.contains("style-src 'self'"), "{output}");
    }

    #[tokio::test]
    async fn the_nonce_marker_is_consumed_and_the_csp_is_widened_per_response() {
        use axum::http::{Request, Response};
        use std::convert::Infallible;
        use tower::{Layer, ServiceExt};

        let layer = SecurityHeaderLayer::new(&SecurityHeaders::web()).expect("valid policy");
        let service = tower::service_fn(|_| async {
            let mut response = Response::new(());
            response.headers_mut().insert(
                INTERNAL_CSP_NONCE_HEADER,
                "testNonce".parse().expect("header"),
            );
            Ok::<_, Infallible>(response)
        });
        let response = layer
            .layer()
            .layer(service)
            .oneshot(Request::new(()))
            .await
            .expect("response");
        let headers = response.headers();
        assert!(!headers.contains_key(INTERNAL_CSP_NONCE_HEADER));
        let csp = headers
            .get("content-security-policy")
            .expect("policy")
            .to_str()
            .expect("text");
        assert!(csp.contains("'nonce-testNonce'"), "{csp}");
        assert!(
            csp.contains("script-src 'self' 'wasm-unsafe-eval'"),
            "{csp}"
        );
    }
}
