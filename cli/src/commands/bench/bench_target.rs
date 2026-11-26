//! [`BenchTarget`]: a workspace package's bench target of a layer's name.

use serde_json::Value;

/// A bench target named for a layer in one workspace package, and its required features.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BenchTarget {
    pub(crate) package: String,
    pub(crate) features: Vec<String>,
}

impl BenchTarget {
    /// Every workspace member's bench target named `name`, from `cargo metadata`.
    pub(crate) fn all_named(metadata: &Value, name: &str) -> Vec<Self> {
        let members: Vec<&str> = metadata["workspace_members"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect();
        metadata["packages"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|package| {
                package["id"]
                    .as_str()
                    .is_some_and(|id| members.contains(&id))
            })
            .filter_map(|package| {
                let target = package["targets"].as_array()?.iter().find(|target| {
                    target["name"] == name
                        && target["kind"]
                            .as_array()
                            .is_some_and(|kinds| kinds.iter().any(|kind| kind == "bench"))
                })?;
                Some(Self {
                    package: package["name"].as_str()?.to_owned(),
                    features: target["required-features"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect(),
                })
            })
            .collect()
    }

    /// The `cargo bench` arguments before `--`: its package, its features and the target.
    pub(crate) fn cargo_args(&self, name: &str) -> Vec<String> {
        let mut args = vec!["-p".to_owned(), self.package.clone()];
        if !self.features.is_empty() {
            args.push("--features".to_owned());
            args.push(self.features.join(","));
        }
        args.push("--bench".to_owned());
        args.push(name.to_owned());
        args
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn metadata() -> Value {
        json!({
            "workspace_members": ["app 0.1.0 (path+file:///w/app)", "bench 0.1.0 (path+file:///w/bench)"],
            "packages": [
                { "id": "app 0.1.0 (path+file:///w/app)", "name": "app",
                  "targets": [{ "name": "app", "kind": ["lib"] }] },
                { "id": "bench 0.1.0 (path+file:///w/bench)", "name": "bench",
                  "targets": [
                    { "name": "instructions", "kind": ["bench"] },
                    { "name": "load", "kind": ["bench"], "required-features": ["live"] }
                  ] },
                { "id": "dep 1.0.0 (registry+https://x)", "name": "dep",
                  "targets": [{ "name": "load", "kind": ["bench"] }] }
            ]
        })
    }

    #[test]
    fn a_members_bench_of_the_name_is_found_with_its_features() {
        assert_eq!(
            BenchTarget::all_named(&metadata(), "load"),
            [BenchTarget {
                package: "bench".to_owned(),
                features: vec!["live".to_owned()],
            }]
        );
    }

    #[test]
    fn a_name_no_member_benches_finds_nothing() {
        assert!(BenchTarget::all_named(&metadata(), "soak").is_empty());
    }

    #[test]
    fn the_cargo_arguments_name_the_package_its_features_and_the_target() {
        let target = BenchTarget {
            package: "bench".to_owned(),
            features: vec!["live".to_owned()],
        };
        assert_eq!(
            target.cargo_args("load"),
            ["-p", "bench", "--features", "live", "--bench", "load"]
        );
    }
}
