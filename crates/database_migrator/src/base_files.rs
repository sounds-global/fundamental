//! [`upgrade`] — which migrations a database at the base revision already ran, and what a branch
//! may not have done to them.

use std::collections::BTreeMap;

use fundamental_db::migration::{MigrationError, MigrationResult, content_hash};
use tracing::warn;

/// The migrations of one directory: a set's, or the app's own.
#[derive(Debug, Clone)]
pub(crate) struct Directory {
    /// What its files' ledger names start with: `<owner>/`, or nothing for the app's own.
    pub(crate) prefix: String,
    /// Each file on the branch, `(name, text)`, in the order it applies.
    pub(crate) files: Vec<(String, String)>,
    /// Each file at the base revision by name, or `None` when the directory is outside the
    /// project's repository, so every file in it counts as already at the base.
    pub(crate) base: Option<BTreeMap<String, String>>,
}

/// The real upgrade: what a database at the base revision already ran, then what the branch
/// adds, each `(ledger name, text)` in the order it applies.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Upgrade {
    /// The files the base revision holds, as it holds them.
    pub(crate) at_base: Vec<(String, String)>,
    /// The files the branch adds.
    pub(crate) new: Vec<(String, String)>,
}

/// The upgrade from `base` to the branch over `directories`, in their order.
///
/// # Errors
/// What the branch did to a file a database at the base may have run, each kind naming every
/// file of that kind at once, in this order of precedence:
/// [`MigrationError::AppliedFileChanged`], [`MigrationError::BaseFileGone`] and
/// [`MigrationError::FileBeforeBase`]. Every one found is logged.
pub(crate) fn upgrade(base: &str, directories: &[Directory]) -> MigrationResult<Upgrade> {
    let mut upgrade = Upgrade::default();
    let (mut changed, mut gone, mut before) = (Vec::new(), Vec::new(), Vec::new());
    for directory in directories {
        let ledger = |name: &str| format!("{}{name}", directory.prefix);
        let Some(at_base) = &directory.base else {
            upgrade.at_base.extend(
                directory
                    .files
                    .iter()
                    .map(|(name, text)| (ledger(name), text.clone())),
            );
            continue;
        };
        let last = at_base.keys().next_back();
        for (name, text) in &directory.files {
            match at_base.get(name) {
                Some(was) if content_hash(was) != content_hash(text) => changed.push(ledger(name)),
                Some(was) => upgrade.at_base.push((ledger(name), was.clone())),
                None => {
                    before.extend(
                        last.filter(|last| last.as_str() > name.as_str())
                            .map(|last| format!("{} before {}", ledger(name), ledger(last))),
                    );
                    upgrade.new.push((ledger(name), text.clone()));
                }
            }
        }
        gone.extend(
            at_base
                .keys()
                .filter(|name| !directory.files.iter().any(|(kept, _)| kept == *name))
                .map(|name| ledger(name)),
        );
    }
    for (kind, names) in [
        ("changed since the base", &changed),
        ("gone since the base", &gone),
        ("numbered before a file at the base", &before),
    ] {
        for name in names {
            warn!(base, migration = %name, kind, "a migration the base holds was not kept");
        }
    }
    let base = base.to_owned();
    if !changed.is_empty() {
        return Err(MigrationError::AppliedFileChanged {
            names: changed,
            base,
        });
    }
    if !gone.is_empty() {
        return Err(MigrationError::BaseFileGone { names: gone, base });
    }
    if !before.is_empty() {
        return Err(MigrationError::FileBeforeBase {
            names: before,
            base,
        });
    }
    Ok(upgrade)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use fundamental_db::migration::MigrationError;

    use super::{Directory, upgrade};

    fn files(names: &[&str]) -> Vec<(String, String)> {
        names
            .iter()
            .map(|name| ((*name).to_owned(), format!("-- {name}\n")))
            .collect()
    }

    fn newsroom(branch: &[&str], base: Option<&[&str]>) -> Directory {
        Directory {
            prefix: "newsroom/".to_owned(),
            files: files(branch),
            base: base.map(|names| files(names).into_iter().collect::<BTreeMap<_, _>>()),
        }
    }

    fn names(files: &[(String, String)]) -> Vec<&str> {
        files.iter().map(|(name, _)| name.as_str()).collect()
    }

    #[test]
    fn the_files_at_the_base_apply_first_then_the_branch_s_in_order() {
        let portfolio = Directory {
            prefix: "portfolio/".to_owned(),
            ..newsroom(&["0001_a"], Some(&["0001_a"]))
        };
        let plan = upgrade(
            "main",
            &[
                newsroom(&["0001_a", "0002_b", "0003_c"], Some(&["0001_a", "0002_b"])),
                portfolio,
            ],
        )
        .expect("nothing at the base changed");
        assert_eq!(
            names(&plan.at_base),
            ["newsroom/0001_a", "newsroom/0002_b", "portfolio/0001_a"]
        );
        assert_eq!(names(&plan.new), ["newsroom/0003_c"]);
    }

    #[test]
    fn a_directory_outside_the_repository_is_all_at_the_base() {
        let plan = upgrade("main", &[newsroom(&["0001_a", "0002_b"], None)]).expect("at base");
        assert_eq!(names(&plan.at_base), ["newsroom/0001_a", "newsroom/0002_b"]);
        assert!(plan.new.is_empty());
    }

    #[test]
    fn a_file_whose_text_changed_since_the_base_is_refused() {
        let mut directory = newsroom(&["0001_a", "0002_b"], Some(&["0001_a", "0002_b"]));
        directory.files[1]
            .1
            .push_str("ALTER TABLE a ADD COLUMN b TEXT;\n");
        let error = upgrade("3953fc6", &[directory]).expect_err("an applied file changed");
        assert!(
            matches!(&error, MigrationError::AppliedFileChanged { names, base }
                if names == &["newsroom/0002_b"] && base == "3953fc6"),
            "{error:?}"
        );
    }

    #[test]
    fn a_new_file_numbered_before_one_at_the_base_is_refused() {
        let error = upgrade(
            "main",
            &[newsroom(
                &["0001_a", "0002_new", "0003_c"],
                Some(&["0001_a", "0003_c"]),
            )],
        )
        .expect_err("numbering is linear");
        assert!(
            matches!(&error, MigrationError::FileBeforeBase { names, .. }
                if names == &["newsroom/0002_new before newsroom/0003_c"]),
            "{error:?}"
        );
    }

    #[test]
    fn a_file_the_base_holds_and_the_branch_removed_is_refused() {
        let error = upgrade(
            "main",
            &[newsroom(&["0001_a"], Some(&["0001_a", "0002_b"]))],
        )
        .expect_err("an applied file is never removed");
        assert!(
            matches!(&error, MigrationError::BaseFileGone { names, .. }
                if names == &["newsroom/0002_b"]),
            "{error:?}"
        );
    }

    #[test]
    fn a_reworded_comment_is_no_change_and_the_base_s_text_applies() {
        let mut directory = newsroom(&["0001_a"], Some(&["0001_a"]));
        directory.files[0].1 = "-- reworded\n".to_owned();
        let plan = upgrade("main", &[directory]).expect("a comment is not a statement");
        assert_eq!(plan.at_base[0].1, "-- 0001_a\n");
    }

    #[test]
    fn every_changed_file_is_named_at_once() {
        let mut newsroom = newsroom(&["0001_a", "0002_b"], Some(&["0001_a", "0002_b"]));
        for (_, text) in &mut newsroom.files {
            text.push_str("SELECT 1;\n");
        }
        let error = upgrade("main", &[newsroom]).expect_err("both changed");
        assert!(
            matches!(&error, MigrationError::AppliedFileChanged { names, .. } if names.len() == 2),
            "{error:?}"
        );
    }
}
