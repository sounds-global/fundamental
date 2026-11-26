//! Markup without its classes: what a structure test reads, so the look can change under it.

use fundamental_test_render::without_classes;

#[test]
fn every_class_attribute_is_left_out() {
    assert_eq!(
        without_classes(
            r#"<div class="a b"><span class="">x</span><a href="/" class="c">y</a></div>"#
        ),
        r#"<div><span>x</span><a href="/">y</a></div>"#
    );
}

#[test]
fn an_attribute_that_only_ends_in_class_stays() {
    assert_eq!(
        without_classes(r#"<td data-class="k" title="class">x</td>"#),
        r#"<td data-class="k" title="class">x</td>"#
    );
}
