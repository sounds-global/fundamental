//! The pinned Tailwind builds a stylesheet from an input and the sources it scans.

use std::fs;

use fundamental_build::tailwind;

#[test]
fn the_pinned_tailwind_compiles_the_utilities_a_source_names() {
    let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
    fs::create_dir_all(dir.path().join("src")).unwrap_or_else(|error| panic!("{error}"));
    fs::write(
        dir.path().join("src/part.rs"),
        r#"const LOOK: &str = "bg-(--kui-primary) px-4";"#,
    )
    .unwrap_or_else(|error| panic!("{error}"));
    fs::write(
        dir.path().join("input.css"),
        "@import \"tailwindcss/theme.css\";\n@import \"tailwindcss/utilities.css\";\n@source \"src\";\n",
    )
    .unwrap_or_else(|error| panic!("{error}"));
    let out = dir.path().join("out.css");
    tailwind::compile(&dir.path().join("input.css"), &out)
        .unwrap_or_else(|error| panic!("{error:?}"));
    let css = fs::read_to_string(&out).unwrap_or_else(|error| panic!("{error}"));
    assert!(
        css.contains("background-color: var(--kui-primary)"),
        "{css}"
    );
    assert!(css.contains(".px-4"), "{css}");
}

#[test]
fn the_utilities_of_given_sources_are_one_stylesheet() {
    let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
    fs::write(
        dir.path().join("part.rs"),
        r#"const LOOK: &str = "text-(--kui-ink) gap-2";"#,
    )
    .unwrap_or_else(|error| panic!("{error}"));
    let css = tailwind::utilities(&[dir.path()]).unwrap_or_else(|error| panic!("{error:?}"));
    assert!(css.contains("color: var(--kui-ink)"), "{css}");
    assert!(css.contains(".gap-2"), "the theme's spacing is in: {css}");
    assert!(
        !css.contains("box-sizing: border-box"),
        "no preflight: {css}"
    );
}
