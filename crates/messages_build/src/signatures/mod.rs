//! [`signatures`]: what each message of a source-language file is called with.

use std::collections::BTreeMap;

use mf2_model::{Declaration, Expression, Message, Pattern, PatternPart};

/// The kind of value an input takes, from the function its message annotates it with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputKind {
    /// Text, as given: no annotation, or `:string`.
    Text,
    /// A whole number: `:integer`.
    Integer,
    /// A number: `:number`.
    Number,
}

impl InputKind {
    /// The kind a function annotation gives; an unknown function takes text.
    fn of(function: Option<&str>) -> Self {
        match function {
            Some("integer") => Self::Integer,
            Some("number") => Self::Number,
            _ => Self::Text,
        }
    }
}

/// One input of a message: its name and kind.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Input {
    /// The variable's name, without its `$`.
    pub name: String,
    /// The kind of value it takes.
    pub kind: InputKind,
}

/// What a message is called by and with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Signature {
    /// Its id: `panel.version`.
    pub id: String,
    /// The Rust path it is called by: `["panel", "version"]`.
    pub path: Vec<String>,
    /// The comment above it, for the translator and the generated function's docs.
    pub doc: Option<String>,
    /// Its inputs, in the order they first appear.
    pub inputs: Vec<Input>,
}

/// The signature of every message in `source`, a source-language resource, in file order.
///
/// # Errors
/// Each problem, naming its message: the resource or a message does not parse, or a message
/// calls a function the runtime lacks, or `:brand` without a literal word of the brand.
pub fn signatures(source: &str) -> Result<Vec<Signature>, Vec<String>> {
    let (resource, diagnostics) = mf2_resource::parse(source);
    let mut problems: Vec<String> = diagnostics
        .iter()
        .map(|diagnostic| format!("the resource: {diagnostic:?}"))
        .collect();
    let mut found = Vec::new();
    for entry in resource.iter() {
        let id = entry.id();
        let name = id.canonical().unwrap_or_else(|_| id.parts().join("."));
        let parsed = mf2_syntax::parse_model(entry.entry.value.as_ref());
        let Some(message) = parsed.message.filter(|_| parsed.diagnostics.is_empty()) else {
            problems.push(format!("{name}: {:?}", parsed.diagnostics));
            continue;
        };
        problems.extend(
            crate::functions::refused_calls(&message)
                .into_iter()
                .map(|refused| format!("{name}: {refused}")),
        );
        found.push(Signature {
            path: id
                .parts()
                .iter()
                .map(|part| part.replace('-', "_"))
                .collect(),
            id: name,
            doc: entry
                .entry
                .comment
                .as_ref()
                .map(|comment| comment.text.trim().to_owned()),
            inputs: inputs(&message),
        });
    }
    if problems.is_empty() {
        Ok(found)
    } else {
        Err(problems)
    }
}

/// A message's external inputs: its `.input` declarations, then every unbound variable.
fn inputs(message: &Message<'_>) -> Vec<Input> {
    let mut order: Vec<String> = Vec::new();
    let mut kinds: BTreeMap<String, InputKind> = BTreeMap::new();
    let mut locals: Vec<String> = Vec::new();
    let mut note = |name: &str, function: Option<&str>| {
        if !kinds.contains_key(name) {
            order.push(name.to_owned());
            kinds.insert(name.to_owned(), InputKind::of(function));
        } else if let Some(function) = function
            && kinds.get(name) == Some(&InputKind::Text)
        {
            kinds.insert(name.to_owned(), InputKind::of(Some(function)));
        }
    };
    for declaration in message.declarations() {
        match declaration {
            Declaration::Input(input) => {
                note(
                    &input.name,
                    input.value.function.as_ref().map(|f| f.name.as_ref()),
                );
            }
            Declaration::Local(local) => locals.push(local.name.to_string()),
            // A declaration a later MF2 adds binds nothing this version reads.
            _ => {}
        }
    }
    let patterns: Vec<&Pattern<'_>> = match message {
        Message::Pattern(pattern) => vec![&pattern.pattern],
        Message::Select(select) => select
            .variants
            .iter()
            .map(|variant| &variant.value)
            .collect(),
        _ => Vec::new(),
    };
    for pattern in patterns {
        for part in pattern.iter() {
            if let PatternPart::Expression(Expression::Variable(variable)) = part
                && !locals.iter().any(|local| *local == variable.arg.name)
            {
                note(
                    &variable.arg.name,
                    variable.function.as_ref().map(|f| f.name.as_ref()),
                );
            }
        }
    }
    order
        .into_iter()
        .map(|name| Input {
            kind: kinds.get(&name).copied().unwrap_or(InputKind::Text),
            name,
        })
        .collect()
}
