use std::fmt;
use std::net::{Ipv4Addr, Ipv6Addr};

/// Longer than any origin a browser serialises.
const MAX_LEN: usize = 300;

/// The `Origin` header of a request, as it may be recorded: a class, or a parsed scheme, host and port.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ObservedOrigin {
    /// The request carried no `Origin`.
    Absent,
    /// The opaque origin `null` — a sandboxed frame, some redirect chains.
    Opaque,
    /// Present, and not an origin.
    Malformed,
    /// A parsed origin.
    Web {
        /// `http`, `https`, or `other` for any other well-formed scheme.
        scheme: &'static str,
        /// The host, lower-cased.
        host: String,
        /// The explicit port, if one was written.
        port: Option<u16>,
    },
}

impl ObservedOrigin {
    /// Parses an `Origin` header value; `None` is [`ObservedOrigin::Absent`].
    #[must_use]
    pub fn parse(value: Option<&str>) -> Self {
        let Some(value) = value else {
            return Self::Absent;
        };
        if value == "null" {
            return Self::Opaque;
        }
        if value.len() > MAX_LEN {
            return Self::Malformed;
        }
        parse_web(value).unwrap_or(Self::Malformed)
    }
}

impl fmt::Display for ObservedOrigin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Absent => f.write_str("absent"),
            Self::Opaque => f.write_str("null"),
            Self::Malformed => f.write_str("malformed"),
            Self::Web { scheme, host, port } => {
                write!(f, "{scheme}://{host}")?;
                match port {
                    Some(port) => write!(f, ":{port}"),
                    None => Ok(()),
                }
            }
        }
    }
}

fn parse_web(value: &str) -> Option<ObservedOrigin> {
    let (scheme, authority) = value.split_once("://")?;
    let scheme = parse_scheme(scheme)?;
    let (host, port) = split_port(authority)?;
    Some(ObservedOrigin::Web {
        scheme,
        host: parse_host(host)?,
        port,
    })
}

/// RFC 3986 §3.1: `ALPHA *( ALPHA / DIGIT / "+" / "-" / "." )`.
fn parse_scheme(scheme: &str) -> Option<&'static str> {
    let mut bytes = scheme.bytes();
    let first = bytes.next()?;
    if !first.is_ascii_alphabetic()
        || !bytes.all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'-' | b'.'))
    {
        return None;
    }
    Some(if scheme.eq_ignore_ascii_case("https") {
        "https"
    } else if scheme.eq_ignore_ascii_case("http") {
        "http"
    } else {
        "other"
    })
}

/// Splits `host[:port]`, where a bracketed IPv6 host carries its own colons.
fn split_port(authority: &str) -> Option<(&str, Option<u16>)> {
    let (host, port) = match authority.strip_prefix('[') {
        Some(rest) => {
            let (inner, after) = rest.split_once(']')?;
            let port = match after {
                "" => None,
                _ => Some(after.strip_prefix(':')?),
            };
            (&authority[..inner.len() + 2], port)
        }
        None => match authority.rsplit_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (authority, None),
        },
    };
    let port = match port {
        None => None,
        Some(digits) if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) => {
            Some(digits.parse().ok()?)
        }
        Some(_) => return None,
    };
    Some((host, port))
}

fn parse_host(host: &str) -> Option<String> {
    if let Some(inner) = host.strip_prefix('[').and_then(|h| h.strip_suffix(']')) {
        // `Ipv6Addr` refuses a `%` zone id, which a browser never serialises.
        let address: Ipv6Addr = inner.parse().ok()?;
        return Some(format!("[{address}]"));
    }
    if let Ok(address) = host.parse::<Ipv4Addr>() {
        return Some(address.to_string());
    }
    is_dns_name(host).then(|| host.to_ascii_lowercase())
}

/// An ASCII DNS name: at most 253 bytes, labels of 1–63 letters, digits and inner hyphens.
fn is_dns_name(host: &str) -> bool {
    host.len() <= 253
        && host.split('.').all(|label| {
            let bytes = label.as_bytes();
            (1..=63).contains(&bytes.len())
                && bytes
                    .iter()
                    .all(|b| b.is_ascii_alphanumeric() || *b == b'-')
                && bytes.first() != Some(&b'-')
                && bytes.last() != Some(&b'-')
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(value: &str) -> String {
        ObservedOrigin::parse(Some(value)).to_string()
    }

    #[test]
    fn a_browser_origin_parses() {
        assert_eq!(parsed("https://app.example.com"), "https://app.example.com");
        assert_eq!(
            parsed("HTTP://App.Example.COM:8443"),
            "http://app.example.com:8443"
        );
        assert_eq!(
            parsed("https://xn--bcher-kva.example"),
            "https://xn--bcher-kva.example"
        );
        assert_eq!(parsed("http://127.0.0.1:9620"), "http://127.0.0.1:9620");
        assert_eq!(parsed("http://[::1]:8080"), "http://[::1]:8080");
        assert_eq!(
            parsed("chrome-extension://abcdefghijklmnop"),
            "other://abcdefghijklmnop"
        );
    }

    #[test]
    fn absent_and_opaque_are_classes() {
        assert_eq!(ObservedOrigin::parse(None), ObservedOrigin::Absent);
        assert_eq!(parsed("null"), "null");
    }

    #[test]
    fn anything_but_an_origin_is_malformed_and_kept_as_nothing() {
        let hostile = [
            "jan@example.nl",
            "https://jan@example.nl",
            "https://example.nl/reset/jan",
            "https://example.nl?email=jan@example.nl",
            "https://example.nl.",
            "https://exa mple.nl",
            "https://exämple.nl",
            "https://[fe80::1%25eth0]",
            "https://example.nl:",
            "https://example.nl:99999",
            "https://example.nl:+80",
            "https://-example.nl",
            "https://\u{1b}[2J",
            "://example.nl",
            "1https://example.nl",
            "",
        ];
        for value in hostile {
            assert_eq!(
                ObservedOrigin::parse(Some(value)),
                ObservedOrigin::Malformed,
                "{value:?}"
            );
        }
    }

    #[test]
    fn an_oversized_value_is_malformed_before_it_is_read() {
        let huge = format!("https://{}", "a".repeat(10 * 1024));
        assert_eq!(parsed(&huge), "malformed");
    }

    #[test]
    fn a_long_host_within_the_limits_parses() {
        let label = "a".repeat(63);
        let host = format!("{label}.{label}.{label}.example");
        assert_eq!(
            parsed(&format!("https://{host}")),
            format!("https://{host}")
        );
    }
}
