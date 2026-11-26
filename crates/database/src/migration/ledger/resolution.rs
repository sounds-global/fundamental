//! [`Resolution`]: who settles a failed migration, and why.

use super::LedgerError;

/// Who settles a failed migration and why, as the resolution record keeps it.
///
/// Who is read from the environment: `SUDO_USER`, else `USER`; neither set, or an empty
/// reason, is refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolution {
    by: String,
    reason: String,
}

impl Resolution {
    /// The resolution `reason` gives, by the person this process runs for.
    ///
    /// # Errors
    /// [`LedgerError::NoReason`] when `reason` is empty or only whitespace;
    /// [`LedgerError::NoOperator`] when neither `SUDO_USER` nor `USER` is set.
    pub fn new(reason: &str) -> Result<Self, LedgerError> {
        Self::read(reason, |key| std::env::var(key).ok())
    }

    fn read(reason: &str, lookup: impl Fn(&str) -> Option<String>) -> Result<Self, LedgerError> {
        let reason = reason.trim();
        if reason.is_empty() {
            return Err(LedgerError::NoReason);
        }
        let by = ["SUDO_USER", "USER"]
            .into_iter()
            .filter_map(lookup)
            .map(|value| value.trim().to_owned())
            .find(|value| !value.is_empty())
            .ok_or(LedgerError::NoOperator)?;
        Ok(Self {
            by,
            reason: reason.to_owned(),
        })
    }

    /// Who settles it.
    #[must_use]
    pub fn by(&self) -> &str {
        &self.by
    }

    /// Why.
    #[must_use]
    pub fn reason(&self) -> &str {
        &self.reason
    }
}

#[cfg(test)]
mod tests {
    use super::Resolution;
    use crate::migration::LedgerError;

    fn env<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |key| {
            pairs
                .iter()
                .find(|(name, _)| *name == key)
                .map(|(_, value)| (*value).to_owned())
        }
    }

    #[test]
    fn the_person_behind_sudo_is_named_before_the_service_account() {
        let resolution = Resolution::read(
            " timeout at peak ",
            env(&[("SUDO_USER", "angelo"), ("USER", "app-ops-migrate")]),
        )
        .expect("a resolution");
        assert_eq!(resolution.by(), "angelo");
        assert_eq!(resolution.reason(), "timeout at peak");
    }

    #[test]
    fn without_sudo_the_user_is_named() {
        let resolution =
            Resolution::read("rerun at night", env(&[("USER", "angelo")])).expect("a resolution");
        assert_eq!(resolution.by(), "angelo");
    }

    #[test]
    fn nobody_to_name_is_refused_rather_than_recorded_blank() {
        assert!(matches!(
            Resolution::read("why", env(&[("USER", " ")])),
            Err(LedgerError::NoOperator)
        ));
    }

    #[test]
    fn an_empty_reason_is_refused() {
        assert!(matches!(
            Resolution::read("  ", env(&[("USER", "angelo")])),
            Err(LedgerError::NoReason)
        ));
    }
}
