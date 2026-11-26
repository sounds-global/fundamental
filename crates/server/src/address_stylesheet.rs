use fundamental_core::asset::AssetVersion;

/// `css`, the stylesheet at `sheet`, with every `url(…)` naming a known file rewritten to `path?v=<version>`.
///
/// Every other reference is left byte for byte, so addressing twice equals addressing once.
pub(crate) fn address_stylesheet<'v>(
    sheet: &str,
    css: &str,
    version_of: impl Fn(&str) -> Option<&'v AssetVersion>,
) -> (String, usize) {
    let mut out = String::with_capacity(css.len());
    let mut rest = css;
    let mut rewritten = 0;
    while let Some(at) = rest.find("url(") {
        let (before, from_url) = rest.split_at(at + "url(".len());
        out.push_str(before);
        let quote = match from_url.as_bytes().first() {
            Some(b'"') => "\"",
            Some(b'\'') => "'",
            _ => "",
        };
        let target_start = quote.len();
        let end = match quote {
            "" => from_url.find(')'),
            quote => from_url[target_start..]
                .find(quote)
                .map(|end| end + target_start),
        };
        let Some(end) = end else {
            out.push_str(from_url);
            return (out, rewritten);
        };
        let target = &from_url[target_start..end];
        out.push_str(&from_url[..end]);
        if let Some(version) = resolved(sheet, target).and_then(|path| version_of(&path)) {
            out.push_str("?v=");
            out.push_str(version.as_str());
            rewritten += 1;
        }
        rest = &from_url[end..];
    }
    out.push_str(rest);
    (out, rewritten)
}

/// The path `target` names from the stylesheet at `sheet`, when the server could hold it.
fn resolved(sheet: &str, target: &str) -> Option<String> {
    let addressable = !target.is_empty()
        && !target.starts_with("//")
        && !target.contains(['?', '#', ':'])
        && !target.ends_with(".css");
    if !addressable {
        return None;
    }
    if target.starts_with('/') {
        return Some(target.to_owned());
    }
    let directory = sheet
        .rsplit_once('/')
        .map_or("", |(directory, _)| directory);
    let mut segments: Vec<&str> = directory.split('/').filter(|s| !s.is_empty()).collect();
    for segment in target.split('/') {
        match segment {
            "." | "" => {}
            ".." => {
                segments.pop()?;
            }
            name => segments.push(name),
        }
    }
    Some(format!("/{}", segments.join("/")))
}

#[cfg(test)]
mod tests {
    use super::*;

    const FONT: &str = "/_f/fonts/inter-400.woff2";
    const SHEET: &str = "/_f/app.css";

    fn version() -> AssetVersion {
        AssetVersion::parse("q83vEjRWeJCrze8SNFZ4kKvN7xI0VniQq83vEjRWeJA").expect("valid")
    }

    fn known(target: &str) -> Option<&'static AssetVersion> {
        static VERSION: std::sync::OnceLock<AssetVersion> = std::sync::OnceLock::new();
        (target == FONT).then(|| VERSION.get_or_init(version))
    }

    #[test]
    fn a_stylesheet_names_the_fonts_it_loads_by_version() {
        let css = format!("@font-face{{src:url(\"{FONT}\") format(\"woff2\")}}");
        let (addressed, count) = address_stylesheet(SHEET, &css, known);
        assert_eq!(count, 1);
        assert_eq!(
            addressed,
            format!(
                "@font-face{{src:url(\"{FONT}?v={}\") format(\"woff2\")}}",
                version()
            )
        );
    }

    #[test]
    fn quoted_unquoted_and_single_quoted_urls_are_addressed() {
        let css = format!("a{{b:url({FONT})}}c{{d:url('{FONT}')}}e{{f:url(\"{FONT}\")}}");
        let (addressed, count) = address_stylesheet(SHEET, &css, known);
        assert_eq!(count, 3);
        let v = version();
        assert_eq!(
            addressed,
            format!(
                "a{{b:url({FONT}?v={v})}}c{{d:url('{FONT}?v={v}')}}e{{f:url(\"{FONT}?v={v}\")}}"
            )
        );
    }

    #[test]
    fn a_reference_to_nothing_held_or_with_its_own_query_is_left_alone() {
        let css = format!(
            "a{{b:url(/_f/other.woff2)}}c{{d:url({FONT}?x=1)}}e{{f:url(data:font/woff2;base64,AA)}}g{{h:url(fonts/x.woff2)}}i{{j:url(//cdn.example{FONT})}}"
        );
        let (addressed, count) = address_stylesheet(SHEET, &css, known);
        assert_eq!(count, 0);
        assert_eq!(addressed, css);
    }

    #[test]
    fn addressing_twice_changes_nothing() {
        let css = format!("a{{b:url(\"{FONT}\")}}");
        let (once, _) = address_stylesheet(SHEET, &css, known);
        let (twice, count) = address_stylesheet(SHEET, &once, known);
        assert_eq!(count, 0);
        assert_eq!(twice, once);
    }

    #[test]
    fn a_relative_reference_is_addressed_as_the_browser_resolves_it_from_the_sheet() {
        let css = "a{b:url(inter-400.woff2)}c{d:url('./inter-400.woff2')}e{f:url(\"../fonts/inter-400.woff2\")}";
        let (addressed, count) = address_stylesheet("/_f/fonts/fonts.css", css, known);
        assert_eq!(count, 3);
        let v = version();
        assert_eq!(
            addressed,
            format!(
                "a{{b:url(inter-400.woff2?v={v})}}c{{d:url('./inter-400.woff2?v={v}')}}e{{f:url(\"../fonts/inter-400.woff2?v={v}\")}}"
            )
        );
    }

    #[test]
    fn a_relative_reference_above_the_root_or_to_another_scheme_is_left_alone() {
        let css =
            "a{b:url(../../../inter-400.woff2)}c{d:url(data:font/woff2;base64,AA)}e{f:url(#x)}";
        let (addressed, count) = address_stylesheet("/_f/fonts/fonts.css", css, known);
        assert_eq!(count, 0);
        assert_eq!(addressed, css);
    }

    #[test]
    fn an_unterminated_reference_is_kept_as_it_was() {
        let css = format!("a{{b:url(\"{FONT}");
        let (addressed, count) = address_stylesheet(SHEET, &css, known);
        assert_eq!(count, 0);
        assert_eq!(addressed, css);
    }
}
