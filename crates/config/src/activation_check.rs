//! [`activation_check`]: the sockets a process was passed agree with the surfaces listening on them.

use fundamental_core::listen::{ActivationFault, SocketName};

use crate::{ConfigError, ConfigResult};

/// Refuses a boot whose passed sockets and activated surfaces disagree; no sockets at all passes.
///
/// # Errors
/// [`ConfigError::ActivationMalformed`] (`CFG_028`), [`ConfigError::ActivatedSocketMissing`]
/// (`CFG_026`) or [`ConfigError::ActivatedSocketUnclaimed`] (`CFG_027`).
pub(crate) fn activation_check<'a>(
    activated: impl IntoIterator<Item = &'a str>,
    passed: Result<&[SocketName], &ActivationFault>,
) -> ConfigResult<()> {
    let passed = passed.map_err(|fault| ConfigError::ActivationMalformed {
        reason: fault.to_string(),
    })?;
    if passed.is_empty() {
        return Ok(());
    }
    let activated: Vec<&str> = activated.into_iter().collect();
    if let Some(surface) = activated
        .iter()
        .find(|surface| !passed.iter().any(|name| name.as_str() == **surface))
    {
        return Err(ConfigError::ActivatedSocketMissing {
            surface: (*surface).to_owned(),
        });
    }
    if let Some(name) = passed
        .iter()
        .find(|name| !activated.contains(&name.as_str()))
    {
        return Err(ConfigError::ActivatedSocketUnclaimed {
            name: name.as_str().to_owned(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use fundamental_core::Error as _;

    use super::*;

    fn names(raw: &[&str]) -> Vec<SocketName> {
        raw.iter()
            .map(|name| SocketName::parse(name).expect("a socket name"))
            .collect()
    }

    #[test]
    fn a_process_passed_no_socket_boots_whatever_listens_activated() {
        assert!(activation_check(["web", "admin"], Ok(&[])).is_ok());
    }

    #[test]
    fn each_activated_surface_takes_the_socket_passed_under_its_name() {
        let passed = names(&["admin", "web"]);

        assert!(activation_check(["web", "admin"], Ok(&passed)).is_ok());
    }

    #[test]
    fn an_activated_surface_with_no_socket_of_its_name_refuses_the_boot() {
        let passed = names(&["web"]);

        let err = activation_check(["web", "admin"], Ok(&passed)).expect_err("admin has no socket");

        assert!(
            matches!(&err, ConfigError::ActivatedSocketMissing { surface } if surface == "admin"),
            "{err:?}"
        );
        assert_eq!(err.error_code(), "CFG_026");
        assert_eq!(err.http_status_code(), 500);
    }

    #[test]
    fn a_passed_socket_no_surface_listens_on_refuses_the_boot() {
        let passed = names(&["web", "sync"]);

        let err = activation_check(["web"], Ok(&passed)).expect_err("sync is unclaimed");

        assert!(
            matches!(&err, ConfigError::ActivatedSocketUnclaimed { name } if name == "sync"),
            "{err:?}"
        );
        assert_eq!(err.error_code(), "CFG_027");
        assert_eq!(err.http_status_code(), 500);
    }

    #[test]
    fn a_socket_passed_while_nothing_listens_activated_refuses_the_boot() {
        let passed = names(&["web"]);

        let err = activation_check([], Ok(&passed)).expect_err("web is unclaimed");

        assert_eq!(err.error_code(), "CFG_027");
    }

    #[test]
    fn passed_sockets_that_cannot_be_read_refuse_the_boot() {
        let fault = ActivationFault::Malformed("a name is passed twice");

        let err = activation_check(["web"], Err(&fault)).expect_err("malformed");

        assert!(
            matches!(&err, ConfigError::ActivationMalformed { reason } if reason.contains("a name is passed twice")),
            "{err:?}"
        );
        assert_eq!(err.error_code(), "CFG_028");
        assert_eq!(err.http_status_code(), 500);
    }
}
