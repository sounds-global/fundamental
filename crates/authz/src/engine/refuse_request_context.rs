//! [`refuse_request_context`]: the one gate a check's request context passes.

use tracing::warn;

use crate::error::AuthzError;
use crate::tuple::CaveatContext;

/// Refuses a request context that binds any caveat parameter.
///
/// Anything a caller binds could move the clock or override a stored argument, widening a
/// grant. Called by every entry point before any fast path.
///
/// # Errors
///
/// [`AuthzError::RequestCaveatParameter`] naming the first bound parameter.
pub(crate) fn refuse_request_context(context: &CaveatContext) -> Result<(), AuthzError> {
    let Some(parameter) = context.first_name() else {
        return Ok(());
    };
    let count = context.len();
    warn!(
        parameter = %parameter,
        count,
        "authorization check refused: the request context bound a caveat parameter"
    );
    Err(AuthzError::RequestCaveatParameter {
        parameter: parameter.to_owned(),
        count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::CaveatValue;
    use fundamental_core::Error as _;

    #[test]
    fn an_empty_context_passes() {
        assert!(refuse_request_context(&CaveatContext::new()).is_ok());
    }

    #[test]
    fn any_binding_is_refused_naming_the_first() {
        let context = CaveatContext::new()
            .with("until", CaveatValue::Timestamp(1))
            .with("region", CaveatValue::String("eu".into()));
        let refused = refuse_request_context(&context).expect_err("must refuse");
        assert!(matches!(
            &refused,
            AuthzError::RequestCaveatParameter { parameter, count: 2 } if parameter == "region"
        ));
        assert_eq!(refused.error_code(), "AUTHZ_019");
        assert_eq!(refused.http_status_code(), 400);
    }
}
