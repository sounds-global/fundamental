use fundamental_core::escape;

use crate::ModuleScript;

/// The `<head>` content a page recorded while it rendered.
///
/// Stored unescaped; [`render`](PageHead::render) escapes on the way out.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PageHead {
    /// The document title.
    pub title: Option<String>,
    /// The `meta name="description"` a search result shows.
    pub description: Option<String>,
    /// The `link rel="canonical"` href.
    pub canonical: Option<String>,
    /// Further `meta name="…" content="…"` pairs, in the order they were added.
    pub meta: Vec<(String, String)>,
    /// Further `meta property="…" content="…"` pairs; Open Graph is defined on `property`.
    pub meta_properties: Vec<(String, String)>,
    /// Further `link rel="…" href="…"` pairs, in the order they were added.
    pub links: Vec<(String, String)>,
    /// `link rel="alternate" hreflang="…" href="…"` pairs, in the order they were added.
    pub alternates: Vec<(String, String)>,
    /// JSON-LD objects (schema.org), each serialized, rendered as `<script type="application/ld+json">`.
    pub structured_data: Vec<String>,
    /// The one module script the page runs, with a `modulepreload` for each module beside it.
    pub module: Option<ModuleScript>,
}

impl PageHead {
    /// Whether the page contributed anything at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.title.is_none()
            && self.description.is_none()
            && self.canonical.is_none()
            && self.meta.is_empty()
            && self.meta_properties.is_empty()
            && self.links.is_empty()
            && self.alternates.is_empty()
            && self.structured_data.is_empty()
            && self.module.is_none()
    }

    /// The `<head>` fragment for what the page recorded, escaped; empty when it recorded nothing.
    #[must_use]
    pub fn render(&self) -> String {
        let mut html = String::new();
        if let Some(title) = &self.title {
            html.push_str(&format!("<title>{}</title>", escape(title)));
        }
        if let Some(description) = &self.description {
            html.push_str(&format!(
                "<meta name=\"description\" content=\"{}\">",
                escape(description)
            ));
        }
        if let Some(canonical) = &self.canonical {
            html.push_str(&format!(
                "<link rel=\"canonical\" href=\"{}\">",
                escape(canonical)
            ));
        }
        for (property, content) in &self.meta_properties {
            html.push_str(&format!(
                "<meta property=\"{}\" content=\"{}\">",
                escape(property),
                escape(content)
            ));
        }
        for (name, content) in &self.meta {
            html.push_str(&format!(
                "<meta name=\"{}\" content=\"{}\">",
                escape(name),
                escape(content)
            ));
        }
        for (rel, href) in &self.links {
            html.push_str(&format!(
                "<link rel=\"{}\" href=\"{}\">",
                escape(rel),
                escape(href)
            ));
        }
        for (hreflang, href) in &self.alternates {
            html.push_str(&format!(
                "<link rel=\"alternate\" hreflang=\"{}\" href=\"{}\">",
                escape(hreflang),
                escape(href)
            ));
        }
        for json in &self.structured_data {
            push_json_ld(&mut html, json);
        }
        if let Some(module) = &self.module {
            push_module(&mut html, module);
        }
        html
    }
}

/// A JSON-LD block that nothing inside can end: `<`, `>`, `&`, U+2028 and U+2029 become `\uXXXX`.
fn push_json_ld(html: &mut String, json: &str) {
    html.push_str("<script type=\"application/ld+json\">");
    for c in json.chars() {
        match c {
            '<' => html.push_str("\\u003c"),
            '>' => html.push_str("\\u003e"),
            '&' => html.push_str("\\u0026"),
            '\u{2028}' => html.push_str("\\u2028"),
            '\u{2029}' => html.push_str("\\u2029"),
            other => html.push(other),
        }
    }
    html.push_str("</script>");
}

/// The preloads first, each with its `integrity`, at low priority so the first paint keeps the bandwidth.
fn push_module(html: &mut String, module: &ModuleScript) {
    for (href, integrity) in module.preload() {
        html.push_str(&format!(
            "<link rel=\"modulepreload\" href=\"{}\" integrity=\"{}\" fetchpriority=\"low\">",
            escape(href),
            escape(integrity)
        ));
    }
    html.push_str(&format!(
        "<script type=\"module\" src=\"{}\" integrity=\"{}\" fetchpriority=\"low\"></script>",
        escape(module.src()),
        escape(module.integrity())
    ));
}

#[cfg(test)]
mod tests {
    use super::PageHead;

    #[test]
    fn a_page_that_recorded_nothing_is_empty() {
        assert!(PageHead::default().is_empty());
    }

    #[test]
    fn any_single_contribution_makes_it_non_empty() {
        // Each field is checked on its own: one left out would silently lose its tag.
        let title = PageHead {
            title: Some("A".to_owned()),
            ..PageHead::default()
        };
        let description = PageHead {
            description: Some("A".to_owned()),
            ..PageHead::default()
        };
        let canonical = PageHead {
            canonical: Some("/a".to_owned()),
            ..PageHead::default()
        };
        let meta = PageHead {
            meta: vec![("robots".to_owned(), "noindex".to_owned())],
            ..PageHead::default()
        };
        let properties = PageHead {
            meta_properties: vec![("og:title".to_owned(), "A".to_owned())],
            ..PageHead::default()
        };
        let links = PageHead {
            links: vec![("alternate".to_owned(), "/nl".to_owned())],
            ..PageHead::default()
        };
        let structured = PageHead {
            structured_data: vec!["{}".to_owned()],
            ..PageHead::default()
        };
        for head in [
            title,
            description,
            canonical,
            meta,
            properties,
            links,
            structured,
        ] {
            assert!(!head.is_empty(), "{head:?}");
        }
    }

    #[test]
    fn a_module_renders_its_preloads_then_its_one_script() {
        let head = PageHead {
            module: Some(crate::ModuleScript::new(
                "/m/main.js",
                "sha256-main",
                [("/m/assets.js".to_owned(), "sha256-assets".to_owned())],
            )),
            ..PageHead::default()
        };
        assert!(!head.is_empty());
        assert_eq!(
            head.render(),
            "<link rel=\"modulepreload\" href=\"/m/assets.js\" integrity=\"sha256-assets\" fetchpriority=\"low\">\
             <script type=\"module\" src=\"/m/main.js\" integrity=\"sha256-main\" fetchpriority=\"low\"></script>"
        );
    }

    #[test]
    fn structured_data_cannot_close_its_script() {
        let head = PageHead {
            structured_data: vec![
                r#"{"name":"</script><script>alert(1)</script> & \u2028"}"#
                    .replace("\\u2028", "\u{2028}"),
            ],
            ..PageHead::default()
        };
        let html = head.render();
        assert_eq!(html.matches("</script>").count(), 1, "{html}");
        assert!(
            html.starts_with("<script type=\"application/ld+json\">"),
            "{html}"
        );
        assert!(html.contains(r"\u003c/script\u003e"), "{html}");
        assert!(
            html.contains(r"\u0026") && html.contains(r"\u2028"),
            "{html}"
        );
        let json = html
            .trim_start_matches("<script type=\"application/ld+json\">")
            .trim_end_matches("</script>");
        let parsed: serde_json::Value = serde_json::from_str(json).expect("still JSON");
        assert_eq!(
            parsed["name"].as_str(),
            Some("</script><script>alert(1)</script> & \u{2028}"),
            "the escapes mean the same data"
        );
    }

    #[test]
    fn a_property_renders_as_a_property_and_a_name_as_a_name() {
        let head = PageHead {
            meta: vec![("twitter:card".to_owned(), "summary".to_owned())],
            meta_properties: vec![("og:title".to_owned(), "A page".to_owned())],
            ..PageHead::default()
        };
        let html = head.render();
        assert!(
            html.contains(r#"<meta property="og:title" content="A page">"#),
            "{html}"
        );
        assert!(
            html.contains(r#"<meta name="twitter:card" content="summary">"#),
            "{html}"
        );
        assert!(
            !html.contains(r#"name="og:title""#),
            "an Open Graph tag rendered on `name`, which every consumer ignores: {html}"
        );
    }

    #[test]
    fn a_property_is_escaped_like_everything_else() {
        let head = PageHead {
            meta_properties: vec![(
                "og:title".to_owned(),
                "A \"quoted\" & <angled> title".to_owned(),
            )],
            ..PageHead::default()
        };
        let html = head.render();
        assert!(!html.contains("<angled>"), "{html}");
        assert!(html.contains("&amp;"), "{html}");
    }

    #[test]
    fn an_alternate_renders_with_its_hreflang() {
        let head = PageHead {
            alternates: vec![
                (
                    "nl-NL".to_owned(),
                    "https://example.test/nl/waarom".to_owned(),
                ),
                ("x-default".to_owned(), "https://example.test/".to_owned()),
            ],
            ..PageHead::default()
        };
        let html = head.render();
        assert!(
            html.contains(
                r#"<link rel="alternate" hreflang="nl-NL" href="https://example.test/nl/waarom">"#
            ),
            "{html}"
        );
        assert!(
            html.contains(r#"hreflang="x-default""#),
            "x-default is an ordinary value here: {html}"
        );
    }

    #[test]
    fn an_alternate_is_escaped_like_everything_else() {
        let head = PageHead {
            alternates: vec![(
                "nl\"><script>".to_owned(),
                "/nl/\"><script>alert(1)</script>".to_owned(),
            )],
            ..PageHead::default()
        };
        let html = head.render();
        assert!(!html.contains("<script>"), "{html}");
        assert!(html.contains("&quot;"), "{html}");
    }

    #[test]
    fn an_alternate_alone_makes_the_head_non_empty() {
        let head = PageHead {
            alternates: vec![("nl".to_owned(), "/nl".to_owned())],
            ..PageHead::default()
        };
        assert!(!head.is_empty());
        assert!(!head.render().is_empty());
    }
}
