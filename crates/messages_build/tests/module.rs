//! The generated module: one function per message, nested by its path, each taking its inputs
//! typed and returning the message to say in the reader's language.

use fundamental_messages_build::{module, signatures};

const PANEL: &str = "@locale en
---
[panel]
# The act at the foot of every panel.
save_for_owner = Save for an owner
version = .input {$number :integer} {{Version {$number}}}
greeting = Hello {$name}, you have {$unread :number} new
";

#[test]
fn each_message_is_a_function_at_its_path_with_typed_inputs() {
    let code = module(
        &signatures(PANEL).expect("a resource"),
        "::fundamental::messages",
        "crate::m::CATALOGS",
    );
    for part in [
        "/// The messages under `panel`.\npub mod panel {",
        "/// The act at the foot of every panel.",
        "pub fn save_for_owner() -> ::fundamental::messages::Message {",
        r#"::fundamental::messages::Message::new(&crate::m::CATALOGS, "panel.save_for_owner", ::std::vec![])"#,
        "pub fn version(number: impl ::core::convert::Into<i64>) -> ::fundamental::messages::Message {",
        r#"("number", ::fundamental::messages::Value::from(::core::convert::Into::<i64>::into(number)))"#,
        "pub fn greeting(name: impl ::std::fmt::Display, unread: impl ::core::convert::Into<f64>) -> ::fundamental::messages::Message {",
        r#"("name", ::fundamental::messages::Value::text(name))"#,
    ] {
        assert!(code.contains(part), "{part}\n in {code}");
    }
}

#[test]
fn a_message_named_as_a_rust_keyword_is_called_by_its_raw_name() {
    let source = "@locale en\n---\n[use]\ntype = Kind\nread = Read it {$type}\n";
    let code = module(
        &signatures(source).expect("a resource"),
        "crate::m::__messages",
        "crate::m::CATALOGS",
    );
    for part in [
        "pub mod r#use {",
        "pub fn r#type() ->",
        "pub fn read(r#type: impl ::std::fmt::Display) ->",
        r#"("type", crate::m::__messages::Value::text(r#type))"#,
    ] {
        assert!(code.contains(part), "{part}\n in {code}");
    }
}

#[test]
fn each_module_names_its_messages_inputs_in_order() {
    let code = module(
        &signatures(PANEL).expect("a resource"),
        "::fundamental::messages",
        "crate::m::CATALOGS",
    );
    for part in [
        "pub mod __inputs {",
        "pub const SAVE_FOR_OWNER: &[&str] = &[];",
        r#"pub const GREETING: &[&str] = &["name", "unread"];"#,
    ] {
        assert!(code.contains(part), "{part}\n in {code}");
    }
}
