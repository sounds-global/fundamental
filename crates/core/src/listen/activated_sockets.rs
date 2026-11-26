//! [`ActivatedSockets`] — the sockets the service manager passed this process, by name.

use std::env;
use std::os::unix::net::UnixListener;
use std::sync::{Mutex, OnceLock, PoisonError};

use listenfd::ListenFd;

use super::{ActivationFault, SocketName};

/// The sockets the service manager passed this process, each taken by its name at most once.
///
/// Read once per process, at boot: reading clears `LISTEN_PID` and `LISTEN_FDS`.
pub struct ActivatedSockets {
    names: Result<Vec<SocketName>, ActivationFault>,
    passed: Mutex<ListenFd>,
}

static INHERITED: OnceLock<ActivatedSockets> = OnceLock::new();

impl ActivatedSockets {
    /// This process's passed sockets, read on the first call.
    pub fn inherited() -> &'static Self {
        INHERITED.get_or_init(Self::read)
    }

    /// The names of the passed sockets, in the order they were passed.
    ///
    /// # Errors
    /// [`ActivationFault::Malformed`] when they are not each named once.
    pub fn names(&self) -> Result<&[SocketName], &ActivationFault> {
        self.names.as_deref()
    }

    /// The listening socket passed as `name`.
    ///
    /// # Errors
    /// [`ActivationFault::Malformed`] as [`names`](Self::names);
    /// [`ActivationFault::NotPassed`] when no socket of that name was passed;
    /// [`ActivationFault::Taken`] when it was taken before;
    /// [`ActivationFault::NotAUnixStream`] when it is not a unix stream socket.
    pub fn take(&self, name: &SocketName) -> Result<UnixListener, ActivationFault> {
        let names = self.names.as_ref().map_err(Clone::clone)?;
        let index = names
            .iter()
            .position(|passed| passed == name)
            .ok_or(ActivationFault::NotPassed)?;
        let mut passed = self.passed.lock().unwrap_or_else(PoisonError::into_inner);
        match passed.take_unix_listener(index) {
            Ok(Some(listener)) => Ok(listener),
            Ok(None) => Err(ActivationFault::Taken),
            Err(error) => Err(ActivationFault::NotAUnixStream(error.to_string())),
        }
    }

    fn read() -> Self {
        let count = env::var("LISTEN_FDS").ok();
        let names = env::var("LISTEN_FDNAMES").ok();
        let passed = ListenFd::from_env();
        // Sockets left for another process (a `LISTEN_PID` not this one) are none of this one's.
        let names = match passed.len() {
            0 => Ok(Vec::new()),
            held => named(count.as_deref(), names.as_deref()).and_then(|names| {
                if names.len() == held {
                    Ok(names)
                } else {
                    Err(ActivationFault::Malformed(
                        "the descriptors held disagree with the names",
                    ))
                }
            }),
        };
        Self {
            names,
            passed: Mutex::new(passed),
        }
    }
}

/// The names of `count` passed sockets, read from `LISTEN_FDS` and `LISTEN_FDNAMES`.
fn named(count: Option<&str>, names: Option<&str>) -> Result<Vec<SocketName>, ActivationFault> {
    let count = match count.filter(|count| !count.is_empty()) {
        None => return Ok(Vec::new()),
        Some(count) => count
            .parse::<usize>()
            .map_err(|_| ActivationFault::Malformed("LISTEN_FDS is not a count"))?,
    };
    if count == 0 {
        return Ok(Vec::new());
    }
    let names = names.ok_or(ActivationFault::Malformed(
        "the passed sockets carry no names",
    ))?;
    let mut named = Vec::with_capacity(count);
    for raw in names.split(':') {
        let name = SocketName::parse(raw).ok_or(ActivationFault::Malformed(
            "a passed socket's name is not a name",
        ))?;
        if named.contains(&name) {
            return Err(ActivationFault::Malformed("a name is passed twice"));
        }
        named.push(name);
    }
    if named.len() != count {
        return Err(ActivationFault::Malformed(
            "the names disagree with the count",
        ));
    }
    Ok(named)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(of: &[&str]) -> Vec<SocketName> {
        of.iter()
            .map(|name| SocketName::parse(name).expect("a name"))
            .collect()
    }

    #[test]
    fn the_names_pair_with_the_fds_in_order() {
        assert_eq!(
            named(Some("2"), Some("web:admin")),
            Ok(names(&["web", "admin"]))
        );
    }

    #[test]
    fn nothing_passed_is_no_sockets() {
        assert_eq!(named(None, None), Ok(Vec::new()));
        assert_eq!(named(Some(""), None), Ok(Vec::new()));
        assert_eq!(named(Some("0"), None), Ok(Vec::new()));
        assert_eq!(named(None, Some("web")), Ok(Vec::new()));
    }

    #[test]
    fn a_count_that_disagrees_with_the_names_is_malformed() {
        for (count, given) in [("1", "web:admin"), ("3", "web:admin")] {
            assert_eq!(
                named(Some(count), Some(given)),
                Err(ActivationFault::Malformed(
                    "the names disagree with the count"
                )),
                "{count} {given}"
            );
        }
        assert!(matches!(
            named(Some("two"), Some("web:admin")),
            Err(ActivationFault::Malformed("LISTEN_FDS is not a count"))
        ));
    }

    #[test]
    fn a_name_passed_twice_is_malformed() {
        assert_eq!(
            named(Some("2"), Some("web:web")),
            Err(ActivationFault::Malformed("a name is passed twice"))
        );
    }

    #[test]
    fn fds_without_names_are_malformed() {
        assert_eq!(
            named(Some("1"), None),
            Err(ActivationFault::Malformed(
                "the passed sockets carry no names"
            ))
        );
        assert_eq!(
            named(Some("1"), Some("unknown socket")),
            Err(ActivationFault::Malformed(
                "a passed socket's name is not a name"
            ))
        );
    }
}
