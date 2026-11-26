use crate::HstsMaxAge;

/// A `Strict-Transport-Security` policy (RFC 6797), built only through constructors the preload list accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hsts {
    max_age: HstsMaxAge,
    subdomains: bool,
    preload: bool,
}

impl Hsts {
    /// HTTPS-only for this host, for `max_age`.
    #[must_use]
    pub const fn host(max_age: HstsMaxAge) -> Self {
        Self {
            max_age,
            subdomains: false,
            preload: false,
        }
    }

    /// HTTPS-only for this host and every name under it, for `max_age`.
    #[must_use]
    pub const fn with_subdomains(max_age: HstsMaxAge) -> Self {
        Self {
            max_age,
            subdomains: true,
            preload: false,
        }
    }

    /// Two years, every subdomain, and eligible for the browsers' preload list.
    #[must_use]
    pub const fn preload() -> Self {
        Self {
            max_age: HstsMaxAge::TwoYears,
            subdomains: true,
            preload: true,
        }
    }

    /// The header value.
    #[must_use]
    pub fn value(&self) -> String {
        let mut value = format!("max-age={}", self.max_age.seconds());
        if self.subdomains {
            value.push_str("; includeSubDomains");
        }
        if self.preload {
            value.push_str("; preload");
        }
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preload_is_two_years_with_subdomains_and_preload() {
        assert_eq!(
            Hsts::preload().value(),
            "max-age=63072000; includeSubDomains; preload"
        );
    }

    #[test]
    fn a_host_policy_never_reaches_subdomains_or_the_preload_list() {
        assert_eq!(Hsts::host(HstsMaxAge::FiveMinutes).value(), "max-age=300");
        assert_eq!(
            Hsts::with_subdomains(HstsMaxAge::OneYear).value(),
            "max-age=31536000; includeSubDomains"
        );
    }
}
