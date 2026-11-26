use fundamental_core::i18n::Locale;
use serde_json::Map;

use crate::page_context::PageContext;

/// The Open Graph card and schema.org `WebPage` a [`Document`](crate::Document) writes from what the page recorded.
///
/// ```
/// # extern crate fundamental_web;
/// use fundamental_web::{Document, ShareCard};
///
/// let document = Document::new().share(ShareCard::new("Sounds Global"));
/// # let _ = document;
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShareCard {
    site_name: String,
}

impl ShareCard {
    /// The card of a site called `site_name`, as `og:site_name` names it.
    #[must_use]
    pub fn new(site_name: impl Into<String>) -> Self {
        Self {
            site_name: site_name.into(),
        }
    }

    /// Writes the card and the `WebPage` into `ctx`'s head.
    pub(crate) fn write(&self, ctx: &PageContext, locales: &[Locale]) {
        let head = ctx.head();
        let current = ctx
            .locale()
            .or_else(|| locales.first())
            .map(|locale| regional(locale, locales));
        ctx.add_meta_property("og:type", "website");
        ctx.add_meta_property("og:site_name", self.site_name.clone());
        if let Some(title) = &head.title {
            ctx.add_meta_property("og:title", title.clone());
        }
        if let Some(description) = &head.description {
            ctx.add_meta_property("og:description", description.clone());
        }
        if let Some(canonical) = &head.canonical {
            ctx.add_meta_property("og:url", canonical.clone());
        }
        if let Some(current) = &current {
            ctx.add_meta_property("og:locale", open_graph_locale(current));
            for other in locales
                .iter()
                .filter(|other| other.language() != current.language())
            {
                ctx.add_meta_property("og:locale:alternate", open_graph_locale(other));
            }
        }
        if head.canonical.is_none() {
            return;
        }
        let mut fields = Map::new();
        if let Some(title) = head.title {
            fields.insert("name".to_owned(), title.into());
        }
        if let Some(description) = head.description {
            fields.insert("description".to_owned(), description.into());
        }
        if let Some(current) = &current {
            fields.insert("inLanguage".to_owned(), current.language().into());
        }
        ctx.add_structured_data_about("WebPage", fields);
    }
}

/// `locale` with the region the site writes its language in, or as it is.
pub(crate) fn regional(locale: &Locale, locales: &[Locale]) -> Locale {
    locales
        .iter()
        .find(|known| known.language() == locale.language())
        .filter(|known| known.region().is_some())
        .unwrap_or(locale)
        .clone()
}

fn open_graph_locale(locale: &Locale) -> String {
    match locale.region() {
        Some(region) => format!("{}_{}", locale.language(), region.to_ascii_uppercase()),
        None => locale.language().to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fundamental_core::address::{
        AddressSpace, LocalePrefixSet, RequestAddress, SlugTable, WebOrigin,
    };

    fn locales() -> Vec<Locale> {
        vec![Locale::new("en", Some("GB")), Locale::new("nl", Some("NL"))]
    }

    fn dutch_page() -> PageContext {
        PageContext::new(Vec::new(), "req").with_locale(Some(Locale::new("nl", None::<&str>)))
    }

    fn property(ctx: &PageContext, name: &str) -> Vec<String> {
        ctx.head()
            .meta_properties
            .into_iter()
            .filter(|(property, _)| property == name)
            .map(|(_, content)| content)
            .collect()
    }

    #[test]
    fn the_card_names_the_page_in_its_language_and_region() {
        let ctx = dutch_page();
        ctx.set_title("Missie");
        ctx.set_description("Waarom we bouwen.");
        ShareCard::new("Sounds Global").write(&ctx, &locales());
        assert_eq!(property(&ctx, "og:site_name"), ["Sounds Global"]);
        assert_eq!(property(&ctx, "og:title"), ["Missie"]);
        assert_eq!(property(&ctx, "og:description"), ["Waarom we bouwen."]);
        assert_eq!(property(&ctx, "og:locale"), ["nl_NL"]);
        assert_eq!(property(&ctx, "og:locale:alternate"), ["en_GB"]);
    }

    #[test]
    fn a_page_without_a_canonical_url_is_named_in_no_structured_data() {
        let ctx = dutch_page();
        ctx.set_title("Niet gevonden");
        ShareCard::new("Sounds Global").write(&ctx, &locales());
        assert!(ctx.head().structured_data.is_empty());
        assert!(property(&ctx, "og:url").is_empty());
    }

    #[test]
    fn a_page_with_a_canonical_url_is_named_a_web_page() {
        let slugs = SlugTable::builder().build().expect("well formed");
        let space = AddressSpace::new(LocalePrefixSet::new(), slugs)
            .with_origin(WebOrigin::parse("https://example.com").expect("an origin"));
        let address = RequestAddress::new(
            std::sync::Arc::new(space),
            Locale::new("nl", None::<&str>),
            "",
            "/mission",
        );
        let ctx = dutch_page().with_address(Some(address));
        ctx.set_title("Missie");
        ctx.set_canonical("https://example.com/mission");
        ShareCard::new("Sounds Global").write(&ctx, &locales());
        let data = ctx.head().structured_data;
        assert_eq!(data.len(), 1, "{data:?}");
        assert!(data[0].contains("\"@type\":\"WebPage\""), "{}", data[0]);
        assert!(data[0].contains("\"inLanguage\":\"nl\""), "{}", data[0]);
        assert!(data[0].contains("\"name\":\"Missie\""), "{}", data[0]);
    }
}
