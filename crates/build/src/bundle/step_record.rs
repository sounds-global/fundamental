//! [`StepRecord`] — one bundle step's inputs *and* the outputs they produced.

use std::fmt::{self, Display};

use super::step_print::StepPrint;

/// What one step last saw, and what it left behind.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StepRecord {
    print: StepPrint,
    outputs: Vec<String>,
}

impl StepRecord {
    /// A record of `print` having produced `outputs`.
    #[must_use]
    pub fn new(print: StepPrint, outputs: Vec<String>) -> Self {
        Self { print, outputs }
    }

    /// The step's inputs.
    #[must_use]
    pub fn print(&self) -> &StepPrint {
        &self.print
    }

    /// What the step wrote, relative to the bundle directory.
    #[must_use]
    pub fn outputs(&self) -> &[String] {
        &self.outputs
    }
}

/// `<print>\toutputs=a;b;c`: a separate field, since a print's `salt` takes the rest of its segment.
impl Display for StepRecord {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}\toutputs={}",
            self.print,
            self.outputs.join(";")
        )
    }
}

impl StepRecord {
    /// Read a record back from its recorded line; a line without outputs parses to a record with none.
    #[must_use]
    pub fn parse(line: &str) -> Option<Self> {
        let (print, outputs) = match line.split_once('\t') {
            Some((print, rest)) => (print, rest.strip_prefix("outputs=").unwrap_or_default()),
            None => (line, ""),
        };
        Some(Self {
            print: StepPrint::parse(print)?,
            outputs: outputs
                .split(';')
                .filter(|path| !path.is_empty())
                .map(str::to_owned)
                .collect(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn print() -> StepPrint {
        StepPrint::of_paths(&[Path::new("/no/such")], "tailwindcss -o dist/app.css")
    }

    #[test]
    fn a_record_round_trips_through_its_line() {
        let record = StepRecord::new(print(), vec!["app.css".into(), "assets/a.woff2".into()]);
        let parsed = StepRecord::parse(&record.to_string()).expect("parsed");
        assert_eq!(parsed, record);
    }

    #[test]
    fn a_record_with_no_outputs_round_trips() {
        let record = StepRecord::new(print(), Vec::new());
        let parsed = StepRecord::parse(&record.to_string()).expect("parsed");
        assert_eq!(parsed.outputs(), &[] as &[String]);
    }

    #[test]
    fn a_line_written_before_outputs_existed_still_parses() {
        // An existing `dist/` without outputs must only rebuild, not error.
        let legacy = print().to_string();
        let parsed = StepRecord::parse(&legacy).expect("parsed");
        assert_eq!(parsed.print(), &print());
        assert!(parsed.outputs().is_empty());
    }

    #[test]
    fn a_salt_containing_a_space_survives_the_split() {
        let record = StepRecord::new(print(), vec!["app.css".into()]);
        let line = record.to_string();
        assert!(line.contains("salt=tailwindcss -o dist/app.css\toutputs=app.css"));
        assert_eq!(StepRecord::parse(&line).expect("parsed"), record);
    }

    #[test]
    fn an_unparseable_print_yields_nothing() {
        assert!(StepRecord::parse("not-a-print\toutputs=app.css").is_none());
    }
}
