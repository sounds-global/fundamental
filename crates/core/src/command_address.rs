//! [`CommandAddress`], the `<namespace>:<name>` a developer command is dispatched by.

#[cfg(not(feature = "std"))]
use alloc::borrow::ToOwned;
#[cfg(not(feature = "std"))]
use alloc::string::String;
use core::fmt;

use crate::CommandAddressObjection;

/// A developer command's address: a namespace and a name, joined by a colon.
///
/// Both halves are lower-case kebab-case (`[a-z][a-z0-9-]*`), with no `_` alias.
///
/// # Examples
///
/// ```rust
/// use fundamental_core::CommandAddress;
///
/// let address = CommandAddress::parse("mint:operator-token")?;
/// assert_eq!(address.namespace(), "mint");
/// assert_eq!(address.name(), "operator-token");
/// assert_eq!(address.to_string(), "mint:operator-token");
///
/// // An underscore is not an address character.
/// assert!(CommandAddress::parse("mint:operator_token").is_err());
/// # Ok::<(), fundamental_core::CommandAddressObjection>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CommandAddress {
    namespace: String,
    name: String,
}

impl CommandAddress {
    /// The character that separates the two halves.
    pub const SEPARATOR: char = ':';

    /// Builds an address from its two halves.
    ///
    /// # Errors
    /// Returns the [`CommandAddressObjection`] for whichever half is not legal kebab-case.
    pub fn new(namespace: &str, name: &str) -> Result<Self, CommandAddressObjection> {
        Self::validate_segment(namespace)?;
        Self::validate_segment(name)?;
        Ok(Self {
            namespace: namespace.to_owned(),
            name: name.to_owned(),
        })
    }

    /// Parses `<namespace>:<name>`.
    ///
    /// # Errors
    /// Returns [`CommandAddressObjection::NotOneColon`] unless there is exactly one colon.
    pub fn parse(address: &str) -> Result<Self, CommandAddressObjection> {
        let mut halves = address.split(Self::SEPARATOR);
        let (Some(namespace), Some(name), None) = (halves.next(), halves.next(), halves.next())
        else {
            return Err(CommandAddressObjection::NotOneColon);
        };
        Self::new(namespace, name)
    }

    /// The namespace half.
    #[must_use]
    pub fn namespace(&self) -> &str {
        &self.namespace
    }

    /// The name half.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Checks one half of an address against the grammar.
    ///
    /// For sites that declare the halves separately, such as `#[command(namespace = …, name = …)]`.
    ///
    /// # Errors
    /// Returns the first [`CommandAddressObjection`] the value earns.
    pub fn validate_segment(value: &str) -> Result<(), CommandAddressObjection> {
        if value.is_empty() {
            return Err(CommandAddressObjection::Empty);
        }
        if !value.starts_with(|c: char| c.is_ascii_lowercase()) {
            return Err(CommandAddressObjection::BadFirstCharacter);
        }
        if value.ends_with('-') {
            return Err(CommandAddressObjection::TrailingHyphen);
        }
        if value.contains("--") {
            return Err(CommandAddressObjection::DoubleHyphen);
        }
        if let Some(bad) = value
            .chars()
            .find(|c| !(c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '-'))
        {
            return Err(CommandAddressObjection::IllegalCharacter(bad));
        }
        Ok(())
    }
}

impl fmt::Display for CommandAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}{}", self.namespace, Self::SEPARATOR, self.name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_ordinary_address_round_trips() {
        let address = CommandAddress::parse("mint:operator-token").expect("legal");
        assert_eq!(address.namespace(), "mint");
        assert_eq!(address.name(), "operator-token");
        assert_eq!(address.to_string(), "mint:operator-token");
    }

    #[test]
    fn digits_and_inner_hyphens_are_legal() {
        for legal in ["e2e", "layer0", "eu-lotl", "a", "a1", "a-b-c"] {
            CommandAddress::validate_segment(legal)
                .unwrap_or_else(|err| panic!("`{legal}` should be legal: {err}"));
        }
    }

    #[test]
    fn each_rule_has_its_own_objection() {
        assert_eq!(
            CommandAddress::validate_segment(""),
            Err(CommandAddressObjection::Empty)
        );
        assert_eq!(
            CommandAddress::validate_segment("Mint"),
            Err(CommandAddressObjection::BadFirstCharacter)
        );
        assert_eq!(
            CommandAddress::validate_segment("1mint"),
            Err(CommandAddressObjection::BadFirstCharacter)
        );
        assert_eq!(
            CommandAddress::validate_segment("mint-"),
            Err(CommandAddressObjection::TrailingHyphen)
        );
        assert_eq!(
            CommandAddress::validate_segment("mint--keys"),
            Err(CommandAddressObjection::DoubleHyphen)
        );
        assert_eq!(
            CommandAddress::validate_segment("mint_keys"),
            Err(CommandAddressObjection::IllegalCharacter('_'))
        );
        assert_eq!(
            CommandAddress::validate_segment("mint keys"),
            Err(CommandAddressObjection::IllegalCharacter(' '))
        );
    }

    #[test]
    fn an_address_needs_exactly_one_colon() {
        for bad in ["mint", "mint:keys:extra", "", ":"] {
            let err = CommandAddress::parse(bad).expect_err("not an address");
            // `:` splits into two empty halves, so it objects to the half, not the count.
            assert!(
                matches!(
                    err,
                    CommandAddressObjection::NotOneColon | CommandAddressObjection::Empty
                ),
                "`{bad}` gave {err:?}"
            );
        }
    }

    #[test]
    fn the_namespace_is_objected_to_before_the_name() {
        assert_eq!(
            CommandAddress::new("Mint", "operator_token"),
            Err(CommandAddressObjection::BadFirstCharacter)
        );
    }
}
