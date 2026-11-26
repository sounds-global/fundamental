use crate::{Method, Peer, RequestHeaders};

/// The part of a request that exists before the server does any work for it: no body, no principal.
#[derive(Debug, Clone, Copy)]
pub struct AdmissionRequest<'a> {
    peer: Option<Peer>,
    method: Option<Method>,
    headers: &'a RequestHeaders,
}

impl<'a> AdmissionRequest<'a> {
    /// A request that arrived on `peer` with `method` (`HEAD` as `GET`, `None` if unserved) and `headers`.
    #[must_use]
    pub fn new(peer: Option<Peer>, method: Option<Method>, headers: &'a RequestHeaders) -> Self {
        Self {
            peer,
            method,
            headers,
        }
    }

    /// The socket the request arrived on: the only peer the server itself observed.
    #[must_use]
    pub fn peer(&self) -> Option<Peer> {
        self.peer
    }

    /// The method the request is routed as, or `None` for one the server does not serve.
    #[must_use]
    pub fn method(&self) -> Option<Method> {
        self.method
    }

    /// The request's headers, every one of them as the caller sent it.
    #[must_use]
    pub fn headers(&self) -> &'a RequestHeaders {
        self.headers
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_carries_the_peer_the_routed_method_and_the_headers_it_was_given() {
        let headers: RequestHeaders = [("X-Forwarded-For".to_owned(), "203.0.113.7".to_owned())]
            .into_iter()
            .collect();
        let peer = Peer::Network("198.51.100.1:40000".parse().expect("a socket address"));
        let request = AdmissionRequest::new(Some(peer), Some(Method::Get), &headers);
        assert_eq!(request.peer(), Some(peer));
        assert_eq!(request.method(), Some(Method::Get));
        assert_eq!(
            request.headers().get("x-forwarded-for"),
            Some("203.0.113.7")
        );
        assert_eq!(
            AdmissionRequest::new(None, None, &headers).method(),
            None,
            "a method the server does not serve reaches admission as None"
        );
    }
}
