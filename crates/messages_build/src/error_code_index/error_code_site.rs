use std::fmt;
use std::path::PathBuf;

/// Where one `#[derive(Error)]` variant declares its code.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ErrorCodeSite {
    /// The source file.
    pub file: PathBuf,
    /// The 1-based line of the variant.
    pub line: usize,
    /// The error enum.
    pub enum_name: String,
    /// The variant.
    pub variant: String,
}

impl fmt::Display for ErrorCodeSite {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}:{} ({}::{})",
            self.file.display(),
            self.line,
            self.enum_name,
            self.variant
        )
    }
}
