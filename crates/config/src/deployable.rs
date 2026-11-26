//! [`Deployable`]: every process a manifest describes, and the code each one runs.

use crate::loader::INSTANCE_OF;

/// One process a manifest describes: an app, or an instance of one.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Deployable {
    /// The `[apps.<name>]` section and the service name.
    pub name: String,
    /// The app whose code it runs: `name` itself, or what its `instance_of` declares.
    pub base: String,
}

impl Deployable {
    /// Whether this deployable is an instance of another app.
    #[must_use]
    pub fn is_instance(&self) -> bool {
        self.name != self.base
    }

    /// Every `[apps.*]` section of a parsed manifest, in name order, read as declared.
    #[must_use]
    pub fn all_in(manifest: &toml::Value) -> Vec<Self> {
        let mut deployables: Vec<Self> = manifest
            .get("apps")
            .and_then(toml::Value::as_table)
            .map(|apps| {
                apps.iter()
                    .map(|(name, section)| Self {
                        name: name.clone(),
                        base: section
                            .get(INSTANCE_OF)
                            .and_then(toml::Value::as_str)
                            .unwrap_or(name)
                            .to_owned(),
                    })
                    .collect()
            })
            .unwrap_or_default();
        deployables.sort();
        deployables
    }
}

#[cfg(test)]
mod tests {
    use super::Deployable;

    #[test]
    fn an_instance_names_its_base_and_an_app_names_itself() {
        let manifest: toml::Value = toml::from_str(
            r#"
            [apps.layer0.api]
            port = 1
            [apps.layer0_sandbox]
            instance_of = "layer0"
            [apps.www]
            admin = { port = 2 }
        "#,
        )
        .expect("a manifest");
        let all = Deployable::all_in(&manifest);
        assert_eq!(
            all,
            [
                Deployable {
                    name: "layer0".into(),
                    base: "layer0".into()
                },
                Deployable {
                    name: "layer0_sandbox".into(),
                    base: "layer0".into()
                },
                Deployable {
                    name: "www".into(),
                    base: "www".into()
                },
            ]
        );
        assert!(all[1].is_instance() && !all[0].is_instance());
    }

    #[test]
    fn a_manifest_with_no_apps_describes_nothing() {
        let manifest: toml::Value = toml::from_str("[custom]\nx = 1").expect("a manifest");
        assert!(Deployable::all_in(&manifest).is_empty());
    }
}
