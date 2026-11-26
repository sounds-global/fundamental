//! A message that says the brand before any is installed says nothing; its own test binary.

use fundamental_messages_fixture::m;

#[test]
fn a_message_says_nothing_of_a_brand_never_installed() {
    assert_eq!(m::CATALOGS.say("brand.signed", "en", &[]), None);
    assert_eq!(m::brand::signed().format_in("en"), "brand.signed");
}
