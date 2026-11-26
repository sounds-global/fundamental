//! A message's signature is read from the source language's file: its id, the Rust path it is
//! called by, and each input with the kind its annotation gives it.

use fundamental_messages_build::{Input, InputKind, Signature, signatures};

const PANEL: &str = "@locale en
---
[panel]
# The act at the foot of every panel.
save_for_owner = Save for an owner
version = .input {$number :integer} {{Version {$number}}}
pages = .input {$count :integer}
        .match $count
        one {{{$count} page}}
        * {{{$count} pages}}
greeting = Hello {$name}, you have {$unread :number} new
";

fn input(name: &str, kind: InputKind) -> Input {
    Input {
        name: name.to_owned(),
        kind,
    }
}

#[test]
fn each_message_is_called_by_its_path_with_its_typed_inputs() {
    let found = signatures(PANEL).expect("a resource");
    assert_eq!(
        found,
        [
            Signature {
                id: "panel.save_for_owner".to_owned(),
                path: vec!["panel".to_owned(), "save_for_owner".to_owned()],
                doc: Some("The act at the foot of every panel.".to_owned()),
                inputs: vec![],
            },
            Signature {
                id: "panel.version".to_owned(),
                path: vec!["panel".to_owned(), "version".to_owned()],
                doc: None,
                inputs: vec![input("number", InputKind::Integer)],
            },
            Signature {
                id: "panel.pages".to_owned(),
                path: vec!["panel".to_owned(), "pages".to_owned()],
                doc: None,
                inputs: vec![input("count", InputKind::Integer)],
            },
            Signature {
                id: "panel.greeting".to_owned(),
                path: vec!["panel".to_owned(), "greeting".to_owned()],
                doc: None,
                inputs: vec![
                    input("name", InputKind::Text),
                    input("unread", InputKind::Number)
                ],
            },
        ]
    );
}

#[test]
fn a_message_that_does_not_parse_is_named_with_its_problem() {
    let problems = signatures("@locale en\n---\nbroken = {{unclosed\n").expect_err("refused");
    assert!(
        problems.iter().any(|problem| problem.contains("broken")),
        "{problems:?}"
    );
}
