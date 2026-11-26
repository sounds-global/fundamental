//! [`CommandEntry`], one addressable command in the project's index.

use fundamental::CommandAddress;

use crate::helpers::CommandSource;

/// One thing `fun run` can run: a manifest script or a Rust `#[command]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CommandEntry {
    pub address: CommandAddress,
    /// The one-line help shown by `fun run --list`, if declared.
    pub description: Option<String>,
    pub source: CommandSource,
}

impl CommandEntry {
    pub(crate) fn address_string(&self) -> String {
        self.address.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_entry_prints_its_address() {
        let entry = CommandEntry {
            address: CommandAddress::parse("gate:docs").expect("legal"),
            description: Some("The docs drift check".to_string()),
            source: CommandSource::Script {
                run: "scripts/docs-check.sh".to_string(),
                declared_in: "fun.toml".to_string(),
            },
        };
        assert_eq!(entry.address_string(), "gate:docs");
    }
}
