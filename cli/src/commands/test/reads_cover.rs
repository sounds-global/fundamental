//! [`reads_cover`]: whether a package's `reads` glob reaches a path.

/// Whether `glob` covers `path`; beyond `**` and a trailing `*`, it errs toward running tests.
#[must_use]
pub(crate) fn reads_cover(glob: &str, path: &str) -> bool {
    let prefix = literal_prefix(glob);
    let Some(rest) = path.strip_prefix(prefix) else {
        return false;
    };
    // `crates/web` is a textual prefix of `crates/web_ui`, so a prefix covers only at a boundary.
    rest.is_empty() || prefix.ends_with('/') || rest.starts_with('/') || glob.len() > prefix.len()
}

fn literal_prefix(glob: &str) -> &str {
    glob.split(['*', '?', '[']).next().unwrap_or(glob)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_directory_covers_what_is_under_it() {
        assert!(reads_cover("domain/**", "domain/layer3/model.rs"));
        assert!(reads_cover("scripts/**", "scripts/check.sh"));
    }

    #[test]
    fn coverage_runs_one_way_only() {
        assert!(!reads_cover("domain/layer3/**", "domain/**"));
        assert!(!reads_cover("scripts/check.sh", "scripts/**"));
    }

    #[test]
    fn a_textual_prefix_that_is_not_a_path_boundary_is_not_coverage() {
        assert!(!reads_cover("crates/web/**", "crates/web_ui/src/lib.rs"));
    }

    #[test]
    fn a_wildcard_reaches_past_the_segment_it_stops_in() {
        assert!(reads_cover("scripts/check-*.sh", "scripts/check-docs.sh"));
    }

    #[test]
    fn unrelated_trees_do_not_cover_each_other() {
        assert!(!reads_cover("domain/**", "apps/www/src/main.rs"));
    }
}
