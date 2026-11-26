//! [`CommandArgs`], the passthrough command-line arguments handed to a command.

use std::slice::Iter;

/// The arguments passed to a command after its name.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CommandArgs(Vec<String>);

impl CommandArgs {
    /// Collects the passthrough arguments into a `CommandArgs`.
    pub fn new(args: impl IntoIterator<Item = String>) -> Self {
        Self(args.into_iter().collect())
    }

    /// The arguments as a slice.
    #[must_use]
    pub fn as_slice(&self) -> &[String] {
        &self.0
    }

    /// The argument at `index`, or `None` if out of range.
    #[must_use]
    pub fn get(&self, index: usize) -> Option<&str> {
        self.0.get(index).map(String::as_str)
    }

    /// The first argument, or `None` if there are none.
    #[must_use]
    pub fn first(&self) -> Option<&str> {
        self.0.first().map(String::as_str)
    }

    /// The number of arguments.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether no arguments were passed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Borrows the arguments as an iterator of `&str`.
    pub fn iter(&self) -> impl Iterator<Item = &str> {
        self.0.iter().map(String::as_str)
    }

    /// The value following `flag`, accepting both `--flag value` and `--flag=value`.
    ///
    /// ```rust
    /// # extern crate fundamental_command;
    /// use fundamental_command::CommandArgs;
    ///
    /// let args = CommandArgs::new(["--name".to_string(), "witness.a".to_string()]);
    /// assert_eq!(args.flag_value("--name"), Some("witness.a"));
    ///
    /// let joined = CommandArgs::new(["--name=witness.a".to_string()]);
    /// assert_eq!(joined.flag_value("--name"), Some("witness.a"));
    ///
    /// assert_eq!(joined.flag_value("--missing"), None);
    /// ```
    #[must_use]
    pub fn flag_value(&self, flag: &str) -> Option<&str> {
        for (index, arg) in self.0.iter().enumerate() {
            if arg == flag {
                return self.get(index + 1);
            }
            if let Some(value) = arg
                .strip_prefix(flag)
                .and_then(|rest| rest.strip_prefix('='))
            {
                return Some(value);
            }
        }
        None
    }

    /// Whether a bare `flag` (for example `--stdout`) was passed.
    #[must_use]
    pub fn has_flag(&self, flag: &str) -> bool {
        self.0.iter().any(|arg| arg == flag)
    }

    /// Consumes the wrapper, returning the owned argument vector.
    #[must_use]
    pub fn into_vec(self) -> Vec<String> {
        self.0
    }
}

impl From<Vec<String>> for CommandArgs {
    fn from(args: Vec<String>) -> Self {
        Self(args)
    }
}

impl FromIterator<String> for CommandArgs {
    fn from_iter<I: IntoIterator<Item = String>>(iter: I) -> Self {
        Self(iter.into_iter().collect())
    }
}

impl<'a> IntoIterator for &'a CommandArgs {
    type Item = &'a String;
    type IntoIter = Iter<'a, String>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

impl IntoIterator for CommandArgs {
    type Item = String;
    type IntoIter = std::vec::IntoIter<String>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_args_report_zero_length() {
        let args = CommandArgs::default();
        assert!(args.is_empty());
        assert_eq!(args.len(), 0);
        assert_eq!(args.first(), None);
        assert_eq!(args.get(0), None);
    }

    #[test]
    fn populated_args_index_and_iterate() {
        let args = CommandArgs::new(["--since".to_string(), "2020".to_string()]);
        assert!(!args.is_empty());
        assert_eq!(args.len(), 2);
        assert_eq!(args.first(), Some("--since"));
        assert_eq!(args.get(1), Some("2020"));
        assert_eq!(args.get(2), None);
        assert_eq!(args.as_slice(), ["--since", "2020"]);
        let collected: Vec<&str> = args.iter().collect();
        assert_eq!(collected, ["--since", "2020"]);
    }

    #[test]
    fn from_vec_and_into_vec_round_trip() {
        let source = vec!["a".to_string(), "b".to_string()];
        let args = CommandArgs::from(source.clone());
        assert_eq!(args.clone().into_vec(), source);
        let owned: Vec<String> = args.into_iter().collect();
        assert_eq!(owned, source);
    }

    #[test]
    fn collects_from_iterator() {
        let args: CommandArgs = ["x".to_string(), "y".to_string()].into_iter().collect();
        assert_eq!(args.len(), 2);
        let borrowed: Vec<&String> = (&args).into_iter().collect();
        assert_eq!(borrowed, [&"x".to_string(), &"y".to_string()]);
    }
}
