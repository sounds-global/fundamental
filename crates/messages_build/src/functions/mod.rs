//! [`refused_calls`]: the function calls of a message the runtime cannot make.

use mf2_model::{Declaration, Expression, FunctionRef, Message, Pattern, PatternPart};

/// The functions the runtime's registry has (`fundamental_messages::FUNCTION_NAMES`): a
/// message calls no other.
pub const FUNCTIONS: [&str; 6] = [
    "string", "integer", "number", "percent", "currency", "brand",
];

/// The words of the brand `:brand` says (`fundamental_messages::BrandTerm`).
pub const BRAND_TERMS: [&str; 4] = ["name", "legal-name", "site-domain", "platform-domain"];

/// Every call of `message` the runtime cannot make.
pub(crate) fn refused_calls(message: &Message<'_>) -> Vec<String> {
    let mut refused = Vec::new();
    for declaration in message.declarations() {
        match declaration {
            Declaration::Input(input) => {
                if let Some(function) = &input.value.function {
                    judge(function, Operand::Variable, &mut refused);
                }
            }
            Declaration::Local(local) => expression(&local.value, &mut refused),
            // A declaration a later MF2 adds calls nothing this version reads.
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
            if let PatternPart::Expression(found) = part {
                expression(found, &mut refused);
            }
        }
    }
    refused
}

/// What a function is called on.
enum Operand<'t> {
    /// A literal: `{name :brand}`.
    Literal(&'t str),
    /// A variable: `{$who :brand}`.
    Variable,
    /// Nothing: `{:brand}`.
    None,
}

fn expression(expression: &Expression<'_>, refused: &mut Vec<String>) {
    match expression {
        Expression::Literal(literal) => {
            if let Some(function) = &literal.function {
                judge(function, Operand::Literal(&literal.arg.value), refused);
            }
        }
        Expression::Variable(variable) => {
            if let Some(function) = &variable.function {
                judge(function, Operand::Variable, refused);
            }
        }
        Expression::Function(call) => judge(&call.function, Operand::None, refused),
        // An expression a later MF2 adds calls nothing this version reads.
        _ => {}
    }
}

fn judge(function: &FunctionRef<'_>, operand: Operand<'_>, refused: &mut Vec<String>) {
    let name = function.name.as_ref();
    if !FUNCTIONS.contains(&name) {
        refused.push(format!(
            ":{name} is no function of the runtime; a message calls :{}",
            FUNCTIONS.join(", :")
        ));
        return;
    }
    if name != "brand" {
        return;
    }
    let words = BRAND_TERMS.join(", ");
    match operand {
        Operand::Literal(word) if BRAND_TERMS.contains(&word) => {}
        Operand::Literal(word) => refused.push(format!(
            "{{{word} :brand}} names no word of the brand; it says one of {words}"
        )),
        Operand::Variable => refused.push(format!(
            ":brand says a word of the brand written in the message ({words}), never a variable"
        )),
        Operand::None => refused.push(format!(
            ":brand needs the word of the brand it says: one of {words}"
        )),
    }
}
