//! Where in the manifest a value is written.

use std::fmt;
use std::path::{Path, PathBuf};

/// Where a value is written: a file, relative to the project, and a line and column, both
/// counted from one.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Origin {
    file: PathBuf,
    line: usize,
    column: usize,
}

impl Origin {
    pub(crate) fn new(file: &Path, line: usize, column: usize) -> Self {
        Self {
            file: file.to_path_buf(),
            line,
            column,
        }
    }

    /// The origin of byte `offset` in `text`, the contents of `file`.
    pub(crate) fn at_offset(file: &Path, text: &str, offset: usize) -> Self {
        let before = text.get(..offset.min(text.len())).unwrap_or(text);
        let line = before.matches('\n').count() + 1;
        let line_start = before.rfind('\n').map_or(0, |index| index + 1);
        let column = before
            .get(line_start..)
            .map_or(0, |rest| rest.chars().count())
            + 1;
        Self::new(file, line, column)
    }

    /// The file, relative to the project.
    #[must_use]
    pub fn file(&self) -> &Path {
        &self.file
    }

    /// The line, counted from one.
    #[must_use]
    pub fn line(&self) -> usize {
        self.line
    }

    /// The column, counted in characters from one.
    #[must_use]
    pub fn column(&self) -> usize {
        self.column
    }
}

impl fmt::Display for Origin {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}:{}:{}",
            self.file.display(),
            self.line,
            self.column
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_lines_and_columns_from_one() {
        let text = "[a]\nkey = 1\n  b = 2\n";
        let file = Path::new("f.toml");
        assert_eq!(Origin::at_offset(file, text, 0).to_string(), "f.toml:1:1");
        assert_eq!(Origin::at_offset(file, text, 4).to_string(), "f.toml:2:1");
        assert_eq!(Origin::at_offset(file, text, 14).to_string(), "f.toml:3:3");
    }

    #[test]
    fn an_offset_past_the_end_is_the_end() {
        let origin = Origin::at_offset(Path::new("f"), "ab", 99);
        assert_eq!((origin.line(), origin.column()), (1, 3));
    }
}
