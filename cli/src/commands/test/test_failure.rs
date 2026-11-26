//! [`TestFailure`]: one failed test, as nextest's JUnit report records it.

use roxmltree::{Document, Node};

/// Where a test fails is at the end of its output.
const SHOWN_LINES: usize = 20;

/// One failed test, as nextest's JUnit report records it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TestFailure {
    /// As nextest names it: `mijn::it signing_in::one_address`.
    pub(crate) name: String,
    /// The end of what the test said when it failed.
    pub(crate) said: String,
}

impl TestFailure {
    /// Every failed test in a JUnit report, with its failure's text or else its captured output.
    ///
    /// # Errors
    /// The parser's reason when the report is not XML.
    pub(crate) fn all_in(report: &str) -> Result<Vec<Self>, String> {
        let document = Document::parse(report).map_err(|error| error.to_string())?;
        Ok(document
            .descendants()
            .filter(|node| node.has_tag_name("testcase"))
            .filter_map(|case| {
                let failed = case
                    .children()
                    .find(|child| child.has_tag_name("failure") || child.has_tag_name("error"))?;
                let suite = case
                    .parent()
                    .and_then(|suite| suite.attribute("name"))
                    .unwrap_or_default();
                let test = case.attribute("name").unwrap_or_default();
                Some(Self {
                    name: format!("{suite} {test}").trim().to_owned(),
                    said: said(case, failed),
                })
            })
            .collect())
    }
}

fn said(case: Node<'_, '_>, failed: Node<'_, '_>) -> String {
    let own = [
        failed.attribute("message").unwrap_or_default(),
        failed.text().unwrap_or_default(),
    ]
    .join("\n");
    let text = if own.trim().is_empty() {
        case.children()
            .filter(|child| child.has_tag_name("system-out") || child.has_tag_name("system-err"))
            .filter_map(|child| child.text())
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        own
    };
    let lines: Vec<&str> = text.trim().lines().collect();
    lines[lines.len().saturating_sub(SHOWN_LINES)..].join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    const REPORT: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<testsuites name="nextest-run" tests="3" failures="1" errors="1">
  <testsuite name="mijn::it" tests="3" failures="1" errors="1">
    <testcase name="signing_in::passes" classname="mijn::it" time="0.1"/>
    <testcase name="signing_in::one_address" classname="mijn::it" time="0.2">
      <failure type="test failure">thread panicked at it.rs:3:5:
two accounts for one address</failure>
      <system-out>running 1 test</system-out>
    </testcase>
    <testcase name="billing::aborts" classname="mijn::it" time="0.3">
      <error type="SIGABRT"/>
      <system-err>fatal runtime error: stack overflow</system-err>
    </testcase>
  </testsuite>
</testsuites>"#;

    #[test]
    fn each_failed_test_is_named_with_what_it_said() {
        let failures = TestFailure::all_in(REPORT).expect("a report");
        assert_eq!(failures.len(), 2, "{failures:?}");
        assert_eq!(failures[0].name, "mijn::it signing_in::one_address");
        assert!(
            failures[0].said.ends_with("two accounts for one address"),
            "{failures:?}"
        );
        assert_eq!(failures[1].name, "mijn::it billing::aborts");
        assert_eq!(failures[1].said, "fatal runtime error: stack overflow");
    }

    #[test]
    fn a_report_that_is_not_xml_is_refused() {
        assert!(TestFailure::all_in("not xml").is_err());
    }
}
