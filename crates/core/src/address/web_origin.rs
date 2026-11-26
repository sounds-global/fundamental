//! [`WebOrigin`] — the scheme, host and port a surface is published at.

#[cfg(not(feature = "std"))]
use alloc::{
    format,
    string::{String, ToString},
};
use core::fmt;

use crate::address::{AbsoluteUrl, AddressError};

/// The origin a surface is published at: `https://`, a host, and a port only when it is
/// not the scheme's default.
///
/// Never the request's `Host`, so a header cannot choose the URL a page names as its own.
///
/// ```
/// # extern crate fundamental_core;
/// use fundamental_core::address::WebOrigin;
///
/// let origin = WebOrigin::parse("https://Example.NL:443/")?;
/// assert_eq!(origin.as_str(), "https://example.nl");
/// assert!(WebOrigin::parse("http://example.nl").is_err());
/// # Ok::<(), fundamental_core::address::AddressError>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct WebOrigin {
    serialized: String,
    host_end: usize,
}

impl WebOrigin {
    /// Parses a published origin: `https`, a DNS name of two or more labels, an optional
    /// port.
    ///
    /// # Errors
    /// [`AddressError::OriginNotHttps`] (`ADDR_009`) for another scheme;
    /// [`AddressError::OriginInvalid`] (`ADDR_008`) for everything else, naming what is
    /// wrong.
    pub fn parse(input: &str) -> Result<Self, AddressError> {
        let rest = input
            .strip_prefix("https://")
            .ok_or_else(|| AddressError::OriginNotHttps {
                origin: input.to_string(),
            })?;
        let (host, port) = split_authority(input, rest)?;
        check_dns_name(input, &host)?;
        Ok(Self::assemble("https", &host, port, 443))
    }

    /// Parses an origin for a surface running on the developer's own machine: `http` or
    /// `https`, and a host that is `localhost`, a `*.localhost` name or a loopback
    /// address.
    ///
    /// # Errors
    /// [`AddressError::OriginInvalid`] (`ADDR_008`) when the host is not a loopback name,
    /// or the origin is malformed as [`parse`](Self::parse) describes.
    pub fn loopback_for_development(input: &str) -> Result<Self, AddressError> {
        let (scheme, rest, default_port) = if let Some(rest) = input.strip_prefix("https://") {
            ("https", rest, 443)
        } else if let Some(rest) = input.strip_prefix("http://") {
            ("http", rest, 80)
        } else {
            return Err(invalid(input, "is neither http:// nor https://"));
        };
        let (host, port) = split_authority(input, rest)?;
        let loopback = host == "localhost"
            || host.ends_with(".localhost")
            || host == "127.0.0.1"
            || host == "[::1]";
        if !loopback {
            return Err(invalid(input, "is not a loopback host"));
        }
        if !host.starts_with('[') && host != "127.0.0.1" {
            check_labels(input, &host)?;
        }
        Ok(Self::assemble(scheme, &host, port, default_port))
    }

    /// The origin as it is written in every URL: `https://example.nl`, no trailing `/`.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.serialized
    }

    /// The host, lower-cased, without its port.
    #[must_use]
    pub fn host(&self) -> &str {
        let start = self.serialized.find("://").map_or(0, |at| at + 3);
        &self.serialized[start..self.host_end]
    }

    /// The authority a browser on this origin sends as its `Host`: the host, and the port
    /// only when it is not the scheme's default — `example.nl`, `localhost:3000`.
    #[must_use]
    pub fn authority(&self) -> &str {
        &self.serialized[self.serialized.find("://").map_or(0, |at| at + 3)..]
    }

    /// Whether a request addressed to `authority` (its `Host`, or HTTP/2 `:authority`) is
    /// addressed to this origin.
    #[must_use]
    pub fn serves_authority(&self, authority: &str) -> bool {
        let default_port = match self.is_https() {
            true => ":443",
            false => ":80",
        };
        let asked = authority.strip_suffix(default_port).unwrap_or(authority);
        asked.eq_ignore_ascii_case(self.authority())
    }

    /// Whether this origin's host is a special-use name the public internet never resolves:
    /// loopback, a last label of `test`, `invalid`, `example`, `localhost`, `local` or
    /// `internal`, or a name ending in `home.arpa`.
    #[must_use]
    pub fn is_special_use(&self) -> bool {
        const RESERVED: [&str; 6] = [
            "test",
            "invalid",
            "example",
            "localhost",
            "local",
            "internal",
        ];
        let host = self.host();
        let ip_literal = host.starts_with('[')
            || host
                .bytes()
                .all(|byte| byte.is_ascii_digit() || byte == b'.');
        let last_label = host.rsplit('.').next().unwrap_or(host);
        ip_literal
            || RESERVED.contains(&last_label)
            || host == "home.arpa"
            || host.ends_with(".home.arpa")
    }

    /// Whether this origin is served over `https`.
    #[must_use]
    pub fn is_https(&self) -> bool {
        self.serialized.starts_with("https://")
    }

    /// The absolute URL of `address` on this origin: a path an address space built, or one a
    /// request already spelled (a page in the reader's language, for a link in a mail).
    ///
    /// Concatenated, never resolved, so a path spelled `//elsewhere` cannot name another host.
    #[must_use]
    pub fn url(&self, address: &str) -> AbsoluteUrl {
        AbsoluteUrl::from_parts(&self.serialized, address)
    }

    fn assemble(scheme: &str, host: &str, port: Option<u16>, default_port: u16) -> Self {
        let authority_start = scheme.len() + 3;
        let serialized = match port {
            Some(port) if port != default_port => format!("{scheme}://{host}:{port}"),
            _ => format!("{scheme}://{host}"),
        };
        Self {
            serialized,
            host_end: authority_start + host.len(),
        }
    }
}

impl fmt::Display for WebOrigin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.serialized)
    }
}

/// Splits `host[:port][/]` into a lower-cased host and a port, refusing everything an
/// origin may not carry.
fn split_authority(input: &str, rest: &str) -> Result<(String, Option<u16>), AddressError> {
    if input
        .bytes()
        .any(|byte| byte.is_ascii_control() || byte == b' ')
    {
        return Err(invalid(input, "carries whitespace or a control character"));
    }
    let authority = rest.strip_suffix('/').unwrap_or(rest);
    if authority.contains(['/', '?', '#']) {
        return Err(invalid(input, "carries a path, a query or a fragment"));
    }
    if authority.contains('@') {
        return Err(invalid(input, "carries userinfo"));
    }
    if authority.is_empty() {
        return Err(invalid(input, "has no host"));
    }
    let (host, port) = if authority.starts_with('[') {
        let close = authority
            .find(']')
            .ok_or_else(|| invalid(input, "has an unclosed IPv6 bracket"))?;
        let (host, after) = authority.split_at(close + 1);
        match after {
            "" => (host, None),
            _ => (
                host,
                Some(
                    after
                        .strip_prefix(':')
                        .ok_or_else(|| invalid(input, "has text after its IPv6 address"))?,
                ),
            ),
        }
    } else {
        match authority.split_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (authority, None),
        }
    };
    if host.is_empty() {
        return Err(invalid(input, "has no host"));
    }
    let port = match port {
        None => None,
        Some(digits) => {
            let parsed = digits
                .bytes()
                .all(|byte| byte.is_ascii_digit())
                .then(|| digits.parse::<u16>().ok())
                .flatten()
                .filter(|port| *port != 0)
                .ok_or_else(|| invalid(input, "has a port that is not 1-65535"))?;
            Some(parsed)
        }
    };
    Ok((host.to_ascii_lowercase(), port))
}

/// A published host: two or more DNS labels, and not an IP address.
fn check_dns_name(input: &str, host: &str) -> Result<(), AddressError> {
    if host.starts_with('[') {
        return Err(invalid(input, "is an IP address, not a name"));
    }
    check_labels(input, host)?;
    let mut labels = host.split('.');
    let top = labels.next_back().unwrap_or_default();
    if labels.next().is_none() {
        return Err(invalid(input, "is a single-label name"));
    }
    if top.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(invalid(input, "is an IP address, not a name"));
    }
    Ok(())
}

/// Every label 1–63 letters, digits and inner hyphens; the whole name at most 253 bytes.
fn check_labels(input: &str, host: &str) -> Result<(), AddressError> {
    if host.len() > 253 {
        return Err(invalid(input, "has a host longer than 253 bytes"));
    }
    if host.ends_with('.') {
        return Err(invalid(input, "ends in a dot"));
    }
    for label in host.split('.') {
        let well_formed = (1..=63).contains(&label.len())
            && label
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
            && !label.starts_with('-')
            && !label.ends_with('-');
        if !well_formed {
            return Err(invalid(
                input,
                "has a label that is not 1-63 letters, digits and inner hyphens",
            ));
        }
    }
    Ok(())
}

fn invalid(input: &str, reason: &'static str) -> AddressError {
    AddressError::OriginInvalid {
        origin: input.to_string(),
        reason,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(not(feature = "std"))]
    use alloc::vec;

    #[test]
    fn a_url_on_an_origin_never_names_another_host() {
        let origin = WebOrigin::parse("https://mijn.example.nl").expect("origin");
        assert_eq!(
            origin.url("/en/aanmelden/code?next=%2F").as_str(),
            "https://mijn.example.nl/en/aanmelden/code?next=%2F"
        );
        assert_eq!(
            origin.url("//elsewhere.example/x").as_str(),
            "https://mijn.example.nl//elsewhere.example/x"
        );
    }

    #[test]
    fn a_published_origin_is_written_one_way() {
        for spelling in [
            "https://example.nl",
            "https://example.nl/",
            "https://EXAMPLE.nl",
            "https://example.nl:443",
        ] {
            let origin = WebOrigin::parse(spelling).expect(spelling);
            assert_eq!(origin.as_str(), "https://example.nl", "{spelling}");
            assert_eq!(origin.host(), "example.nl");
        }
        let ported = WebOrigin::parse("https://mijn.example.nl:8443").expect("port");
        assert_eq!(ported.as_str(), "https://mijn.example.nl:8443");
        assert_eq!(ported.host(), "mijn.example.nl");
        assert!(ported.is_https());
    }

    #[test]
    fn plain_http_is_refused_as_addr_009() {
        let error = WebOrigin::parse("http://example.nl").expect_err("http");
        assert_eq!(error.code(), "ADDR_009");
    }

    #[test]
    fn everything_an_origin_may_not_carry_is_refused_as_addr_008() {
        let refused = vec![
            "https://example.nl/path",
            "https://example.nl?q=1",
            "https://example.nl#top",
            "https://user@example.nl",
            "https://example.nl.",
            "https://localhost",
            "https://192.0.2.1",
            "https://[2001:db8::1]",
            "https://exa_mple.nl",
            "https://-example.nl",
            "https://example.nl:0",
            "https://example.nl:99999",
            "https://example.nl:",
            "https://example.nl:+443",
            "https://",
            "https://exam ple.nl",
            "https://example.nl\r\nSet-Cookie: x",
            "https://bücher.de",
        ];
        for spelling in refused {
            let error = WebOrigin::parse(spelling).expect_err(spelling);
            assert_eq!(error.code(), "ADDR_008", "{spelling}");
        }
    }

    #[test]
    fn an_international_name_is_accepted_in_its_ascii_form() {
        let origin = WebOrigin::parse("https://xn--bcher-kva.de").expect("punycode");
        assert_eq!(origin.host(), "xn--bcher-kva.de");
    }

    #[test]
    fn a_loopback_origin_is_only_reachable_through_its_own_constructor() {
        let dev = WebOrigin::loopback_for_development("http://localhost:3000").expect("dev");
        assert_eq!(dev.as_str(), "http://localhost:3000");
        assert!(!dev.is_https());
        assert!(WebOrigin::loopback_for_development("http://www.app.localhost").is_ok());
        assert!(WebOrigin::loopback_for_development("http://127.0.0.1:8080").is_ok());
        assert_eq!(
            WebOrigin::loopback_for_development("http://example.nl")
                .expect_err("not loopback")
                .code(),
            "ADDR_008"
        );
        assert!(WebOrigin::parse("http://localhost:3000").is_err());
    }

    #[test]
    fn a_special_use_name_is_known_by_its_whole_last_label() {
        for origin in [
            "https://winkel.test",
            "https://mijn.x.winkel-lane.test",
            "https://x.invalid",
            "https://x.example",
            "https://www.app.localhost",
            "https://x.local",
            "https://x.internal",
            "https://router.home.arpa",
        ] {
            let origin = WebOrigin::parse(origin).expect("origin");
            assert!(origin.is_special_use(), "{origin}");
        }
        for origin in [
            "http://localhost:3000",
            "http://127.0.0.1:8080",
            "http://[::1]",
        ] {
            let origin = WebOrigin::loopback_for_development(origin).expect("loopback");
            assert!(origin.is_special_use(), "{origin}");
        }
        for origin in [
            "https://mijn.winkel.nl",
            "https://mijn.test.nl",
            "https://testing.nl",
            "https://example.com",
            "https://home.arpa.nl",
            "https://xhome.arpa",
        ] {
            let origin = WebOrigin::parse(origin).expect("origin");
            assert!(!origin.is_special_use(), "{origin}");
        }
    }

    #[test]
    fn an_authority_is_matched_to_the_origin_it_names_and_nothing_else() {
        let origin = WebOrigin::parse("https://shop.com").expect("origin");
        for asked in ["shop.com", "SHOP.com", "shop.com:443"] {
            assert!(origin.serves_authority(asked), "{asked}");
        }
        for asked in ["shop.com:8443", "evil.shop.com", "shop.com.evil", ""] {
            assert!(!origin.serves_authority(asked), "{asked}");
        }
        let dev =
            WebOrigin::loopback_for_development("http://en.a7d.localhost:11001").expect("dev");
        assert!(dev.serves_authority("en.a7d.localhost:11001"));
        assert!(!dev.serves_authority("nl.a7d.localhost:11001"));
    }

    #[test]
    fn the_authority_is_host_and_port_without_a_default_port() {
        for (origin, authority) in [
            ("https://Example.NL:443/", "example.nl"),
            ("https://example.nl:8443", "example.nl:8443"),
        ] {
            let origin = WebOrigin::parse(origin).expect("origin");
            assert_eq!(origin.authority(), authority);
            assert!(origin.serves_authority(authority));
        }
        let dev = WebOrigin::loopback_for_development("http://localhost:80").expect("dev");
        assert_eq!(dev.authority(), "localhost");
        let dev = WebOrigin::loopback_for_development("http://localhost:3000").expect("dev");
        assert_eq!(dev.authority(), "localhost:3000");
    }

    #[test]
    fn a_url_is_the_origin_followed_by_the_address_never_a_resolution() {
        let origin = WebOrigin::parse("https://example.nl").expect("origin");
        assert_eq!(
            origin.url("/en/prijzen").as_str(),
            "https://example.nl/en/prijzen"
        );
        assert_eq!(
            origin.url("//elsewhere.example").as_str(),
            "https://example.nl//elsewhere.example"
        );
    }
}
