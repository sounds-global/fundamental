//! [`Landing`]: the control a reply lands at.

use fundamental_core::address::Landing;

#[test]
fn a_landing_writes_its_control_as_query_and_fragment() {
    let landing = Landing::new("f_portals_d0_dtitle").unwrap();
    assert_eq!(
        landing.location("/aanvraag/apps"),
        "/aanvraag/apps?at=f_portals_d0_dtitle#f_portals_d0_dtitle"
    );
}

#[test]
fn a_landing_keeps_the_page_query_and_drops_its_fragment() {
    let landing = Landing::new("example").unwrap();
    assert_eq!(
        landing.location("/aanvraag/apps?next=%2Fhome#old"),
        "/aanvraag/apps?next=%2Fhome&at=example#example"
    );
}

#[test]
fn a_landing_is_read_back_from_its_query() {
    let read = Landing::read("next=%2Fhome&at=f_users_d1").unwrap();
    assert_eq!(read.anchor(), "f_users_d1");
    assert_eq!(read, Landing::new("f_users_d1").unwrap());
}

#[test]
fn a_query_without_a_landing_reads_none() {
    assert_eq!(Landing::read(""), None);
    assert_eq!(Landing::read("next=%2Fhome"), None);
    assert_eq!(Landing::read("bat=f_x"), None);
}

#[test]
fn only_an_element_id_of_small_letters_digits_and_underscores_lands() {
    for refused in [
        "",
        "F_x",
        "f-x",
        "f x",
        "f_x\"",
        "f_x#y",
        "<script>",
        "f_\u{e9}",
        &"a".repeat(65),
    ] {
        assert_eq!(Landing::new(refused), None, "{refused:?} lands");
        assert_eq!(
            Landing::read(&format!("at={refused}")),
            None,
            "{refused:?} is read"
        );
    }
    assert!(Landing::new(&"a".repeat(64)).is_some());
}

#[test]
fn the_query_name_is_at() {
    assert_eq!(Landing::QUERY, "at");
}
