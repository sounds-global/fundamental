//! [`PayloadRejection`] — why a frame's JSON was refused, without a byte of it.

use std::fmt;

use serde_json::error::Category;

/// Why a frame, or a payload inside one, did not deserialize: serde's class and position.
///
/// serde's message quotes the client's input, so only the class and position are kept.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PayloadRejection {
    category: Category,
    line: usize,
    column: usize,
}

impl PayloadRejection {
    /// The rejection a serde failure describes, keeping its class and position only.
    #[must_use]
    pub fn of(error: &serde_json::Error) -> Self {
        Self {
            category: error.classify(),
            line: error.line(),
            column: error.column(),
        }
    }

    /// serde's class of failure.
    #[must_use]
    pub const fn category(&self) -> Category {
        self.category
    }

    /// The class as it is recorded.
    #[must_use]
    pub const fn class(&self) -> &'static str {
        match self.category {
            Category::Syntax => "syntax",
            Category::Data => "data",
            Category::Eof => "eof",
            Category::Io => "io",
        }
    }

    const fn explanation(&self) -> &'static str {
        match self.category {
            Category::Syntax => "the input is not JSON",
            Category::Data => "a field is missing, unknown, or of the wrong type",
            Category::Eof => "the JSON ends early",
            Category::Io => "the input could not be read",
        }
    }
}

impl fmt::Display for PayloadRejection {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.explanation())?;
        // A value deserialized from an already-parsed tree has no position: serde reports 0.
        if self.line > 0 {
            write!(formatter, " (line {}, column {})", self.line, self.column)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;

    use super::*;

    const EMAIL: &str = "jan@example.nl";

    #[derive(Debug, Deserialize)]
    #[expect(dead_code, reason = "the test only deserializes into it")]
    struct Pull {
        since: i64,
    }

    #[test]
    fn a_mistyped_field_is_a_data_rejection_that_quotes_nothing() {
        let error = serde_json::from_str::<Pull>(&format!(r#"{{"since":"{EMAIL}"}}"#))
            .expect_err("a string is not an i64");
        assert!(error.to_string().contains(EMAIL), "serde quotes the value");
        let rejection = PayloadRejection::of(&error);
        assert_eq!(rejection.category(), Category::Data);
        assert_eq!(rejection.class(), "data");
        let shown = format!("{rejection} {rejection:?}");
        assert!(!shown.contains(EMAIL), "{shown}");
        assert!(shown.contains("line 1"), "{shown}");
    }

    #[test]
    fn a_frame_that_is_not_json_is_a_syntax_rejection() {
        let error = serde_json::from_str::<Pull>(EMAIL).expect_err("not JSON");
        assert_eq!(PayloadRejection::of(&error).class(), "syntax");
    }

    #[test]
    fn a_tree_value_has_no_position_to_report() {
        let error = serde_json::from_value::<Pull>(serde_json::json!({ "since": EMAIL }))
            .expect_err("a string is not an i64");
        assert_eq!(
            PayloadRejection::of(&error).to_string(),
            "a field is missing, unknown, or of the wrong type"
        );
    }
}
