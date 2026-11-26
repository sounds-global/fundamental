//! [`IsolationFinding`], one place a test reaches beyond its own rows.

use std::fmt;
use std::path::PathBuf;

use crate::IsolationRule;

/// One place a test running beside others reaches beyond its own rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IsolationFinding {
    /// The rule it breaks.
    pub rule: IsolationRule,
    /// The test source.
    pub file: PathBuf,
    /// The line the call starts on.
    pub line: usize,
    /// The function it sits in.
    pub function: String,
    /// What was found there.
    pub detail: String,
}

impl fmt::Display for IsolationFinding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}  {}:{} in `{}` — {}; {}",
            self.rule,
            self.file.display(),
            self.line,
            self.function,
            self.detail,
            self.rule.remedy()
        )
    }
}
