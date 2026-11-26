//! What this binary answers when asked which build it is.

/// The full version with its commit: `0.1.0 (78d689c8, <date>)`, `+` when dirty, or `(unknown revision)`.
pub(crate) const VERSION: &str = concat!(
    env!("CARGO_PKG_VERSION"),
    " (",
    env!("FUNDAMENTAL_BUILD"),
    ")"
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_version_string_identifies_the_build_it_came_from() {
        assert!(
            VERSION.starts_with(env!("CARGO_PKG_VERSION")),
            "the package version stays first: {VERSION}"
        );
        assert!(
            VERSION.len() > env!("CARGO_PKG_VERSION").len(),
            "a bare package version identifies nothing: {VERSION}"
        );
        assert!(
            VERSION.contains('(') && VERSION.ends_with(')'),
            "the build is named in brackets after it: {VERSION}"
        );
    }

    #[test]
    fn a_build_that_cannot_name_its_commit_says_so_rather_than_inventing_one() {
        let build = VERSION
            .split_once('(')
            .and_then(|(_, rest)| rest.strip_suffix(')'))
            .unwrap_or_default();
        assert!(!build.is_empty(), "something must be said: {VERSION}");
        if build == "unknown revision" {
            return;
        }
        let (revision, date) = build
            .split_once(", ")
            .unwrap_or_else(|| panic!("a known build names a revision and a date: {build}"));
        assert!(
            revision
                .trim_end_matches('+')
                .chars()
                .all(|c| c.is_ascii_hexdigit()),
            "the revision is a commit, optionally marked dirty: {revision}"
        );
        assert_eq!(
            date.len(),
            10,
            "the date is ISO, as `%cs` writes it: {date}"
        );
    }
}
