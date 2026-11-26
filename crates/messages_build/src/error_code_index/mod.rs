//! [`ErrorCodeIndex`]: every error code declared under some source roots, and where.

mod error_code_site;
mod scan;

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use syn::Item;
use tracing::{info, instrument, warn};

pub use error_code_site::ErrorCodeSite;
use scan::{collect_rust_files_excluding, derives, error_code};

/// Left out: a fixture may repeat a production code on purpose.
const NOT_SHIPPED: &[&str] = &["tests", "benches", "examples"];

/// Every error code declared under some source roots, with every site that declares it; read
/// from source, so it covers code no host build compiles.
#[derive(Debug, Default)]
pub struct ErrorCodeIndex {
    sites: BTreeMap<String, Vec<ErrorCodeSite>>,
    unreadable: Vec<PathBuf>,
}

impl ErrorCodeIndex {
    /// Scan every `.rs` file under each root.
    ///
    /// # Errors
    ///
    /// Any I/O error reading a root or a file; a missing root is an error, never an empty index.
    #[instrument(skip_all, fields(roots = roots.len()))]
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    pub fn scan(roots: &[&Path]) -> io::Result<Self> {
        let mut index = Self::default();
        for root in roots {
            let mut files = Vec::new();
            collect_rust_files_excluding(root, NOT_SHIPPED, &mut files)?;
            for file in files {
                let content = fs::read_to_string(&file)?;
                match syn::parse_file(&content) {
                    Ok(ast) => index.visit(&file, &ast.items),
                    Err(error) => {
                        warn!(file = %file.display(), %error, "an unparseable file's error codes are unseen");
                        index.unreadable.push(file);
                    }
                }
            }
        }
        info!(
            codes = index.sites.len(),
            unreadable = index.unreadable.len(),
            "error codes indexed"
        );
        Ok(index)
    }

    /// Every code declared at more than one site, with the sites.
    #[must_use]
    pub fn duplicates(&self) -> Vec<(&str, &[ErrorCodeSite])> {
        self.sites
            .iter()
            .filter(|(_, sites)| sites.len() > 1)
            .map(|(code, sites)| (code.as_str(), sites.as_slice()))
            .collect()
    }

    /// Every code found, in code order, with the sites declaring it.
    pub fn codes(&self) -> impl ExactSizeIterator<Item = (&str, &[ErrorCodeSite])> {
        self.sites
            .iter()
            .map(|(code, sites)| (code.as_str(), sites.as_slice()))
    }

    /// The sites declaring `code`.
    #[must_use]
    pub fn sites_of(&self, code: &str) -> &[ErrorCodeSite] {
        self.sites.get(code).map_or(&[], Vec::as_slice)
    }

    /// How many distinct codes were found.
    #[must_use]
    pub fn len(&self) -> usize {
        self.sites.len()
    }

    /// Whether no code was found at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.sites.is_empty()
    }

    /// The files `syn` could not parse, whose codes the index therefore does not hold.
    #[must_use]
    pub fn unreadable(&self) -> &[PathBuf] {
        &self.unreadable
    }

    #[expect(clippy::excessive_nesting, reason = "readability baseline")]
    fn visit(&mut self, file: &Path, items: &[Item]) {
        for item in items {
            match item {
                Item::Enum(item_enum) if derives(&item_enum.attrs, "Error") => {
                    for variant in &item_enum.variants {
                        let Some(code) = error_code(&variant.attrs) else {
                            continue;
                        };
                        if code == "UNKNOWN" {
                            continue;
                        }
                        self.sites.entry(code).or_default().push(ErrorCodeSite {
                            file: file.to_path_buf(),
                            line: variant.ident.span().start().line,
                            enum_name: item_enum.ident.to_string(),
                            variant: variant.ident.to_string(),
                        });
                    }
                }
                Item::Mod(item_mod) if !is_test_only(&item_mod.attrs) => {
                    if let Some((_, sub_items)) = &item_mod.content {
                        self.visit(file, sub_items);
                    }
                }
                _ => {}
            }
        }
    }
}

/// `#[cfg(test)]` on a module: its errors never reach a caller.
fn is_test_only(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        attr.path().is_ident("cfg")
            && attr
                .parse_args::<syn::Ident>()
                .is_ok_and(|predicate| predicate == "test")
    })
}

#[cfg(test)]
mod tests {
    use super::ErrorCodeIndex;
    use std::fs;

    fn write(dir: &std::path::Path, name: &str, body: &str) {
        fs::write(dir.join(name), body).expect("a temporary file");
    }

    #[test]
    fn a_code_declared_in_two_enums_names_both_sites() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        write(
            dir.path(),
            "first.rs",
            "#[derive(Debug, Error)]\nenum First {\n    #[error(code = \"DUP_001\", message = \"a\", status = 403)]\n    Denied,\n}\n",
        );
        write(
            dir.path(),
            "second.rs",
            "#[derive(Debug, Error)]\nenum Second {\n    #[error(code = \"DUP_001\", message = \"b\", status = 500)]\n    Broken,\n    #[error(code = \"DUP_002\", message = \"c\", status = 500)]\n    Other,\n}\n",
        );
        let index = ErrorCodeIndex::scan(&[dir.path()]).expect("a readable directory");
        let duplicates = index.duplicates();
        assert_eq!(duplicates.len(), 1);
        let (code, sites) = duplicates[0];
        assert_eq!(code, "DUP_001");
        let mut named: Vec<String> = sites
            .iter()
            .map(|site| format!("{}::{} line {}", site.enum_name, site.variant, site.line))
            .collect();
        named.sort();
        assert_eq!(named, ["First::Denied line 4", "Second::Broken line 4"]);
        assert_eq!(index.sites_of("DUP_002").len(), 1);
    }

    #[test]
    fn every_code_is_listed_once_in_code_order_with_its_sites() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        write(
            dir.path(),
            "listed.rs",
            "#[derive(Debug, Error)]\nenum Listed {\n    #[error(code = \"LST_002\", message = \"b\", status = 500)]\n    Second,\n    #[error(code = \"LST_001\", message = \"a\", status = 500)]\n    First,\n}\n#[derive(Debug, Error)]\nenum Again {\n    #[error(code = \"LST_001\", message = \"c\", status = 500)]\n    Repeated,\n}\n",
        );
        let index = ErrorCodeIndex::scan(&[dir.path()]).expect("a readable directory");
        let listed: Vec<(&str, usize)> = index
            .codes()
            .map(|(code, sites)| (code, sites.len()))
            .collect();
        assert_eq!(listed, [("LST_001", 2), ("LST_002", 1)]);
        assert_eq!(index.codes().len(), index.len());
    }

    #[test]
    fn a_path_qualified_derive_is_indexed() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        write(
            dir.path(),
            "qualified.rs",
            "#[derive(Debug, fundamental_macros::Error)]\nenum Qualified {\n    #[error(code = \"QUA_001\", message = \"q\", status = 500)]\n    Seen,\n}\n",
        );
        let index = ErrorCodeIndex::scan(&[dir.path()]).expect("a readable directory");
        assert_eq!(index.sites_of("QUA_001").len(), 1);
    }

    #[test]
    fn test_code_and_transparent_variants_are_not_indexed() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        fs::create_dir(dir.path().join("tests")).expect("a tests directory");
        write(
            &dir.path().join("tests"),
            "fixture.rs",
            "#[derive(Debug, Error)]\nenum Fixture {\n    #[error(code = \"DUP_001\", message = \"a\", status = 403)]\n    Denied,\n}\n",
        );
        write(
            dir.path(),
            "lib.rs",
            "#[derive(Debug, Error)]\nenum Real {\n    #[error(code = \"DUP_001\", message = \"a\", status = 403)]\n    Denied,\n    #[error(transparent)]\n    Inner(Other),\n}\n#[cfg(test)]\nmod tests {\n    #[derive(Debug, Error)]\n    enum Probe {\n        #[error(code = \"DUP_001\", message = \"p\", status = 500)]\n        Probe,\n    }\n}\n",
        );
        let index = ErrorCodeIndex::scan(&[dir.path()]).expect("a readable directory");
        assert!(index.duplicates().is_empty(), "{:?}", index.duplicates());
        assert_eq!(index.len(), 1);
    }

    #[test]
    fn a_missing_root_is_an_error_not_an_empty_index() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        assert!(ErrorCodeIndex::scan(&[&dir.path().join("absent")]).is_err());
    }

    #[test]
    fn an_unparseable_file_is_named() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        write(dir.path(), "broken.rs", "enum {");
        let index = ErrorCodeIndex::scan(&[dir.path()]).expect("a readable directory");
        assert_eq!(index.unreadable().len(), 1);
    }
}
