//! [`ManifestCommand`]: one entry of the `[commands]` table in `fun.toml`.

/// A shell command declared in a `[commands.<namespace>]` table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ManifestCommand {
    pub namespace: String,
    pub name: String,
    pub run: String,
    /// The one-line description shown by `fun run --list`, if declared.
    pub description: Option<String>,
}

impl ManifestCommand {
    /// The command's address, `<namespace>:<name>`.
    pub(crate) fn address(&self) -> String {
        format!("{}:{}", self.namespace, self.name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_address_joins_the_namespace_and_the_name() {
        let command = ManifestCommand {
            namespace: "gate".to_string(),
            name: "docs".to_string(),
            run: "scripts/docs-check.sh".to_string(),
            description: None,
        };
        assert_eq!(command.address(), "gate:docs");
    }
}
