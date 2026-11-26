//! [`CommandListing`], one row of a runner's registry as `fun run --list` reads it.

/// One registered command, flattened for listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandListing {
    /// `<namespace>:<name>` — the dispatch key.
    pub address: String,
    /// The command's namespace.
    pub namespace: &'static str,
    /// The command's name within its namespace.
    pub name: &'static str,
    /// The one-line help the command declared, if any.
    pub description: Option<&'static str>,
}

impl CommandListing {
    /// The tab-separated line `commands_main!`'s `--list` prints, and the CLI parses.
    #[must_use]
    pub fn to_line(&self) -> String {
        format!(
            "{}\t{}\t{}\t{}",
            self.address,
            self.namespace,
            self.name,
            self.description.unwrap_or_default()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listing(description: Option<&'static str>) -> CommandListing {
        CommandListing {
            address: "mint:operator-token".to_string(),
            namespace: "mint",
            name: "operator-token",
            description,
        }
    }

    #[test]
    fn a_line_carries_all_four_fields() {
        assert_eq!(
            listing(Some("Mint a bearer")).to_line(),
            "mint:operator-token\tmint\toperator-token\tMint a bearer"
        );
    }

    #[test]
    fn a_missing_description_still_leaves_four_fields() {
        let line = listing(None).to_line();
        assert_eq!(line.split('\t').count(), 4);
        assert!(line.ends_with('\t'), "{line}");
    }
}
