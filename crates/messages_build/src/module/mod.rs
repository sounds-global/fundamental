//! [`module`]: the Rust a crate's messages are called by.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use crate::{InputKind, Signature};

/// The items of a crate's message module: one typed function per message, nested by its path;
/// `facade` is the path the runtime is named by.
#[must_use]
pub fn module(signatures: &[Signature], facade: &str, catalogs: &str) -> String {
    let mut tree = Node::default();
    for signature in signatures {
        tree.insert(&signature.path, signature);
    }
    let mut out = String::new();
    tree.write(&mut out, facade, catalogs, "");
    out
}

/// A module of the generated tree: its functions and the modules below it.
#[derive(Default)]
struct Node<'s> {
    functions: Vec<(&'s str, &'s Signature)>,
    modules: BTreeMap<&'s str, Node<'s>>,
}

impl<'s> Node<'s> {
    fn insert(&mut self, path: &'s [String], signature: &'s Signature) {
        match path {
            [] => {}
            [name] => self.functions.push((name, signature)),
            [module, rest @ ..] => self
                .modules
                .entry(module)
                .or_default()
                .insert(rest, signature),
        }
    }

    fn write(&self, out: &mut String, facade: &str, catalogs: &str, prefix: &str) {
        for (name, signature) in &self.functions {
            function(out, facade, catalogs, name, signature);
        }
        if !self.functions.is_empty() {
            inputs(out, &self.functions);
        }
        for (name, node) in &self.modules {
            let path = if prefix.is_empty() {
                (*name).to_owned()
            } else {
                format!("{prefix}.{name}")
            };
            let _ = writeln!(
                out,
                "/// The messages under `{path}`.\npub mod {} {{",
                ident(name)
            );
            node.write(out, facade, catalogs, &path);
            out.push_str("}\n");
        }
    }
}

/// One message's function.
fn function(out: &mut String, facade: &str, catalogs: &str, name: &str, signature: &Signature) {
    match &signature.doc {
        Some(doc) => doc.lines().for_each(|line| {
            let _ = writeln!(out, "/// {line}");
        }),
        None => {
            let _ = writeln!(out, "/// `{}`.", signature.id);
        }
    }
    let parameters: Vec<String> = signature
        .inputs
        .iter()
        .map(|input| {
            let ty = match input.kind {
                InputKind::Text => "impl ::std::fmt::Display",
                InputKind::Integer => "impl ::core::convert::Into<i64>",
                InputKind::Number => "impl ::core::convert::Into<f64>",
            };
            format!("{}: {ty}", ident(&input.name))
        })
        .collect();
    let values: Vec<String> = signature
        .inputs
        .iter()
        .map(|input| {
            let value = match input.kind {
                InputKind::Text => format!("{facade}::Value::text({})", ident(&input.name)),
                InputKind::Integer => format!(
                    "{facade}::Value::from(::core::convert::Into::<i64>::into({}))",
                    ident(&input.name)
                ),
                InputKind::Number => format!(
                    "{facade}::Value::from(::core::convert::Into::<f64>::into({}))",
                    ident(&input.name)
                ),
            };
            format!("(\"{}\", {value})", input.name)
        })
        .collect();
    let _ = writeln!(
        out,
        "pub fn {}({}) -> {facade}::Message {{\n    {facade}::Message::new(&{catalogs}, \"{}\", ::std::vec![{}])\n}}",
        ident(name),
        parameters.join(", "),
        signature.id,
        values.join(", "),
    );
}

/// The names of each message's inputs, for `#[derive(Messages)]` to check fields against.
fn inputs(out: &mut String, functions: &[(&str, &Signature)]) {
    out.push_str("#[doc(hidden)]\npub mod __inputs {\n");
    for (name, signature) in functions {
        let names: Vec<String> = signature
            .inputs
            .iter()
            .map(|input| format!("\"{}\"", input.name))
            .collect();
        let _ = writeln!(
            out,
            "pub const {}: &[&str] = &[{}];",
            name.to_ascii_uppercase(),
            names.join(", ")
        );
    }
    out.push_str("}\n");
}

/// A name as Rust calls it: a keyword as its raw identifier (`r#use`).
fn ident(name: &str) -> String {
    const KEYWORDS: &[&str] = &[
        "as", "async", "await", "box", "break", "const", "continue", "dyn", "else", "enum",
        "extern", "false", "fn", "for", "gen", "if", "impl", "in", "let", "loop", "match", "mod",
        "move", "mut", "pub", "ref", "return", "static", "struct", "trait", "true", "try", "type",
        "unsafe", "use", "where", "while", "yield", "abstract", "become", "do", "final", "macro",
        "override", "priv", "typeof", "unsized", "virtual",
    ];
    if KEYWORDS.contains(&name) {
        format!("r#{name}")
    } else {
        name.to_owned()
    }
}
