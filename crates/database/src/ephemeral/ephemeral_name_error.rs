//! [`EphemeralNameError`] — why a store name could not be minted.

use std::borrow::Cow;

use fundamental_macros::Error;

/// Why [`EphemeralName::mint`](super::EphemeralName::mint) refused.
///
/// Every variant is a fixture's programming error, so all are `500`.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum EphemeralNameError {
    /// The prefix is not a plain unquoted identifier.
    #[error(
        code = "EPHEMERAL_NAME_001",
        message = "Something went wrong while preparing a test fixture.",
        developer_message = "`{prefix}` is not a plain identifier, and a store name reaches \
                             the engine as interpolated DDL",
        status = 500
    )]
    InvalidPrefix {
        /// The prefix as it was offered.
        prefix: String,
    },

    /// The prefix does not end with the separator the grammar requires.
    ///
    /// Without it the sweep could never recognise the store as reclaimable.
    #[error(
        code = "EPHEMERAL_NAME_002",
        message = "Something went wrong while preparing a test fixture.",
        developer_message = "the prefix `{prefix}` must end with `_`, or the name it \
                             produces cannot be swept",
        status = 500
    )]
    PrefixNotSeparated {
        /// The prefix as it was offered.
        prefix: String,
    },

    /// The minted name is longer than the engine allows.
    #[error(
        code = "EPHEMERAL_NAME_003",
        message = "Something went wrong while preparing a test fixture.",
        developer_message = "`{prefix}` mints a {actual}-character name and the limit is \
                             {max_len}",
        status = 500
    )]
    TooLong {
        /// The prefix as it was offered.
        prefix: String,
        /// The length the minted name would have had.
        actual: usize,
        /// The engine's limit — 63 for a SQL database, 48 for a CQL keyspace.
        max_len: usize,
    },
}

impl EphemeralNameError {
    /// The prefix that was refused, whichever rule it broke.
    #[must_use]
    pub fn prefix(&self) -> &str {
        match self {
            Self::InvalidPrefix { prefix }
            | Self::PrefixNotSeparated { prefix }
            | Self::TooLong { prefix, .. } => prefix,
        }
    }

    /// Which rule was broken, for whoever reads a fixture failure.
    ///
    /// The developer message, never `Display`, which is the user-facing half.
    #[must_use]
    pub fn detail(&self) -> String {
        fundamental_core::Error::developer_message(self)
            .map_or_else(|| self.to_string(), Cow::into_owned)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_refusal_names_its_prefix_and_its_rule() {
        let refusals = [
            EphemeralNameError::InvalidPrefix {
                prefix: "x;".to_owned(),
            },
            EphemeralNameError::PrefixNotSeparated {
                prefix: "s".to_owned(),
            },
            EphemeralNameError::TooLong {
                prefix: "long_".to_owned(),
                actual: 70,
                max_len: 63,
            },
        ];
        for (refusal, (prefix, rule)) in refusals.iter().zip([
            ("x;", "not a plain identifier"),
            ("s", "must end with `_`"),
            ("long_", "70-character name and the limit is 63"),
        ]) {
            assert_eq!(refusal.prefix(), prefix);
            assert!(refusal.detail().contains(rule), "{}", refusal.detail());
        }
    }
}
