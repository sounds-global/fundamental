//! [`names_loopback`]: whether an address stays on this host.

use std::net::IpAddr;

use url::{Host, Url};

/// Whether `url`'s host is loopback or `localhost`, parsed as the WHATWG parser the client dials with.
#[must_use]
pub(crate) fn names_loopback(url: &str) -> bool {
    let Ok(parsed) = Url::parse(url.trim()) else {
        return false;
    };
    match parsed.host() {
        Some(Host::Ipv4(address)) => address.is_loopback(),
        Some(Host::Ipv6(address)) => address.is_loopback(),
        Some(Host::Domain(name)) => {
            name.eq_ignore_ascii_case("localhost")
                || name
                    .trim_start_matches('[')
                    .trim_end_matches(']')
                    .parse::<IpAddr>()
                    .is_ok_and(|address| address.is_loopback())
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::names_loopback;

    #[test]
    fn a_loopback_host_is_recognised_in_every_spelling() {
        for url in [
            "http://127.0.0.1:8200",
            "https://127.10.0.3",
            "nats://localhost:4222",
            "http://[::1]:8200/v1",
            "nats://user@127.0.0.1:4222",
            "nats://[::1]:4222",
            "http://0x7f.1:8200",
        ] {
            assert!(names_loopback(url), "{url}");
        }
    }

    #[test]
    fn a_network_host_or_an_unreadable_address_is_not() {
        for url in [
            "http://vault.internal:8200",
            "nats://10.0.0.4:4222",
            "http://127.0.0.1.attacker.example",
            "http://localhost@evil.example",
            "http://evil.example\\@127.0.0.1:8200",
            "http://evil.example\\@127.0.0.1",
            "127.0.0.1:8200",
            "",
        ] {
            assert!(!names_loopback(url), "{url}");
        }
    }
}
