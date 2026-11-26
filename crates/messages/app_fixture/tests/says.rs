//! The app's messages are said as any crate's are.

use fundamental_messages_app_fixture::m;

#[test]
fn the_app_s_messages_say_its_own_words() {
    assert_eq!(
        m::panel::save_for_owner().format_in("en"),
        "Keep for an owner"
    );
    assert_eq!(m::change::any_value("stage").format_in("nl"), "Elke fase");
}
