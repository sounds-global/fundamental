use std::any::Any;

use axum::body::Body;
use axum::http::{Response, StatusCode};
use tracing::error;

/// Answers `500` for a panicked handler; the payload is dropped unread, as it may quote caller data.
pub(crate) fn panic_response(_payload: Box<dyn Any + Send + 'static>) -> Response<Body> {
    error!(
        "a request handler panicked; the panic message is not logged, because it can quote request data"
    );
    let mut response = Response::new(Body::empty());
    *response.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_panic_answers_500_with_no_body() {
        let response = panic_response(Box::new(String::from("jan@example.nl")));
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }
}
