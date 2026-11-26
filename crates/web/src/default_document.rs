use crate::page_context::PageContext;

/// Wraps rendered page `body` HTML in a minimal HTML5 document.
///
/// `body` is trusted and not escaped; the page's head values are.
#[must_use]
pub fn default_document(ctx: &PageContext, body: &str) -> String {
    if let Some(document) = ctx.document() {
        return document.render(ctx, body);
    }
    let lang = ctx.locale().map_or("en", |locale| locale.language());
    let page_head = ctx.head().render();
    format!(
        "<!DOCTYPE html>\
<html lang=\"{lang}\">\
<head>\
<meta charset=\"utf-8\">\
<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
{page_head}</head>\
<body>{body}</body>\
</html>"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context() -> PageContext {
        PageContext::new(Vec::new(), "req-1")
    }

    #[test]
    fn an_installed_document_wraps_the_page() {
        let document = crate::Document::new().theme("appearance");
        let ctx = context()
            .with_preferences(vec![("appearance", "light")])
            .with_document(Some(std::sync::Arc::new(document)));
        assert!(default_document(&ctx, "").contains("data-theme=\"light\""));
    }

    #[test]
    fn wraps_body_in_a_full_document() {
        let html = default_document(&context(), "<main>hi</main>");
        assert!(html.starts_with("<!DOCTYPE html>"));
        assert!(html.contains("<meta charset=\"utf-8\">"));
        assert!(html.contains("<body><main>hi</main></body>"));
        assert!(html.trim_end().ends_with("</html>"));
    }

    #[test]
    fn the_document_declares_the_language_it_was_rendered_in() {
        let dutch = context().with_locale(Some("nl-BE".parse().expect("a language tag")));
        assert!(
            default_document(&dutch, "").contains("<html lang=\"nl\">"),
            "the language subtag is what a screen reader needs"
        );
    }

    #[test]
    fn a_surface_that_negotiates_nothing_still_declares_a_language() {
        assert!(default_document(&context(), "").contains("<html lang=\"en\">"));
    }

    #[test]
    fn a_page_that_recorded_no_head_gets_none() {
        // A browser falls back to the URL for an absent title, and shows nothing for a blank one.
        let html = default_document(&context(), "");
        assert!(!html.contains("<title>"));
        assert!(!html.contains("rel=\"canonical\""));
    }

    #[test]
    fn the_page_head_reaches_the_document() {
        let ctx = context();
        ctx.set_title("Pricing");
        ctx.set_description("What you are charged for.");
        ctx.set_canonical("https://example.com/pricing");
        ctx.add_meta("robots", "index,follow");
        ctx.add_link("alternate", "https://example.com/nl/pricing");

        let html = default_document(&ctx, "<main></main>");
        assert!(html.contains("<title>Pricing</title>"));
        assert!(html.contains("<meta name=\"description\" content=\"What you are charged for.\">"));
        assert!(html.contains("<link rel=\"canonical\" href=\"https://example.com/pricing\">"));
        assert!(html.contains("<meta name=\"robots\" content=\"index,follow\">"));
        assert!(html.contains("<link rel=\"alternate\" href=\"https://example.com/nl/pricing\">"));
        // A `<title>` after `</head>` is moved into `<body>` by the parser.
        let head_end = html.find("</head>").expect("a head");
        assert!(html.find("<title>").expect("a title") < head_end);
    }

    #[test]
    fn the_last_title_wins_so_a_page_can_refine_its_shell() {
        let ctx = context();
        ctx.set_title("Guides");
        ctx.set_title("Ongoing monitoring, in practice");
        assert!(
            default_document(&ctx, "").contains("<title>Ongoing monitoring, in practice</title>")
        );
    }

    #[test]
    fn a_head_value_cannot_break_out_of_its_element_or_attribute() {
        let ctx = context();
        ctx.set_title("</title><script>alert(1)</script>");
        ctx.set_description("a \" onload=\"alert(1)");
        ctx.set_canonical("https://example.com/?a=1&b=2");

        let html = default_document(&ctx, "");
        assert!(!html.contains("<script>"));
        assert!(html.contains("&lt;/title&gt;&lt;script&gt;"));
        assert!(html.contains("content=\"a &quot; onload=&quot;alert(1)\">"));
        // Escaped once, not twice: `&amp;amp;` would be a visible defect in a canonical URL.
        assert!(html.contains("?a=1&amp;b=2"));
        assert!(!html.contains("&amp;amp;"));
    }
}
