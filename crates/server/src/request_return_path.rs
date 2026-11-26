use crate::{HttpContext, Method, ReturnPath};

/// Rebuild the path this `GET` was for from the route it matched; `None` when there is nothing honest to return to.
#[must_use]
pub fn request_return_path<S>(context: &HttpContext<S>) -> Option<ReturnPath> {
    if !matches!(context.method(), Some(Method::Get)) {
        return None;
    }
    let template = context.route_template()?;
    let params = context.params();
    // A declared segment returns as it was asked, not as the record's id.
    let requested: Vec<(&str, String)> = params
        .iter()
        .filter_map(|(name, _)| Some((name, context.segments().requested(name)?)))
        .collect();
    ReturnPath::from_route(
        template,
        |name| {
            requested
                .iter()
                .find(|(param, _)| *param == name)
                .map(|(_, raw)| raw.as_str())
                .or_else(|| params.get(name))
        },
        context.query_raw(),
    )
}
