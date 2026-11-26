//! [`LocalLogin`] — a login the local node holds for a project's app, and the roles it holds.

use crate::LocalError;
use crate::database_name::plain;

/// A role's name, `[a-z][a-z0-9_]{0,62}`, written into SQL unquoted.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct RoleName(String);

impl RoleName {
    /// The name.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A login an app connects to the local node as, with the roles it reads through, as in
/// production; locally no password is needed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalLogin {
    name: RoleName,
    roles: Vec<RoleName>,
}

impl LocalLogin {
    /// The login `name` granted `roles`, when each is one plain name.
    ///
    /// # Errors
    ///
    /// [`LocalError::LoginInvalid`] (`LOCAL_026`) naming the first name that is not plain.
    pub fn parse(name: &str, roles: &[String]) -> Result<Self, LocalError> {
        let role = |raw: &str| {
            if plain(raw) {
                Ok(RoleName(raw.to_owned()))
            } else {
                Err(LocalError::LoginInvalid {
                    name: raw.to_owned(),
                })
            }
        };
        Ok(Self {
            name: role(name)?,
            roles: roles
                .iter()
                .map(|raw| role(raw))
                .collect::<Result<_, _>>()?,
        })
    }

    /// The login's name.
    #[must_use]
    pub const fn name(&self) -> &RoleName {
        &self.name
    }

    /// The roles it is granted.
    #[must_use]
    pub fn roles(&self) -> &[RoleName] {
        &self.roles
    }
}
