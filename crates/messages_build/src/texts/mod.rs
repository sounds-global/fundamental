//! [`texts`]: what a reader can read of each message in a file.

use mf2_model::{Expression, Message, Pattern, PatternPart};
use mf2_resource::LineIndex;

/// One text a reader can read: a message's pattern, or one variant of its `.match`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Text {
    /// The message's id: `panel.pages`.
    pub id: String,
    /// The line its message's value starts on, counting from 1.
    pub line: usize,
    /// The words, each input written as its placeholder (`{$count}`), a literal as its value,
    /// and markup left out.
    pub text: String,
}

/// Every text of `resource`, in file order: one per message, or one per `.match` variant.
///
/// # Errors
/// Each problem, naming its message: the resource or a message does not parse.
pub fn texts(resource: &str) -> Result<Vec<Text>, Vec<String>> {
    let (parsed, diagnostics) = mf2_resource::parse(resource);
    let lines = LineIndex::new(resource);
    let mut problems: Vec<String> = diagnostics
        .iter()
        .map(|diagnostic| format!("the resource: {diagnostic:?}"))
        .collect();
    let mut found = Vec::new();
    for entry in parsed.iter() {
        let id = entry.id();
        let id = id.canonical().unwrap_or_else(|_| id.parts().join("."));
        let model = mf2_syntax::parse_model(entry.entry.value.as_ref());
        let Some(message) = model.message.filter(|_| model.diagnostics.is_empty()) else {
            problems.push(format!("{id}: {:?}", model.diagnostics));
            continue;
        };
        let line = lines.position(resource, entry.entry.value_span.start).line as usize;
        let patterns: Vec<&Pattern<'_>> = match &message {
            Message::Pattern(pattern) => vec![&pattern.pattern],
            Message::Select(select) => select
                .variants
                .iter()
                .map(|variant| &variant.value)
                .collect(),
            _ => Vec::new(),
        };
        found.extend(patterns.into_iter().map(|pattern| Text {
            id: id.clone(),
            line,
            text: words(pattern),
        }));
    }
    if problems.is_empty() {
        Ok(found)
    } else {
        Err(problems)
    }
}

/// A pattern's words as a reader reads them, an input standing as its placeholder.
fn words(pattern: &Pattern<'_>) -> String {
    let mut out = String::new();
    for part in pattern.iter() {
        match part {
            PatternPart::Text(text) => out.push_str(text),
            PatternPart::Expression(Expression::Literal(literal)) => {
                out.push_str(&literal.arg.value);
            }
            PatternPart::Expression(Expression::Variable(variable)) => {
                out.push_str("{$");
                out.push_str(&variable.arg.name);
                out.push('}');
            }
            PatternPart::Expression(Expression::Function(function)) => {
                out.push_str("{:");
                out.push_str(&function.function.name);
                out.push('}');
            }
            // Markup is how a text is drawn, never words of it; nor is a part a later MF2 adds.
            _ => {}
        }
    }
    out
}
