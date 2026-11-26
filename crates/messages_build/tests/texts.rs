//! What a reader can read of a crate's messages: each pattern's words, each variant of a
//! `.match` on its own, an input written as its placeholder, at the line its message starts on.
//! A copy gate reads these as it reads any other text a reader sees.

use fundamental_messages_build::{Text, texts};

const PANEL: &str = "@locale en
---
[panel]
# The act at the foot of every panel.
save_for_owner = Save for an owner
pages = .input {$count :integer}
        .match $count
        one {{{$count} page}}
        * {{{$count} pages}}
greeting = Hello {$name}, {|welcome|} {#b}back{/b}
";

fn text(id: &str, line: usize, words: &str) -> Text {
    Text {
        id: id.to_owned(),
        line,
        text: words.to_owned(),
    }
}

#[test]
fn each_pattern_is_read_as_a_reader_sees_it() {
    assert_eq!(
        texts(PANEL).expect("a resource"),
        [
            text("panel.save_for_owner", 5, "Save for an owner"),
            text("panel.pages", 6, "{$count} page"),
            text("panel.pages", 6, "{$count} pages"),
            text("panel.greeting", 10, "Hello {$name}, welcome back"),
        ]
    );
}

#[test]
fn a_message_that_does_not_parse_is_named() {
    let problems = texts("@locale en\n---\nbroken = {$\n").expect_err("a broken message");
    assert!(
        problems.iter().any(|problem| problem.starts_with("broken")),
        "{problems:?}"
    );
}
