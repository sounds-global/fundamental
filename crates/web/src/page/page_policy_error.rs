use fundamental_macros::Error;

/// Why the router refused to start: a route's policies or loader ask for what its chain lacks.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum PagePolicyError {
    /// A loader takes an object at a relation that no guard in its route's chain checks.
    #[error(
        code = "FW_PAGE_001",
        message = "'{file}' loads a {namespace} at `{relation}`, and no `policy.rs` above route {route} authorizes exactly that. Add `.authorize(..)` for it to the policy that owns the object.",
        status = 500
    )]
    LoaderOutranksGuards {
        /// The route template.
        route: String,
        /// The loader file.
        file: &'static str,
        /// The namespace the loader asks for.
        namespace: &'static str,
        /// The relation the loader asks for.
        relation: &'static str,
    },

    /// Two guards in one chain check different objects of one namespace.
    #[error(
        code = "FW_PAGE_002",
        message = "route {route} guards two different {namespace} objects, so no loader could tell which one it was given. Guard one {namespace} per route.",
        status = 500
    )]
    TwoObjectsInOneNamespace {
        /// The route template.
        route: String,
        /// The namespace guarded twice.
        namespace: String,
    },

    /// A route parameter no guard checks and no policy declares public.
    #[error(
        code = "FW_PAGE_003",
        message = "route {route} has a parameter `{param}` that no policy guards or declares public. Guard it with `.authorize(..).object(\"{param}\")`, or declare it with `.public_segment` or `.public_param`.",
        status = 500
    )]
    UnguardedParam {
        /// The route template.
        route: String,
        /// The parameter nobody accounts for.
        param: String,
    },

    /// A guard names a parameter its route does not have.
    #[error(
        code = "FW_PAGE_004",
        message = "'{file}' guards the parameter `{param}`, and route {route} has none. A policy guards the parameters of the routes below it.",
        status = 500
    )]
    GuardOnAbsentParam {
        /// The route template.
        route: String,
        /// The policy file.
        file: &'static str,
        /// The parameter the guard names.
        param: &'static str,
    },

    /// A raw public parameter below a guard whose object comes from a parameter.
    #[error(
        code = "FW_PAGE_005",
        message = "route {route} guards an object from its address and also declares `{param}` a raw public parameter, which could name another object's records. Guard `{param}` too, or make it a `public_segment` of a fieldless enum.",
        status = 500
    )]
    RawParamUnderObjectGuard {
        /// The route template.
        route: String,
        /// The raw public parameter.
        param: &'static str,
    },
}

#[cfg(test)]
mod tests {
    use super::PagePolicyError;
    use fundamental_core::Error as _;

    #[test]
    fn every_variant_carries_its_code_and_names_the_route() {
        let cases = [
            (
                PagePolicyError::LoaderOutranksGuards {
                    route: "/teams/{team}".into(),
                    file: "teams/[team]/loader.rs",
                    namespace: "team",
                    relation: "admin",
                },
                "FW_PAGE_001",
            ),
            (
                PagePolicyError::TwoObjectsInOneNamespace {
                    route: "/teams/{team}".into(),
                    namespace: "team".into(),
                },
                "FW_PAGE_002",
            ),
            (
                PagePolicyError::UnguardedParam {
                    route: "/teams/{team}".into(),
                    param: "team".into(),
                },
                "FW_PAGE_003",
            ),
            (
                PagePolicyError::GuardOnAbsentParam {
                    route: "/teams/{team}".into(),
                    file: "policy.rs",
                    param: "project",
                },
                "FW_PAGE_004",
            ),
            (
                PagePolicyError::RawParamUnderObjectGuard {
                    route: "/teams/{team}".into(),
                    param: "project",
                },
                "FW_PAGE_005",
            ),
        ];
        for (error, code) in cases {
            assert_eq!(error.error_code(), code);
            assert_eq!(error.http_status_code(), 500);
            assert!(!error.is_retryable());
            assert!(error.to_string().contains("/teams/{team}"), "{error}");
        }
    }
}
